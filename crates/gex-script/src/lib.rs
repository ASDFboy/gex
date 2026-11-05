//! gex-script: runs .jlb scripts on nodes.
//!
//! Every node can have any number of scripts. A script is a .jlb file; its outside
//! variables are that node's instance variables (one copy per node, like GameMaker's
//! instance variables), and `self` is a table of the node's properties that writes
//! back after each event. Events are functions with fixed names:
//!
//! | Function                         | When it runs                                    |
//! |----------------------------------|-------------------------------------------------|
//! | (top-level code)                 | when the script is attached                     |
//! | `on_create()`                    | once the whole scene is loaded / after SPAWN    |
//! | `on_step(dt)`                    | every frame                                     |
//! | `on_physics(dt)`                 | 60 times a second, before collisions            |
//! | `on_destroy()`                   | before the node is removed                      |
//! | `on_key_pressed(key)` / `on_key_released(key)` | any key                           |
//! | `on_press_<key>()` / `on_release_<key>()` | one key: `on_press_space()`            |
//! | `on_mouse_pressed(button, x, y)` | a mouse click                                   |
//! | `on_collide_start(other)` / `on_collide(other)` / `on_collide_end(other)` | Collider2D overlaps |
//! | `on_timer(name)` / `on_timer_<name>()` | a TIMER finishing                         |
//! | `on_<event>(...)`                | SEND(node, "<event>", ...) or EMIT("<event>", ...) |
//! | `when_<name>()` → `on_<name>()`  | a condition: on_<name> runs when when_<name> turns TRUE |

pub mod commands;
pub mod convert;
pub mod thread;
pub mod world;

pub use world::{Game, LogKind, LogLine};

#[cfg(test)]
mod tests;
