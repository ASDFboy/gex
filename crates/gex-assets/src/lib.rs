//! gex-assets: where asset bytes come from.
//!
//! Paths look like `res://sprites/hero.png` (inside the project) or `<scheme>://...`
//! for a mount, e.g. `hl2://materials/brick/brickwall001a.vmt` read straight from a
//! local Half-Life 2 install. Mounted files are always read in place: nothing here
//! copies them, and `Vfs::redistributable` lets export tools leave them out.

pub mod builtin;
pub mod formats;
pub mod image_load;
pub mod model;
pub mod source;
pub mod text;
pub mod vfs;
pub mod vmt;
pub mod vpk;
pub mod vtf;

pub use image_load::{decode_image, image_size, load_image, Rgba};
pub use vfs::{Entry, Vfs};

#[cfg(test)]
mod tests;
