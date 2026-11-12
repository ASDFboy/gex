//! gex-core: the parts of the engine everything else shares. No graphics, no scripting
//! here: just the object tree, blueprint files, projects, input state and render extraction.

pub mod anim;
pub mod input;
pub mod object;
pub mod project;
pub mod props;
pub mod render;
pub mod blueprint;
pub mod settings;
pub mod tiles;
pub mod tree;
pub mod variant;

pub use glam;
pub use object::{Object, ObjectId, ObjectKind, Transform};
pub use tree::BlueprintTree;
pub use variant::Variant;

#[cfg(test)]
mod tests;
