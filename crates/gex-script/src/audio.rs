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
            rodio::Decoder::new(Cursor::new(Bytes(bytes))).map_err(|e| format!("{}: {}", path, e))?;
            return Ok(0);
        };
        let player = rodio::Player::connect_new(dev.mixer());
        let cur = Cursor::new(Bytes(bytes));
        let src: Box<dyn Source + Send> = if looped {
            Box::new(rodio::Decoder::new_looped(cur).map_err(|e| format!("{}: {}", path, e))?)
        } else {
            Box::new(rodio::Decoder::new(cur).map_err(|e| format!("{}: {}", path, e))?)
        };
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
