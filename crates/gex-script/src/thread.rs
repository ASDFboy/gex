//! Running a game on its own thread, so a slow or stuck script never freezes the editor.
//! The editor sends input each frame; the game sends back what to draw, a copy of its
//! tree (for the inspector) and log lines.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use gex_core::project::Project;
use gex_core::render::RenderList;
use gex_core::{blueprint, BlueprintTree};

use crate::world::{FrameInput, Game, LogKind, LogLine};
pub use jlb::host::Breakpoints;
use jlb::host::PauseHook;

pub enum ToGame {
    Frame(FrameInput),
    /// A script file changed: load it again in the running game.
    Reload(String),
    /// Go online before the scripts start (playing together from the editor).
    Net(NetStart),
    Stop,
}

/// How a game started from the editor goes online by itself.
#[derive(Clone, Debug)]
pub enum NetStart {
    Host(u16),
    Join(String, u16),
}

pub enum FromGame {
    /// `tree` (a copy for the editor's inspector) comes about ten times a second.
    /// `blueprint`: the blueprint the game is in (it changes with CHANGE_BLUEPRINT), with `tree`.
    Frame { list: RenderList, tree: Option<BlueprintTree>, blueprint: Option<String>, logs: Vec<LogLine>, stats: crate::world::FrameStats },
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
    /// Watch expressions and their values.
    pub watches: Vec<(String, String)>,
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
    /// `blueprint` is the file it came from (multiplayer games match players' blueprints by it).
    pub fn start(project: Project, tree: BlueprintTree, blueprint: &str) -> GameHandle {
        Self::start_with(project, tree, blueprint, Arc::new(Breakpoints::default()))
    }

    /// Starts a game that stops at these breakpoints (which can change while it runs).
    pub fn start_with(project: Project, tree: BlueprintTree, blueprint: &str, breakpoints: Arc<Breakpoints>) -> GameHandle {
        let blueprint = blueprint.to_string();
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
            .spawn(move || run(project, tree, blueprint, c2, game_rx, game_tx, bp, debug_rx))
            .expect("couldn't start the game thread");
        GameHandle { tx, debug, breakpoints, paused: None, rx, cancel, thread: Some(thread), waiting: false }
    }

    /// Hosts or joins a multiplayer game as it starts (see `NetStart`).
    pub fn go_online(&self, how: NetStart) {
        let _ = self.tx.send(ToGame::Net(how));
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
                watches: p.watches,
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
#[allow(clippy::too_many_arguments)]
fn run(project: Project, tree: BlueprintTree, blueprint: String, cancel: Arc<AtomicBool>, rx: Receiver<ToGame>, tx: Sender<FromGame>, bp: Arc<Breakpoints>, debug_rx: Receiver<Debug>) {
    let debug = Arc::new(std::sync::Mutex::new(debug_rx));
    let mut game = Game::new(project.clone(), tree, Some(cancel.clone()));
    game.st.borrow_mut().blueprint = blueprint;
    let mut frames: u64 = 0;
    game.set_pause_hook(pause_hook(bp.clone(), tx.clone(), debug.clone(), cancel.clone()));
    while let Ok(msg) = rx.recv() {
        match msg {
            ToGame::Stop => break,
            ToGame::Reload(res) => {
                game.reload_script(&res);
            }
            ToGame::Net(how) => {
                let mut st = game.st.borrow_mut();
                let r = match &how {
                    NetStart::Host(port) => crate::net::host(&mut st, *port),
                    NetStart::Join(addr, port) => crate::net::join(&mut st, addr, *port),
                };
                match r {
                    Ok(()) => {
                        if let Some(n) = st.net.as_mut() {
                            n.auto = true;
                        }
                    }
                    Err(e) => st.log(LogKind::Error, format!("playing together: {}", e)),
                }
            }
            ToGame::Frame(input) => {
                let started = std::time::Instant::now();
                game.step(input);
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let next = game.st.borrow_mut().change_blueprint.take();
                if let Some(res) = next {
                    let mut logs = game.take_logs();
                    // Settings scripts changed last for the whole run, across blueprints.
                    let (fade, title, graphics, display, physics, actions) = {
                        let st = game.st.borrow();
                        (st.fade.clone(), st.window_title.clone(), st.project.config.graphics.clone(), st.project.config.display.clone(), st.physics.settings.clone(), st.input.actions.clone())
                    };
                    // A multiplayer game carries on in the next blueprint.
                    let net = {
                        let mut st = game.st.borrow_mut();
                        let old = st.blueprint.clone();
                        st.net.take().map(|mut n| {
                            n.change_level(&old);
                            n
                        })
                    };
                    match project.read_text(&res).map_err(|e| e).and_then(|t| blueprint::load(&t, &|p: &str| project.read_text(p)).map_err(|e| e.0)) {
                        Ok(t) => {
                            let mut next = Game::new(project.clone(), t, Some(cancel.clone()));
                            next.carry_over(&mut game);
                            game = next;
                            game.set_pause_hook(pause_hook(bp.clone(), tx.clone(), debug.clone(), cancel.clone()));
                            let mut st = game.st.borrow_mut();
                            st.window_title = title;
                            st.project.config.graphics = graphics;
                            st.project.config.display = display;
                            st.physics.settings = physics;
                            st.input.actions = actions;
                            st.blueprint = res.clone();
                            st.net = net;
                            // The fade comes back in from its color.
                            st.fade = fade.map(|f| crate::world::Fade { to_blueprint: None, time: 0.0, ..f });
                        }
                        Err(e) => logs.push(LogLine { kind: LogKind::Error, text: format!("CHANGE_BLUEPRINT {}: {}", res, e), file: None, line: 0 }),
                    }
                    game.st.borrow_mut().logs.extend(logs);
                }
                let quit = game.st.borrow().quit;
                let list = game.render_list();
                let mut stats = std::mem::take(&mut game.stats);
                stats.total_ms = started.elapsed().as_secs_f32() * 1000.0;
                stats.objects = game.tree().objects.len();
                stats.sprites = list.sprites.len();
                stats.meshes = list.meshes.len();
                frames += 1;
                let tree = (frames % 6 == 1).then(|| game.tree().clone());
                let blueprint = tree.as_ref().map(|_| game.st.borrow().blueprint.clone());
                let logs = game.take_logs();
                if tx.send(FromGame::Frame { list, tree, blueprint, logs, stats }).is_err() || quit {
                    break;
                }
            }
        }
    }
    let _ = tx.send(FromGame::Stopped { logs: game.take_logs() });
}
