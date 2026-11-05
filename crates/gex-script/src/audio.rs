//! Sound, with rodio. Lives on the game thread. With no sound device (a server, a test
//! machine), everything still works, silently.

use std::collections::HashMap;
use std::io::Cursor;

use gex_assets::Vfs;

pub struct Audio {
    device: Option<rodio::MixerDeviceSink>,
    tried: bool,
    players: HashMap<u64, rodio::Player>,
    /// Sound node → the sound it's playing.
    pub by_node: HashMap<gex_core::NodeId, u64>,
    music: Option<rodio::Player>,
    next: u64,
    pub master: f32,
    /// Decoded-file cache: path → bytes.
    cache: HashMap<String, std::sync::Arc<Vec<u8>>>,
}

impl Default for Audio {
    fn default() -> Self {
        Audio { device: None, tried: false, players: HashMap::new(), by_node: HashMap::new(), music: None, next: 1, master: 1.0, cache: HashMap::new() }
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
        let bytes = self.bytes(vfs, path)?;
        let master = self.master;
        let Some(dev) = self.device() else {
            // No sound device: still check the file can be decoded, so problems show up.
            rodio::Decoder::new(Cursor::new(Bytes(bytes))).map_err(|e| format!("{}: {}", path, e))?;
            return Ok(0);
        };
        let player = rodio::Player::connect_new(dev.mixer());
        let cur = Cursor::new(Bytes(bytes));
        if looped {
            let d = rodio::Decoder::new_looped(cur).map_err(|e| format!("{}: {}", path, e))?;
            player.append(d);
        } else {
            let d = rodio::Decoder::new(cur).map_err(|e| format!("{}: {}", path, e))?;
            player.append(d);
        }
        player.set_volume(volume * master);
        player.set_speed(pitch.max(0.01));
        let id = self.next;
        self.next += 1;
        self.players.insert(id, player);
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
        self.by_node.clear();
    }

    pub fn playing(&self, id: u64) -> bool {
        self.players.get(&id).is_some_and(|p| !p.empty())
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
        self.by_node.retain(|_, id| live.contains(id));
    }
}
