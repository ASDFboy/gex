//! Running a game on its own thread, so a slow or stuck script never freezes the editor.
//! The editor sends input each frame; the game sends back what to draw, a copy of its
//! tree (for the inspector) and log lines.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use gex_core::project::Project;
use gex_core::render::RenderList;
use gex_core::{scene, SceneTree};

use crate::world::{FrameInput, Game, LogKind, LogLine};
pub use jlb::host::Breakpoints;
use jlb::host::PauseHook;

pub enum ToGame {
    Frame(FrameInput),
    /// A script file changed: load it again in the running game.
    Reload(String),
    Stop,
}

pub enum FromGame {
    /// `tree` (a copy for the editor's inspector) comes about ten times a second.
    Frame { list: RenderList, tree: Option<SceneTree>, logs: Vec<LogLine> },
    /// A breakpoint (or a step) stopped the game; it waits for `resume` / `step`.
    Paused(Paused),
    Stopped { logs: Vec<LogLine> },
}

/// Where the game is paused, for the editor's debugger.
#[derive(Clone, Debug)]
pub struct Paused {
    pub file: String,
    pub line: usize,
    pub stack: Vec<(String, usize)>,
    pub locals: Vec<(String, String)>,
    pub outside: Vec<(String, String)>,
}

pub enum Debug {
    Continue,
    Step,
    Stop,
}

pub struct GameHandle {
    tx: Sender<ToGame>,
    debug: Sender<Debug>,
    pub breakpoints: Arc<Breakpoints>,
    /// The game is stopped at a breakpoint.
    pub paused: Option<Paused>,
    pub rx: Receiver<FromGame>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// A frame request is out and not answered yet.
    pub waiting: bool,
}

impl GameHandle {
    /// Starts a game over a copy of `tree` (ids stay the same as the editor's, so the
    /// selection carries over into play mode).
    pub fn start(project: Project, tree: SceneTree) -> GameHandle {
        Self::start_with(project, tree, Arc::new(Breakpoints::default()))
    }

    /// Starts a game that stops at these breakpoints (which can change while it runs).
    pub fn start_with(project: Project, tree: SceneTree, breakpoints: Arc<Breakpoints>) -> GameHandle {
        let (tx, game_rx) = channel::<ToGame>();
        let (game_tx, rx) = channel::<FromGame>();
        let (debug, debug_rx) = channel::<Debug>();
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let bp = breakpoints.clone();
        let thread = std::thread::Builder::new()
            .name("game".into())
            // The interpreter walks the syntax tree recursively; give deep code room.
            .stack_size(256 * 1024 * 1024)
            .spawn(move || run(project, tree, c2, game_rx, game_tx, bp, debug_rx))
            .expect("couldn't start the game thread");
        GameHandle { tx, debug, breakpoints, paused: None, rx, cancel, thread: Some(thread), waiting: false }
    }

    pub fn reload(&self, res: &str) {
        let _ = self.tx.send(ToGame::Reload(res.to_string()));
    }

    /// Goes on from a breakpoint.
    pub fn resume(&mut self) {
        if self.paused.take().is_some() {
            let _ = self.debug.send(Debug::Continue);
        }
    }

    /// Runs one statement, then stops again.
    pub fn step(&mut self) {
        if self.paused.take().is_some() {
            let _ = self.debug.send(Debug::Step);
        }
    }

    /// Asks for the next frame, unless the last one isn't done yet.
    pub fn request_frame(&mut self, input: FrameInput) {
        if !self.waiting && self.tx.send(ToGame::Frame(input)).is_ok() {
            self.waiting = true;
        }
    }

    /// The newest message, if one arrived.
    pub fn poll(&mut self) -> Vec<FromGame> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(m) => {
                    match &m {
                        FromGame::Frame { .. } => self.waiting = false,
                        FromGame::Paused(p) => self.paused = Some(p.clone()),
                        FromGame::Stopped { .. } => self.paused = None,
                    }
                    out.push(m);
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// Stops the game, interrupting a script that's still running.
    pub fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.debug.send(Debug::Stop);
        let _ = self.tx.send(ToGame::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for GameHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn pause_hook(bp: Arc<Breakpoints>, tx: Sender<FromGame>, debug: Arc<std::sync::Mutex<Receiver<Debug>>>, cancel: Arc<AtomicBool>) -> PauseHook {
    let b2 = bp.clone();
    PauseHook {
        breakpoints: bp,
        on_pause: Box::new(move |p| {
            let _ = tx.send(FromGame::Paused(Paused {
                file: p.file.to_string_lossy().into_owned(),
                line: p.line,
                stack: p.stack,
                locals: p.locals,
                outside: p.outside,
            }));
            match debug.lock().unwrap().recv() {
                Ok(Debug::Continue) => true,
                Ok(Debug::Step) => {
                    b2.set_step(true);
                    true
                }
                _ => {
                    cancel.store(true, Ordering::Relaxed);
                    false
                }
            }
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn run(project: Project, tree: SceneTree, cancel: Arc<AtomicBool>, rx: Receiver<ToGame>, tx: Sender<FromGame>, bp: Arc<Breakpoints>, debug_rx: Receiver<Debug>) {
    let debug = Arc::new(std::sync::Mutex::new(debug_rx));
    let mut game = Game::new(project.clone(), tree, Some(cancel.clone()));
    let mut frames: u64 = 0;
    game.set_pause_hook(pause_hook(bp.clone(), tx.clone(), debug.clone(), cancel.clone()));
    while let Ok(msg) = rx.recv() {
        match msg {
            ToGame::Stop => break,
            ToGame::Reload(res) => {
                game.reload_script(&res);
            }
            ToGame::Frame(input) => {
                game.step(input);
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let next = game.st.borrow_mut().change_scene.take();
                if let Some(res) = next {
                    let mut logs = game.take_logs();
                    match project.read_text(&res).map_err(|e| e).and_then(|t| scene::load(&t, &|p: &str| project.read_text(p)).map_err(|e| e.0)) {
                        Ok(t) => {
                            game = Game::new(project.clone(), t, Some(cancel.clone()));
                            game.set_pause_hook(pause_hook(bp.clone(), tx.clone(), debug.clone(), cancel.clone()));
                        }
                        Err(e) => logs.push(LogLine { kind: LogKind::Error, text: format!("CHANGE_SCENE {}: {}", res, e), file: None, line: 0 }),
                    }
                    game.st.borrow_mut().logs.extend(logs);
                }
                let quit = game.st.borrow().quit;
                let list = game.render_list();
                frames += 1;
                let tree = (frames % 6 == 1).then(|| game.tree().clone());
                let logs = game.take_logs();
                if tx.send(FromGame::Frame { list, tree, logs }).is_err() || quit {
                    break;
                }
            }
        }
    }
    let _ = tx.send(FromGame::Stopped { logs: game.take_logs() });
}
