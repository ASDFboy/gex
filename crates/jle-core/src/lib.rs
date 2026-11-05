//! jle-core: the parts of the engine everything else shares. No graphics, no scripting
//! here: just the node tree, scene files, projects, input state and render extraction.

pub mod input;
pub mod node;
pub mod project;
pub mod render;
pub mod scene;
pub mod tree;
pub mod variant;

pub use glam;
pub use node::{Node, NodeId, NodeKind, Transform};
pub use tree::SceneTree;
pub use variant::Variant;

#[cfg(test)]
mod tests;
