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

// ---------- debugging from a host ----------

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Breakpoints a host can change at any time, from any thread.
#[derive(Default)]
pub struct Breakpoints {
    /// Quick check: are there any breakpoints, or is a step asked for?
    pub active: AtomicBool,
    pub lines: RwLock<HashMap<PathBuf, HashSet<usize>>>,
    /// Pause at the very next statement (stepping).
    pub step: AtomicBool,
    /// Breakpoints that only stop when an expression is true: (file, line) → expression.
    pub conditions: RwLock<HashMap<(PathBuf, usize), String>>,
    /// Expressions worked out and shown whenever code stops.
    pub watches: RwLock<Vec<String>>,
}

impl Breakpoints {
    pub fn set(&self, file: PathBuf, lines: HashSet<usize>) {
        let mut l = self.lines.write().unwrap();
        if lines.is_empty() {
            l.remove(&file);
        } else {
            l.insert(file, lines);
        }
        self.refresh(&l);
    }
    fn refresh(&self, l: &HashMap<PathBuf, HashSet<usize>>) {
        self.active.store(!l.is_empty() || self.step.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    pub fn set_step(&self, on: bool) {
        self.step.store(on, Ordering::Relaxed);
        let l = self.lines.read().unwrap();
        self.refresh(&l);
    }
    /// Sets (or, with an empty expression, removes) the condition on one breakpoint.
    pub fn set_condition(&self, file: PathBuf, line: usize, expr: &str) {
        let mut c = self.conditions.write().unwrap();
        if expr.trim().is_empty() {
            c.remove(&(file, line));
        } else {
            c.insert((file, line), expr.trim().to_string());
        }
    }
    pub fn set_watches(&self, w: Vec<String>) {
        *self.watches.write().unwrap() = w;
    }
}

/// Works out an expression where code is running now (its local and outside variables).
/// Breakpoints don't stop code it calls.
pub fn eval_here(it: &mut Interp, text: &str) -> Result<Value, String> {
    let toks = crate::lexer::lex(text).map_err(|e| e.msg)?;
    let prog = crate::parser::Parser::new(toks).program().map_err(|e| e.msg)?;
    let e = match prog.as_slice() {
        [s] => match &s.kind {
            crate::ast::StmtKind::Expr(e) => e,
            _ => return Err("not an expression".into()),
        },
        _ => return Err("one expression only".into()),
    };
    let saved = it.pause.take();
    let line = it.frames.last().map(|f| f.line);
    let depth = it.frames.len();
    let scopes = it.frames.last().map(|f| f.scopes.len());
    let r = it.eval(e);
    it.frames.truncate(depth);
    if let (Some(f), Some(l), Some(n)) = (it.frames.last_mut(), line, scopes) {
        f.line = l;
        f.scopes.truncate(n);
    }
    it.pause = saved;
    r.map_err(|f| match f {
        Fault::Error(e) | Fault::TaskError(e) => e.msg,
        Fault::End => "END".into(),
        Fault::Cancelled => "cancelled".into(),
    })
}

/// Where code stopped, for the host to show.
pub struct Paused {
    pub file: PathBuf,
    pub line: usize,
    /// Running functions, innermost first: (name, line).
    pub stack: Vec<(String, usize)>,
    /// The innermost function's local variables, then the file's outside variables.
    pub locals: Vec<(String, String)>,
    pub outside: Vec<(String, String)>,
    /// Each watch expression and its value (or the error working it out).
    pub watches: Vec<(String, String)>,
}

pub struct PauseHook {
    pub breakpoints: Arc<Breakpoints>,
    /// Called when code stops; blocks until the host says to go on. Returning false stops
    /// the running code (as if cancelled).
    pub on_pause: Box<dyn FnMut(Paused) -> bool>,
}

pub(crate) fn check_pause(it: &mut Interp, line: usize) -> R<()> {
    let hook = it.pause.as_ref().unwrap();
    if !hook.breakpoints.active.load(Ordering::Relaxed) {
        return Ok(());
    }
    let file = it.frames.last().map(|f| f.src.canon.clone()).unwrap_or_default();
    let stepping = hook.breakpoints.step.load(Ordering::Relaxed);
    let hit = stepping || hook.breakpoints.lines.read().unwrap().get(&file).is_some_and(|l| l.contains(&line));
    if !hit {
        return Ok(());
    }
    let mut cond_error = None;
    if !stepping {
        let cond = hook.breakpoints.conditions.read().unwrap().get(&(file.clone(), line)).cloned();
        if let Some(c) = cond {
            match eval_here(it, &c).and_then(|v| crate::ops::truth(&v).map_err(|_| "it isn't TRUE or FALSE".to_string())) {
                Ok(false) => return Ok(()),
                Ok(true) => {}
                Err(e) => cond_error = Some(format!("the condition `{}` didn't work: {}", c, e)),
            }
        }
    }
    let hook = it.pause.as_ref().unwrap();
    hook.breakpoints.set_step(false);
    let watch_list = hook.breakpoints.watches.read().unwrap().clone();
    let short = |v: &Value| {
        let mut s = crate::value::display(v);
        if s.chars().count() > 200 {
            s = s.chars().take(200).collect::<String>() + "…";
        }
        s
    };
    let frame = it.frames.last().unwrap();
    let mut locals = Vec::new();
    for scope in &frame.scopes {
        for (k, v) in crate::interp::vars_of(scope) {
            locals.push((k.to_string(), short(&v)));
        }
    }
    let outside = crate::interp::vars_of(&frame.env.borrow())
        .into_iter()
        .filter(|(_, v)| !v.is_func())
        .map(|(k, v)| (k.to_string(), short(&v)))
        .collect();
    let stack = it
        .frames
        .iter()
        .rev()
        .map(|f| (f.func.as_ref().and_then(|x| x.decl.name.as_ref().map(|n| n.to_string())).unwrap_or_else(|| "(file)".into()), f.line))
        .collect();
    let mut watches: Vec<(String, String)> = Vec::new();
    if let Some(e) = cond_error {
        watches.push(("⚠".into(), e));
    }
    for w in watch_list {
        let v = match eval_here(it, &w) {
            Ok(v) => short(&v),
            Err(e) => format!("⚠ {}", e),
        };
        watches.push((w, v));
    }
    let paused = Paused { file, line, stack, locals, outside, watches };
    let go_on = (it.pause.as_mut().unwrap().on_pause)(paused);
    if go_on {
        Ok(())
    } else {
        Err(Fault::Cancelled)
    }
}
