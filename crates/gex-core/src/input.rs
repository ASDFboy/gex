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
    pub mouse_released: HashSet<String>,
    /// Input actions from the project: name → keys (see ProjectConfig::actions).
    pub actions: std::collections::BTreeMap<String, Vec<String>>,
}

impl Input {
    /// Keys by name; mouse buttons as "mouse_left", "mouse_right", "mouse_middle" (so the
    /// left arrow and the left button don't get mixed up).
    pub fn key_down(&self, k: &str) -> bool {
        match k.strip_prefix("mouse_") {
            Some(b) => self.mouse_down.contains(b),
            None => self.down.contains(k),
        }
    }
    pub fn key_pressed(&self, k: &str) -> bool {
        match k.strip_prefix("mouse_") {
            Some(b) => self.mouse_pressed.contains(b),
            None => self.pressed.contains(k),
        }
    }
    pub fn key_released(&self, k: &str) -> bool {
        match k.strip_prefix("mouse_") {
            Some(b) => self.mouse_released.contains(b),
            None => self.released.contains(k),
        }
    }
    /// Applies one frame of events: what's down now, given what was down before.
    pub fn next_frame(&mut self, mut now_down: HashSet<String>, mouse: [f32; 2], mouse_down: HashSet<String>) {
        // An action is down while any of its keys is, so it gets pressed/released like a key.
        for (name, keys) in &self.actions {
            let on = keys.iter().any(|k| match k.strip_prefix("mouse_") {
                Some(b) => mouse_down.contains(b),
                None => now_down.contains(k.as_str()),
            });
            if on {
                now_down.insert(name.clone());
            }
        }
        self.pressed = now_down.difference(&self.down).cloned().collect();
        self.released = self.down.difference(&now_down).cloned().collect();
        self.mouse_pressed = mouse_down.difference(&self.mouse_down).cloned().collect();
        self.mouse_released = self.mouse_down.difference(&mouse_down).cloned().collect();
        self.down = now_down;
        self.mouse = mouse;
        self.mouse_down = mouse_down;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_and_mouse_names() {
        let mut i = Input::default();
        i.actions.insert("jump".into(), vec!["space".into(), "pad_a".into(), "mouse_right".into()]);
        i.next_frame(["pad_a".to_string()].into(), [0.0; 2], HashSet::new());
        assert!(i.key_down("jump") && i.key_pressed("jump"));
        i.next_frame(["space".to_string()].into(), [0.0; 2], HashSet::new());
        assert!(i.key_down("jump") && !i.key_pressed("jump"), "still held through another key");
        i.next_frame(HashSet::new(), [0.0; 2], ["right".to_string()].into());
        assert!(i.key_down("jump") && i.key_down("mouse_right") && !i.key_down("right"), "the right button isn't the right arrow");
        i.next_frame(HashSet::new(), [0.0; 2], HashSet::new());
        assert!(i.key_released("jump") && i.key_released("mouse_right"));
    }
}
