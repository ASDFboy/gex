// The .jlb interpreter as a library. The `jlb` program (src/main.rs) is one user of it;
// a game engine or editor can be another (see embed.rs and host.rs).

pub mod ast;
pub mod builtins;
pub mod debug;
pub mod embed;
pub mod graphics;
pub mod host;
pub mod ide;
pub mod interp;
pub mod lexer;
pub mod number;
pub mod ops;
pub mod parser;
pub mod tasks;
pub mod value;

pub use interp::Interp;
pub use value::{Env, Fault, Value, R};
