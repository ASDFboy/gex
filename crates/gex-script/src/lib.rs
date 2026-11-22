//! gex-script: runs .jlb scripts on objects.
//!
//! Every object can have any number of scripts. A script is a .jlb file; its outside
//! variables are that object's instance variables (one copy per object, like GameMaker's
//! instance variables), and `self` is a table of the object's properties that writes
//! back after each event. Events are functions with fixed names:
//!
//! | Function                         | When it runs                                    |
//! |----------------------------------|-------------------------------------------------|
//! | (top-level code)                 | when the script is attached                     |
//! | `on_create()`                    | once the whole blueprint is loaded / after SPAWN (and its starting values) |
//! | `on_step(dt)`                    | every frame                                     |
//! | `on_physics(dt)`                 | 60 times a second, before collisions            |
//! | `on_destroy()`                   | before the object is removed                      |
//! | `on_key_pressed(key)` / `on_key_released(key)` | any key                           |
//! | `on_press_<key>()` / `on_release_<key>()` | one key: `on_press_space()`            |
//! | `on_mouse_pressed(button, x, y)` | a mouse click                                   |
//! | `on_mouse_enter()` / `on_mouse_leave()` | the mouse over / off a 2D object             |
//! | `on_clicked(b)` / `on_click_held(b)` / `on_click_released(b)` | a button on a 2D object |
//! | `on_collide_start(other)` / `on_collide(other)` / `on_collide_end(other)` | Collider2D overlaps |
//! | `on_timer(name)` / `on_timer_<name>()` | a TIMER finishing                         |
//! | `on_<event>(...)`                | SEND(object, "<event>", ...) or EMIT("<event>", ...) |
//! | `when_<name>()` → `on_<name>()`  | a condition: on_<name> runs when when_<name> turns TRUE |

pub mod audio;
pub mod command_guide;
pub mod commands;
pub mod convert;
pub mod extras;
pub mod hit;
pub mod net;
pub mod particles;
pub mod tween;
pub mod physics;
pub mod thread;
pub mod wire;
pub mod world;

pub use world::{FrameInput, Game, LogKind, LogLine, UiEvent};

#[cfg(test)]
mod tests;
