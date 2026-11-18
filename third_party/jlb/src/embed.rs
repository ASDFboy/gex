// Running .jlb code from inside another program.
//
// The usual shape (a game engine, say):
//
//     jlb::host::register(&["SPAWN", "EMIT"]);        // its own commands, once per thread
//     let mut it = Interp::embedded(project_dir);
//     it.host = Some(Box::new(my_host));
//     let env = it.load_module(path, &[("self", me)])?; // one file = one set of variables
//     if let Some(f) = Interp::env_get(&env, "on_step") {
//         it.call_value(&f, vec![Value::Num(dt)])?;
//     }
//
// A file loaded with load_module keeps its outside variables in its own Env for as long
// as the host holds it, so the same file loaded twice gives two independent copies.

use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::interp::Interp;
use crate::number::Number;
use crate::value::*;

impl Interp {
    /// An interpreter ready to load files and call their functions, with `dir` as the
    /// folder IMPORT and LOAD start from when no file is running.
    pub fn embedded(dir: PathBuf) -> Interp {
        let mut it = Interp::new();
        it.start_session(dir);
        it
    }

    /// Sends PRINT's output to `w` instead of standard output.
    pub fn set_output(&mut self, w: Box<dyn Write>) {
        let _ = self.out.flush();
        self.out = std::io::BufWriter::new(w);
    }

    /// Runs the file at `path` in a fresh set of outside variables, after giving it the
    /// `preset` ones, and gives that set back.
    pub fn load_module(&mut self, path: &Path, preset: &[(&str, Value)]) -> R<Env> {
        let env: Env = Rc::new(RefCell::new(OrdMap::new()));
        {
            let mut e = env.borrow_mut();
            for (k, v) in preset {
                e.insert(Rc::from(*k), new_slot(v.clone()));
            }
        }
        let r = self.run_module(path, env.clone());
        // An error part way can leave loop scopes behind in the base frame.
        self.frames.truncate(1);
        let _ = self.out.flush();
        r.map(|_| env)
    }

    /// Calls a function value (your own or a built-in) with these inputs.
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>) -> R<Value> {
        let r = match f {
            Value::Func(func) => {
                let slots = args.into_iter().map(|v| new_slot(copy_val(v))).collect();
                self.enter_call(func.clone(), func.decl.clone(), slots)
            }
            Value::Builtin(name) => crate::builtins::call(self, name, args),
            o => err(format!("a {} can't be called like a function", o.type_name())),
        };
        self.frames.truncate(1);
        let _ = self.out.flush();
        r
    }

    /// A variable from a loaded file's outside variables.
    pub fn env_get(env: &Env, name: &str) -> Option<Value> {
        env.borrow().get(name).map(|s| s.borrow().clone())
    }

    /// Sets (or makes) a loaded file's outside variable. Constants are left alone.
    pub fn env_set(env: &Env, name: &str, v: Value) {
        let mut e = env.borrow_mut();
        if let Some(s) = e.get(name) {
            if !s.is_const() {
                *s.borrow_mut() = v;
            }
            return;
        }
        e.insert(Rc::from(name), new_slot(v));
    }

    /// The names and values of a loaded file's outside variables, in the order they were made.
    pub fn env_vars(env: &Env) -> Vec<(Rc<str>, Value)> {
        crate::interp::vars_of(&env.borrow())
    }
}

/// A plain-text report for an error, or None for END and CANCEL (which aren't errors).
pub fn fault_report(f: &Fault) -> Option<String> {
    match f {
        Fault::Error(e) | Fault::TaskError(e) => Some(report(e)),
        Fault::End | Fault::Cancelled => None,
    }
}

/// The file path and line of an error, for jumping to it in an editor.
pub fn fault_location(f: &Fault) -> Option<(String, usize)> {
    match f {
        Fault::Error(e) | Fault::TaskError(e) => Some((e.path.clone(), e.line)),
        _ => None,
    }
}

/// Number helpers for hosts, since .jlb numbers aren't plain floats.
pub fn num(v: f64) -> Value {
    Value::Num(Number::from_f64(if v.is_finite() { v } else { 0.0 }).unwrap_or_else(|_| Number::int(0)))
}

pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Num(n) => Some(n.to_f64()),
        _ => None,
    }
}

/// A table with named entries, in order.
pub fn named_table(entries: Vec<(&str, Value)>) -> Value {
    let mut t = Table::new();
    t.named.reserve(entries.len());
    for (k, v) in entries {
        t.named.insert(Rc::from(k), new_slot(v));
    }
    Value::new_table(t)
}

/// The same, with names already made (shared between tables).
pub fn named_table_rc(entries: Vec<(Rc<str>, Value)>) -> Value {
    let mut t = Table::new();
    t.named.reserve(entries.len());
    for (k, v) in entries {
        t.named.insert(k, new_slot(v));
    }
    Value::new_table(t)
}

/// A table of items, numbered from 1.
pub fn list_table(items: Vec<Value>) -> Value {
    Value::new_table(Table::from_values(items))
}

/// A named entry of a table value.
pub fn table_get(t: &Value, name: &str) -> Option<Value> {
    match t {
        Value::Table(rc, _) => rc.named.get(name).map(|s| s.borrow().clone()),
        _ => None,
    }
}

impl std::fmt::Debug for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Error(e) => write!(f, "Error({})", report(e)),
            Fault::TaskError(e) => write!(f, "TaskError({})", report(e)),
            Fault::End => write!(f, "End"),
            Fault::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", display(self))
    }
}
