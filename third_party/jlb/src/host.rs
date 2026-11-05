// Lets another program (a game engine, an editor) run .jlb code with commands of its own.
//
// A host registers its command names once per thread with `register`, then gives the
// interpreter a `Host` (Interp::host). Host commands follow the language's own rule:
// ALL CAPS, like every built-in. A host may also take over a built-in's name (a game
// engine answering KEY_DOWN from its own input, say); anything it doesn't handle falls
// through to the built-in command.

use std::cell::RefCell;

use crate::interp::Interp;
use crate::value::*;

thread_local! {
    static NAMES: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Adds command names the lexer and parser accept on this thread. Each name must be
/// ALL CAPS letters, digits and `_`. Registering a name twice is harmless.
pub fn register(names: &[&str]) {
    NAMES.with(|n| {
        let mut n = n.borrow_mut();
        for name in names {
            assert!(
                name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "host command names must be ALL CAPS: {}",
                name
            );
            if !n.contains(name) {
                n.push(Box::leak(name.to_string().into_boxed_str()));
            }
        }
    });
}

/// The registered name equal to `w`, if any.
pub fn find(w: &str) -> Option<&'static str> {
    NAMES.with(|n| n.borrow().iter().find(|k| **k == w).copied())
}

/// Every registered host command name (for editors: highlighting, completion).
pub fn names() -> Vec<&'static str> {
    NAMES.with(|n| n.borrow().clone())
}

pub trait Host {
    /// Runs the command `name`, or gives the inputs back (`Err`) to let the built-in
    /// command of that name run instead. While a host command runs, the host is out of
    /// the interpreter, so `.jlb` code it calls back into reaches only the built-ins.
    fn call(&mut self, it: &mut Interp, name: &'static str, args: Vec<Value>) -> Result<R<Value>, Vec<Value>>;
}

pub(crate) fn try_host(it: &mut Interp, name: &'static str, args: Vec<Value>) -> Result<R<Value>, Vec<Value>> {
    let Some(mut h) = it.host.take() else { return Err(args) };
    let r = h.call(it, name, args);
    it.host = Some(h);
    r
}
