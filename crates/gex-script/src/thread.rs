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

pub enum ToGame {
    Frame(FrameInput),
    Stop,
}

pub enum FromGame {
    Frame { list: RenderList, tree: SceneTree, logs: Vec<LogLine> },
    Stopped { logs: Vec<LogLine> },
}

pub struct GameHandle {
    tx: Sender<ToGame>,
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
        let (tx, game_rx) = channel::<ToGame>();
        let (game_tx, rx) = channel::<FromGame>();
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("game".into())
            // The interpreter walks the syntax tree recursively; give deep code room.
            .stack_size(256 * 1024 * 1024)
            .spawn(move || run(project, tree, c2, game_rx, game_tx))
            .expect("couldn't start the game thread");
        GameHandle { tx, rx, cancel, thread: Some(thread), waiting: false }
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
                    if matches!(m, FromGame::Frame { .. }) {
                        self.waiting = false;
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

fn run(project: Project, tree: SceneTree, cancel: Arc<AtomicBool>, rx: Receiver<ToGame>, tx: Sender<FromGame>) {
    let mut game = Game::new(project.clone(), tree, Some(cancel.clone()));
    while let Ok(msg) = rx.recv() {
        match msg {
            ToGame::Stop => break,
            ToGame::Frame(input) => {
                game.step(input);
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let next = game.st.borrow_mut().change_scene.take();
                if let Some(res) = next {
                    let mut logs = game.take_logs();
                    match project.read_text(&res).map_err(|e| e).and_then(|t| scene::load(&t, &|p: &str| project.read_text(p)).map_err(|e| e.0)) {
                        Ok(t) => game = Game::new(project.clone(), t, Some(cancel.clone())),
                        Err(e) => logs.push(LogLine { kind: LogKind::Error, text: format!("CHANGE_SCENE {}: {}", res, e), file: None, line: 0 }),
                    }
                    game.st.borrow_mut().logs.extend(logs);
                }
                let quit = game.st.borrow().quit;
                let list = game.render_list();
                let tree = game.tree().clone();
                let logs = game.take_logs();
                if tx.send(FromGame::Frame { list, tree, logs }).is_err() || quit {
                    break;
                }
            }
        }
    }
    let _ = tx.send(FromGame::Stopped { logs: game.take_logs() });
}
