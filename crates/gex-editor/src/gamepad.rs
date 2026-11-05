//! Gamepads (gilrs): buttons become keys named "pad_a", "pad_b"... and sticks become
//! axes for AXIS("left_x") and friends. Any connected pad counts.

use std::collections::{HashMap, HashSet};

use gilrs::{Axis, Button, Gilrs};

pub struct Pads {
    gilrs: Option<Gilrs>,
}

const BUTTONS: &[(Button, &str)] = &[
    (Button::South, "pad_a"),
    (Button::East, "pad_b"),
    (Button::West, "pad_x"),
    (Button::North, "pad_y"),
    (Button::LeftTrigger, "pad_lb"),
    (Button::RightTrigger, "pad_rb"),
    (Button::LeftTrigger2, "pad_lt"),
    (Button::RightTrigger2, "pad_rt"),
    (Button::Select, "pad_select"),
    (Button::Start, "pad_start"),
    (Button::LeftThumb, "pad_ls"),
    (Button::RightThumb, "pad_rs"),
    (Button::DPadUp, "pad_up"),
    (Button::DPadDown, "pad_down"),
    (Button::DPadLeft, "pad_left"),
    (Button::DPadRight, "pad_right"),
];

impl Pads {
    pub fn new() -> Pads {
        Pads { gilrs: Gilrs::new().ok() }
    }

    /// What's held now, and the sticks/triggers (y grows downward, like the 2D world).
    pub fn poll(&mut self) -> (HashSet<String>, HashMap<String, f32>) {
        let mut keys = HashSet::new();
        let mut axes = HashMap::new();
        let Some(g) = &mut self.gilrs else { return (keys, axes) };
        while g.next_event().is_some() {}
        for (_, pad) in g.gamepads() {
            for (b, name) in BUTTONS {
                if pad.is_pressed(*b) {
                    keys.insert(name.to_string());
                }
            }
            let dead = |v: f32| if v.abs() < 0.12 { 0.0 } else { v };
            for (a, name, flip) in [(Axis::LeftStickX, "left_x", 1.0), (Axis::LeftStickY, "left_y", -1.0), (Axis::RightStickX, "right_x", 1.0), (Axis::RightStickY, "right_y", -1.0)] {
                let v = dead(pad.value(a)) * flip;
                let e = axes.entry(name.to_string()).or_insert(0.0f32);
                if v.abs() > e.abs() {
                    *e = v;
                }
            }
            for (b, name) in [(Button::LeftTrigger2, "lt"), (Button::RightTrigger2, "rt")] {
                let v = pad.button_data(b).map(|d| d.value()).unwrap_or(0.0);
                let e = axes.entry(name.to_string()).or_insert(0.0f32);
                *e = e.max(v);
            }
        }
        (keys, axes)
    }
}
