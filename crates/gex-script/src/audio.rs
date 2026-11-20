//! Sound, with rodio. Lives on the game thread. With no sound device (a server, a test
//! machine), everything still works, silently.

use std::collections::HashMap;
use std::io::Cursor;
use std::num::NonZero;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use rodio::Source;

/// Plays a sound in stereo with a left/right balance that can change while it plays
/// (-1 left, 0 middle, 1 right).
struct Panned<S: Source> {
    inner: S,
    pan: Arc<AtomicU32>,
    frame: [f32; 2],
    side: usize,
}

impl<S: Source> Iterator for Panned<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.side == 0 {
            let ch = self.inner.channels().get();
            let l = self.inner.next()?;
            let r = if ch >= 2 { self.inner.next().unwrap_or(l) } else { l };
            for _ in 2..ch {
                self.inner.next();
            }
            // Equal-power balance, both sides at full in the middle.
            let p = f32::from_bits(self.pan.load(Ordering::Relaxed)).clamp(-1.0, 1.0);
            let a = (p + 1.0) * std::f32::consts::FRAC_PI_4;
            let (gl, gr) = (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2);
            self.frame = [l * gl.min(1.0), r * gr.min(1.0)];
        }
        let v = self.frame[self.side];
        self.side = (self.side + 1) % 2;
        Some(v)
    }
}

impl<S: Source> Source for Panned<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

/// Tracker music (.mod, .xm, .s3m, .it), played by the module player. It plays from the
/// module it owns: the player borrows it, so the module is boxed (it never moves) and the
/// player is dropped first (fields drop in order).
struct Tracker {
    player: xmrsplayer::prelude::XmrsPlayer<'static>,
    _module: Box<xmrs::prelude::Module>,
    rate: u32,
}

impl Tracker {
    fn new(bytes: &[u8], looped: bool) -> Result<Tracker, String> {
        let module = Box::new(xmrs::prelude::Module::load(bytes).map_err(|e| format!("{:?}", e))?);
        let rate = 48_000;
        // SAFETY: the module lives in its box for as long as the player (see above).
        let m: &'static xmrs::prelude::Module = unsafe { &*(module.as_ref() as *const xmrs::prelude::Module) };
        let mut player = xmrsplayer::prelude::XmrsPlayer::new(m, rate, 0);
        // 0 loops forever; 1 plays the song once.
        player.set_max_loop_count(if looped { 0 } else { 1 });
        Ok(Tracker { player, _module: module, rate })
    }
}

impl Iterator for Tracker {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.player.next().map(|s| s as f32 / 32768.0)
    }
}

impl Source for Tracker {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Opus (.opus, or Opus in .ogg/.webm/.mka when the usual decoder can't): decoded by the
/// pure-Rust Opus decoder through Symphonia 0.6, a packet at a time.
struct Opus {
    bytes: std::sync::Arc<Vec<u8>>,
    looped: bool,
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::audio::AudioDecoder>,
    track: u32,
    buf: Vec<f32>,
    at: usize,
    rate: u32,
    channels: u16,
}

impl Opus {
    fn open(bytes: &std::sync::Arc<Vec<u8>>) -> Result<(Box<dyn symphonia::core::formats::FormatReader>, Box<dyn symphonia::core::codecs::audio::AudioDecoder>, u32, u32, u16), String> {
        use symphonia::core::codecs::audio::AudioDecoderOptions;
        use symphonia::core::codecs::registry::CodecRegistry;
        use symphonia::core::common::Tier;
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::{FormatOptions, TrackType};
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        let mut codecs = CodecRegistry::new();
        codecs.register_audio_decoder_at_tier::<symphonia_opus::OpusDecoder>(Tier::Fallback);
        let mss = MediaSourceStream::new(Box::new(Cursor::new(Bytes(bytes.clone()))), Default::default());
        let format = symphonia::default::get_probe().probe(&Hint::new(), mss, FormatOptions::default(), MetadataOptions::default()).map_err(|e| e.to_string())?;
        let track = format.default_track(TrackType::Audio).ok_or("no sound in it")?;
        let id = track.id;
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("no sound in it")?.clone();
        let decoder = codecs.make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(|e| e.to_string())?;
        let rate = params.sample_rate.unwrap_or(48_000);
        let channels = params.channels.as_ref().map(|c| c.count() as u16).unwrap_or(2).max(1);
        Ok((format, decoder, id, rate, channels))
    }

    fn new(bytes: std::sync::Arc<Vec<u8>>, looped: bool) -> Result<Opus, String> {
        let (format, decoder, track, rate, channels) = Opus::open(&bytes)?;
        let mut o = Opus { bytes, looped, format, decoder, track, buf: Vec::new(), at: 0, rate, channels };
        // The first packet tells the real rate and channels.
        o.refill();
        Ok(o)
    }

    /// Decodes the next packet into `buf`; false at the end.
    fn refill(&mut self) -> bool {
        for _ in 0..10_000 {
            match self.format.next_packet() {
                Ok(Some(p)) if p.track_id == self.track => {
                    let Ok(d) = self.decoder.decode(&p) else { continue };
                    self.rate = d.spec().rate();
                    self.channels = (d.spec().channels().count() as u16).max(1);
                    self.buf.clear();
                    d.copy_to_vec_interleaved::<f32>(&mut self.buf);
                    self.at = 0;
                    if !self.buf.is_empty() {
                        return true;
                    }
                }
                Ok(Some(_)) => {}
                _ => {
                    if !self.looped {
                        return false;
                    }
                    let Ok((f, d, t, _, _)) = Opus::open(&self.bytes) else { return false };
                    (self.format, self.decoder, self.track) = (f, d, t);
                }
            }
        }
        false
    }
}

impl Iterator for Opus {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buf.len() && !self.refill() {
            return None;
        }
        self.at += 1;
        Some(self.buf[self.at - 1])
    }
}

impl Source for Opus {
    fn current_span_len(&self) -> Option<usize> {
        // The rate and channels hold for the rest of this packet.
        Some(self.buf.len() - self.at.min(self.buf.len()))
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(self.channels).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate.max(1)).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// A sound that stops (instead of taking the sound thread down) if its decoder panics on a
/// broken file: some decoders do on damaged data.
struct Guarded {
    inner: Option<Box<dyn Source + Send>>,
    channels: rodio::ChannelCount,
    rate: rodio::SampleRate,
}

impl Iterator for Guarded {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let inner = self.inner.as_mut()?;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| inner.next())) {
            Ok(s) => s,
            Err(_) => {
                self.inner = None;
                None
            }
        }
    }
}

impl Source for Guarded {
    fn current_span_len(&self) -> Option<usize> {
        match &self.inner {
            Some(i) => i.current_span_len(),
            None => Some(0),
        }
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.as_ref().map_or(self.channels, |i| i.channels())
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.as_ref().map_or(self.rate, |i| i.sample_rate())
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Decodes a sound file (any format the audio decoders know, or tracker music).
fn decode(bytes: std::sync::Arc<Vec<u8>>, path: &str, looped: bool) -> Result<Box<dyn Source + Send>, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decode_unguarded(bytes, path, looped))) {
        Ok(Ok(src)) => {
            let (channels, rate) = (src.channels(), src.sample_rate());
            Ok(Box::new(Guarded { inner: Some(src), channels, rate }))
        }
        Ok(Err(e)) => Err(e),
        Err(_) => Err(format!("{}: a damaged sound file", path)),
    }
}

fn decode_unguarded(bytes: std::sync::Arc<Vec<u8>>, path: &str, looped: bool) -> Result<Box<dyn Source + Send>, String> {
    if gex_assets::formats::is_tracker(path) {
        return Ok(Box::new(Tracker::new(&bytes, looped).map_err(|e| format!("{}: {}", path, e))?));
    }
    let len = bytes.len() as u64;
    let bytes2 = bytes.clone();
    let b = rodio::Decoder::builder().with_data(Cursor::new(Bytes(bytes))).with_byte_len(len).with_seekable(true);
    let ext = gex_assets::formats::extension(path);
    let b = if ext.is_empty() { b } else { b.with_hint(&ext) };
    let err = |e: rodio::decoder::DecoderError| format!("{}: {}", path, e);
    let decoded: Result<Box<dyn Source + Send>, String> = if looped { b.build_looped().map(|d| Box::new(d) as Box<dyn Source + Send>).map_err(err) } else { b.build().map(|d| Box::new(d) as Box<dyn Source + Send>).map_err(err) };
    // Opus isn't one the usual decoder knows.
    decoded.or_else(|e| match Opus::new(bytes2, looped) {
        Ok(o) => Ok(Box::new(o) as Box<dyn Source + Send>),
        Err(_) => Err(e),
    })
}

/// Extras for a sound: balance (shared, so it can move) and an echo (delay seconds, loudness).
#[derive(Default, Clone)]
pub struct SoundFx {
    pub pan: f32,
    pub echo: Option<(f32, f32)>,
}

use gex_assets::Vfs;

pub struct Audio {
    device: Option<rodio::MixerDeviceSink>,
    tried: bool,
    players: HashMap<u64, rodio::Player>,
    /// Sound object → the sound it's playing.
    pub by_object: HashMap<gex_core::ObjectId, u64>,
    music: Option<rodio::Player>,
    /// Each playing sound's balance.
    pans: HashMap<u64, Arc<AtomicU32>>,
    next: u64,
    pub master: f32,
    /// Decoded-file cache: path → bytes.
    cache: HashMap<String, std::sync::Arc<Vec<u8>>>,
}

impl Default for Audio {
    fn default() -> Self {
        Audio { device: None, tried: false, players: HashMap::new(), by_object: HashMap::new(), music: None, pans: HashMap::new(), next: 1, master: 1.0, cache: HashMap::new() }
    }
}

struct Bytes(std::sync::Arc<Vec<u8>>);
impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Audio {
    fn device(&mut self) -> Option<&rodio::MixerDeviceSink> {
        if !self.tried {
            self.tried = true;
            self.device = rodio::DeviceSinkBuilder::open_default_sink().ok().map(|mut d| {
                d.log_on_drop(false);
                d
            });
        }
        self.device.as_ref()
    }

    fn bytes(&mut self, vfs: &Vfs, path: &str) -> Result<std::sync::Arc<Vec<u8>>, String> {
        if let Some(b) = self.cache.get(path) {
            return Ok(b.clone());
        }
        let b = std::sync::Arc::new(vfs.read(path)?);
        self.cache.insert(path.to_string(), b.clone());
        Ok(b)
    }

    /// Starts a sound. Gives its id (0 when there's no sound device).
    pub fn play(&mut self, vfs: &Vfs, path: &str, volume: f32, pitch: f32, looped: bool) -> Result<u64, String> {
        self.play_fx(vfs, path, volume, pitch, looped, SoundFx::default())
    }

    /// Like `play`, with balance and echo.
    pub fn play_fx(&mut self, vfs: &Vfs, path: &str, volume: f32, pitch: f32, looped: bool, fx: SoundFx) -> Result<u64, String> {
        let bytes = self.bytes(vfs, path)?;
        let master = self.master;
        let Some(dev) = self.device() else {
            // No sound device: still check the file can be decoded, so problems show up.
            decode(bytes, path, false)?;
            return Ok(0);
        };
        let player = rodio::Player::connect_new(dev.mixer());
        let src = decode(bytes, path, looped)?;
        let src: Box<dyn Source + Send> = match fx.echo {
            Some((delay, amount)) if amount > 0.0 => Box::new(src.buffered().reverb(std::time::Duration::from_secs_f32(delay.clamp(0.01, 2.0)), amount.clamp(0.0, 0.95))),
            _ => src,
        };
        let pan = Arc::new(AtomicU32::new(fx.pan.to_bits()));
        player.append(Panned { inner: src, pan: pan.clone(), frame: [0.0; 2], side: 0 });
        player.set_volume(volume * master);
        player.set_speed(pitch.max(0.01));
        let id = self.next;
        self.next += 1;
        self.players.insert(id, player);
        self.pans.insert(id, pan);
        Ok(id)
    }

    pub fn stop(&mut self, id: u64) {
        if let Some(p) = self.players.remove(&id) {
            p.stop();
        }
    }

    pub fn stop_all(&mut self) {
        for (_, p) in self.players.drain() {
            p.stop();
        }
        if let Some(m) = self.music.take() {
            m.stop();
        }
        self.by_object.clear();
    }

    pub fn playing(&self, id: u64) -> bool {
        self.players.get(&id).is_some_and(|p| !p.empty())
    }

    pub fn set_pan(&mut self, id: u64, pan: f32) {
        if let Some(p) = self.pans.get(&id) {
            p.store(pan.to_bits(), Ordering::Relaxed);
        }
    }

    pub fn set_volume(&mut self, id: u64, v: f32) {
        if let Some(p) = self.players.get(&id) {
            p.set_volume(v * self.master);
        }
    }

    /// Plays a looping music track, replacing the last one ("" stops the music).
    pub fn music(&mut self, vfs: &Vfs, path: &str, volume: f32) -> Result<(), String> {
        if let Some(m) = self.music.take() {
            m.stop();
        }
        if path.is_empty() {
            return Ok(());
        }
        let id = self.play(vfs, path, volume, 1.0, true)?;
        self.music = self.players.remove(&id);
        Ok(())
    }

    /// Drops finished sounds.
    pub fn tidy(&mut self) {
        self.players.retain(|_, p| !p.empty());
        let live: std::collections::HashSet<u64> = self.players.keys().copied().collect();
        self.pans.retain(|k, _| live.contains(k));
        self.by_object.retain(|_, id| live.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panning_moves_sound_between_sides() {
        let mono = rodio::buffer::SamplesBuffer::new(NonZero::new(1).unwrap(), NonZero::new(44100).unwrap(), vec![0.5f32; 4]);
        let pan = Arc::new(AtomicU32::new((-1.0f32).to_bits()));
        let mut p = Panned { inner: mono, pan: pan.clone(), frame: [0.0; 2], side: 0 };
        assert_eq!(p.channels().get(), 2);
        let (l, r) = (p.next().unwrap(), p.next().unwrap());
        assert!((l - 0.5).abs() < 1e-4 && r.abs() < 1e-4, "hard left: {} {}", l, r);
        pan.store(0.0f32.to_bits(), Ordering::Relaxed);
        let (l, r) = (p.next().unwrap(), p.next().unwrap());
        assert!((l - 0.5).abs() < 1e-3 && (r - 0.5).abs() < 1e-3, "middle: {} {}", l, r);
    }
}

#[cfg(test)]
mod format_tests {
    /// Decodes every sound file under $GEX_AUDIO (a second's worth each):
    /// `GEX_AUDIO=dir cargo test --release -p gex-script audio_corpus -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn audio_corpus() {
        let Ok(dir) = std::env::var("GEX_AUDIO") else { return };
        let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !gex_assets::formats::is_audio(&name) {
                println!("skip {} (not a sound GEX knows)", name);
                continue;
            }
            let bytes = std::sync::Arc::new(std::fs::read(&p).unwrap());
            match super::decode(bytes, &name, false) {
                Ok(src) => {
                    let (rate, ch) = (src.sample_rate().get(), src.channels().get());
                    let n = src.take(48_000 * 2).filter(|s| s.abs() > 0.001).count();
                    println!("ok   {}: {} Hz, {} ch, {} loud samples in the first second", name, rate, ch, n);
                }
                Err(e) => println!("FAIL {}: {}", name, e),
            }
        }
    }

    /// Broken sound files give an error or silence, never a crash.
    #[test]
    #[ignore]
    fn audio_fuzz() {
        let Ok(dir) = std::env::var("GEX_AUDIO") else { return };
        let mut seed = 99u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let (mut tries, mut panics) = (0, 0);
        for p in std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()) {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !gex_assets::formats::is_audio(&name) {
                continue;
            }
            let bytes = std::fs::read(&p).unwrap();
            let mut variants: Vec<Vec<u8>> = [0.0, 0.05, 0.2, 0.5, 0.9].iter().map(|k| bytes[..(bytes.len() as f64 * k) as usize].to_vec()).collect();
            for _ in 0..15 {
                let mut b = bytes.clone();
                for _ in 0..1 + rnd() % 10 {
                    let i = (rnd() as usize) % b.len();
                    b[i] = rnd() as u8;
                }
                variants.push(b);
            }
            for (i, v) in variants.into_iter().enumerate() {
                tries += 1;
                let r = std::panic::catch_unwind(|| {
                    if let Ok(src) = super::decode(std::sync::Arc::new(v), &name, false) {
                        let _ = src.take(200_000).count();
                    }
                });
                if r.is_err() {
                    panics += 1;
                    println!("PANIC {} variant {}", name, i);
                }
            }
        }
        println!("{} tries, {} panics", tries, panics);
        assert_eq!(panics, 0);
    }
}
