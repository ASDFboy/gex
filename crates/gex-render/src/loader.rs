//! Pictures decoded on other threads, so a big texture showing for the first time doesn't
//! freeze the frame. The renderer asks for what it needs now (first in line) and what a game
//! will need soon (PRELOAD, after); finished pictures are uploaded at the start of a frame.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

/// Where a job's bytes come from: a file the worker reads itself, or bytes read already.
pub enum Src {
    File(PathBuf),
    Bytes(Vec<u8>),
}

pub struct Job {
    pub key: (String, bool),
    pub path: String,
    pub src: Src,
    /// Make mipmaps too (smooth textures).
    pub mips: bool,
    /// The renderer's texture generation when asked (reloads start a new one).
    pub generation: u64,
}

/// A decoded picture: its size and its mip levels (largest first).
pub struct Decoded {
    pub w: u32,
    pub h: u32,
    pub levels: Vec<Vec<u8>>,
}

pub struct Done {
    pub key: (String, bool),
    pub path: String,
    pub generation: u64,
    pub result: Result<Decoded, String>,
}

struct Queue {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
}

pub struct Loader {
    queue: Arc<Queue>,
    pub done: Receiver<Done>,
}

impl Loader {
    pub fn new() -> Loader {
        let queue = Arc::new(Queue { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new() });
        let (tx, done) = channel::<Done>();
        // Leave a core for the game and one for drawing.
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(2).clamp(1, 6);
        for i in 0..n {
            let (q, tx): (Arc<Queue>, Sender<Done>) = (queue.clone(), tx.clone());
            let _ = std::thread::Builder::new().name(format!("textures {}", i)).spawn(move || work(q, tx));
        }
        Loader { queue, done }
    }

    /// Asks for a picture: `urgent` ones (needed for this frame) go first in line.
    pub fn ask(&self, job: Job, urgent: bool) {
        let mut q = self.queue.jobs.lock().unwrap();
        if urgent {
            q.push_front(job);
        } else {
            q.push_back(job);
        }
        self.queue.ready.notify_one();
    }

    /// Moves a picture asked for earlier (as a preload) to the front of the line.
    pub fn hurry(&self, key: &(String, bool)) {
        let mut q = self.queue.jobs.lock().unwrap();
        if let Some(i) = q.iter().position(|j| &j.key == key) {
            if i > 0 {
                let j = q.remove(i).unwrap();
                q.push_front(j);
            }
        }
    }

    /// Forgets pictures not started yet (a reload).
    pub fn clear(&self) {
        self.queue.jobs.lock().unwrap().clear();
    }
}

fn work(q: Arc<Queue>, tx: Sender<Done>) {
    loop {
        let job = {
            let mut jobs = q.jobs.lock().unwrap();
            loop {
                if let Some(j) = jobs.pop_front() {
                    break j;
                }
                jobs = q.ready.wait(jobs).unwrap();
            }
        };
        let result = (|| -> Result<Decoded, String> {
            let bytes = match job.src {
                Src::File(p) => std::fs::read(&p).map_err(|e| format!("{}: {}", job.path, e))?,
                Src::Bytes(b) => b,
            };
            let img = gex_assets::decode_image(&job.path, &bytes)?;
            Ok(decoded(img.width, img.height, img.pixels, job.mips))
        })();
        if tx.send(Done { key: job.key, path: job.path, generation: job.generation, result }).is_err() {
            return;
        }
    }
}

/// A picture with its mip levels (each half the size of the last, down to 1 pixel).
pub fn decoded(w: u32, h: u32, rgba: Vec<u8>, mips: bool) -> Decoded {
    let mut levels = vec![rgba];
    if mips {
        let (mut cw, mut ch) = (w, h);
        while cw > 1 || ch > 1 {
            let (nw, nh, nd) = half(cw, ch, levels.last().unwrap());
            levels.push(nd);
            cw = nw;
            ch = nh;
        }
    }
    Decoded { w, h, levels }
}

/// Halves an RGBA image (box filter).
pub fn half(w: u32, h: u32, px: &[u8]) -> (u32, u32, Vec<u8>) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for y in 0..nh as usize {
        let (y0, y1) = ((y * 2).min(h - 1), (y * 2 + 1).min(h - 1));
        let (r0, r1) = (&px[y0 * w * 4..(y0 + 1) * w * 4], &px[y1 * w * 4..(y1 + 1) * w * 4]);
        let o = &mut out[y * nw as usize * 4..(y + 1) * nw as usize * 4];
        for x in 0..nw as usize {
            let (x0, x1) = ((x * 2).min(w - 1) * 4, (x * 2 + 1).min(w - 1) * 4);
            for c in 0..4 {
                o[x * 4 + c] = ((r0[x0 + c] as u32 + r0[x1 + c] as u32 + r1[x0 + c] as u32 + r1[x1 + c] as u32) / 4) as u8;
            }
        }
    }
    (nw, nh, out)
}
