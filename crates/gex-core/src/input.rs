//! Keyboard and mouse state, using the same key names as .jlb's own graphics
//! ("a".."z", "0".."9", "f1".., "space", "enter", "escape", "left", "shift", ...).

use std::collections::HashSet;

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub down: HashSet<String>,
    /// Went down / up since the last frame.
    pub pressed: HashSet<String>,
    pub released: HashSet<String>,
    /// Mouse position in game coordinates (2D world pixels), and buttons
    /// ("left", "right", "middle").
    pub mouse: [f32; 2],
    pub mouse_down: HashSet<String>,
    pub mouse_pressed: HashSet<String>,
}

impl Input {
    pub fn key_down(&self, k: &str) -> bool {
        self.down.contains(k) || self.mouse_down.contains(k)
    }
    pub fn key_pressed(&self, k: &str) -> bool {
        self.pressed.contains(k) || self.mouse_pressed.contains(k)
    }
    pub fn key_released(&self, k: &str) -> bool {
        self.released.contains(k)
    }
    /// Applies one frame of events: what's down now, given what was down before.
    pub fn next_frame(&mut self, now_down: HashSet<String>, mouse: [f32; 2], mouse_down: HashSet<String>) {
        self.pressed = now_down.difference(&self.down).cloned().collect();
        self.released = self.down.difference(&now_down).cloned().collect();
        self.mouse_pressed = mouse_down.difference(&self.mouse_down).cloned().collect();
        self.down = now_down;
        self.mouse = mouse;
        self.mouse_down = mouse_down;
    }
}
