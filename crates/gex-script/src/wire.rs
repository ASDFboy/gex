//! Messages over TCP, for multiplayer games and for editing a project together. Each message
//! is a frame: its length (4 bytes, little-endian), a kind byte (0 = JSON, 1 = raw bytes) and
//! the body. Sockets are non-blocking and polled once a frame, so nothing waits and no thread
//! sits idle; only connecting (which can take seconds) runs on a short-lived thread.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

/// The biggest frame accepted (a whole asset file goes in one).
pub const MAX_FRAME: usize = 512 << 20;
/// Unsent bytes allowed to pile up for a game's peer before it's dropped as too slow.
pub const MAX_BACKLOG: usize = 64 << 20;
/// The same for the editor's links (whole files go through them).
pub const MAX_LINK_BACKLOG: usize = 1 << 30;

pub enum Frame {
    Json(Vec<u8>),
    Bytes(Vec<u8>),
}

pub struct Conn {
    stream: TcpStream,
    inbuf: Vec<u8>,
    in_pos: usize,
    out: Vec<u8>,
    out_pos: usize,
    pub closed: bool,
}

impl Conn {
    pub fn new(stream: TcpStream) -> std::io::Result<Conn> {
        stream.set_nonblocking(true)?;
        let _ = stream.set_nodelay(true);
        Ok(Conn { stream, inbuf: Vec::new(), in_pos: 0, out: Vec::new(), out_pos: 0, closed: false })
    }

    pub fn peer(&self) -> String {
        self.stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default()
    }

    fn frame(&mut self, kind: u8, body: &[u8]) {
        if self.closed {
            return;
        }
        if self.out.len() - self.out_pos + body.len() > MAX_BACKLOG {
            self.closed = true;
            return;
        }
        self.out.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        self.out.push(kind);
        self.out.extend_from_slice(body);
    }

    /// Queues a message (sent by `flush`).
    pub fn send<T: serde::Serialize>(&mut self, msg: &T) {
        if let Ok(b) = serde_json::to_vec(msg) {
            self.frame(0, &b);
        }
    }

    /// Queues an already encoded JSON message (one received, to pass on).
    pub fn send_json(&mut self, body: &[u8]) {
        self.frame(0, body);
    }

    pub fn send_bytes(&mut self, body: &[u8]) {
        self.frame(1, body);
    }

    /// Bytes still waiting to go out.
    pub fn backlog(&self) -> usize {
        self.out.len() - self.out_pos
    }

    /// Sends what the socket takes now; the rest waits for the next call.
    pub fn flush(&mut self) {
        while !self.closed && self.out_pos < self.out.len() {
            match self.stream.write(&self.out[self.out_pos..]) {
                Ok(0) => self.closed = true,
                Ok(n) => self.out_pos += n,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
        if self.out_pos == self.out.len() {
            self.out.clear();
            self.out_pos = 0;
        } else if self.out_pos > (1 << 20) && self.out_pos * 2 > self.out.len() {
            self.out.drain(..self.out_pos);
            self.out_pos = 0;
        }
    }

    /// Every whole frame that has arrived.
    pub fn recv(&mut self) -> Vec<Frame> {
        let mut buf = [0u8; 64 * 1024];
        // Read what's there, but not forever: a flood can wait for the next frame.
        for _ in 0..256 {
            if self.closed {
                break;
            }
            match self.stream.read(&mut buf) {
                Ok(0) => self.closed = true,
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.closed = true,
            }
        }
        let mut out = Vec::new();
        loop {
            let rest = &self.inbuf[self.in_pos..];
            if rest.len() < 4 {
                break;
            }
            let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
            if len == 0 || len > MAX_FRAME {
                self.closed = true;
                break;
            }
            if rest.len() < 4 + len {
                break;
            }
            let kind = rest[4];
            let body = rest[5..4 + len].to_vec();
            self.in_pos += 4 + len;
            out.push(if kind == 1 { Frame::Bytes(body) } else { Frame::Json(body) });
        }
        if self.in_pos == self.inbuf.len() {
            self.inbuf.clear();
            self.in_pos = 0;
        } else if self.in_pos > (1 << 20) {
            self.inbuf.drain(..self.in_pos);
            self.in_pos = 0;
        }
        out
    }
}

/// Connects in the background (it can take a few seconds); poll the receiver each frame.
pub fn connect(address: &str, port: u16) -> Receiver<Result<TcpStream, String>> {
    let (tx, rx) = channel();
    let address = address.trim().to_string();
    std::thread::spawn(move || {
        let r = (|| {
            let addrs: Vec<_> = (address.as_str(), port).to_socket_addrs().map_err(|e| format!("can't find {}: {}", address, e))?.collect();
            let mut last = format!("can't find {}", address);
            for a in addrs {
                match TcpStream::connect_timeout(&a, Duration::from_secs(6)) {
                    Ok(s) => return Ok(s),
                    Err(e) => last = format!("can't reach {}:{} ({})", address, port, e),
                }
            }
            Err(last)
        })();
        let _ = tx.send(r);
    });
    rx
}

/// Splits "host:port" (or just "host", using `default_port`).
pub fn split_address(s: &str, default_port: u16) -> (String, u16) {
    let s = s.trim();
    if let Some((h, p)) = s.rsplit_once(':') {
        if !h.contains(':') || h.starts_with('[') {
            if let Ok(p) = p.parse() {
                return (h.trim_matches(|c| c == '[' || c == ']').to_string(), p);
            }
        }
    }
    (if s.is_empty() { "localhost".to_string() } else { s.to_string() }, default_port)
}

/// This computer's best address for others to join: a ZeroTier (or Tailscale, Hamachi)
/// address when there is one, since that's what friends elsewhere use, otherwise the local
/// network one.
pub fn local_ip() -> Option<String> {
    addresses().into_iter().next().map(|(ip, _)| ip)
}

/// Every address others could join this computer at, with what network it's on, best first:
/// ZeroTier, Tailscale, Hamachi, the local network, then anything else.
pub fn addresses() -> Vec<(String, &'static str)> {
    let mut out: Vec<(u8, String, &'static str)> = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let std::net::IpAddr::V4(ip) = i.ip() else { continue };
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        let name = i.name.to_lowercase();
        let o = ip.octets();
        let (rank, label) = if name.contains("zerotier") || name.starts_with("zt") {
            (0, "ZeroTier")
        } else if name.contains("tailscale") || (o[0] == 100 && (64..128).contains(&o[1])) {
            (1, "Tailscale")
        } else if name.contains("hamachi") || o[0] == 25 {
            (2, "Hamachi")
        } else if name.contains("vethernet") || name.contains("docker") || name.contains("virtualbox") || name.contains("vmware") || name.starts_with("br-") || name.starts_with("veth") {
            // Virtual machines' and containers' own networks: no friend is on those.
            continue;
        } else if ip.is_private() {
            (3, "LAN")
        } else {
            (4, "Internet")
        };
        out.push((rank, ip.to_string(), label));
    }
    out.sort();
    out.dedup_by(|a, b| a.1 == b.1);
    out.into_iter().map(|(_, ip, l)| (ip, l)).collect()
}

/// Called when something arrives (to wake a window that only redraws when there's news).
pub type Waker = std::sync::Arc<dyn Fn() + Send + Sync>;

/// A connection read and written by two threads of its own that sleep until there's something
/// to do, so an idle connection costs nothing: for the editor, which doesn't run every frame.
/// Same use as `Conn`.
pub struct Link {
    tx: std::sync::mpsc::Sender<Vec<u8>>,
    rx: Receiver<Frame>,
    queued: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// The reader is done: nothing more can arrive.
    dead: std::sync::Arc<AtomicBool>,
    /// Closed from this side.
    closed: bool,
    stream: TcpStream,
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

impl Link {
    pub fn new(stream: TcpStream, wake: Waker) -> std::io::Result<Link> {
        stream.set_nonblocking(false)?;
        let _ = stream.set_nodelay(true);
        let (tx, wrx) = channel::<Vec<u8>>();
        let (ftx, rx) = channel::<Frame>();
        let queued = std::sync::Arc::new(AtomicUsize::new(0));
        let dead = std::sync::Arc::new(AtomicBool::new(false));
        // Writer: sends what's queued, in order.
        {
            let mut s = stream.try_clone()?;
            let (queued, dead) = (queued.clone(), dead.clone());
            std::thread::Builder::new().name("link-out".into()).spawn(move || {
                let mut broken = false;
                while let Ok(b) = wrx.recv() {
                    let n = b.len();
                    queued.fetch_sub(n, Ordering::Relaxed);
                    // A failed write ends sending; the reader notices the connection is gone
                    // after taking in whatever had arrived (a last message isn't lost).
                    if !broken && s.write_all(&b).is_err() {
                        broken = true;
                    }
                }
                if !broken {
                    let _ = s.shutdown(std::net::Shutdown::Write);
                }
                // Give the other end a moment to take it all in and close; then close fully, so
                // the reader stops waiting even if the other end never does.
                for _ in 0..40 {
                    if dead.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = s.shutdown(std::net::Shutdown::Both);
            })?;
        }
        // Reader: whole frames, then a wake-up.
        {
            let mut s = stream.try_clone()?;
            let dead = dead.clone();
            std::thread::Builder::new().name("link-in".into()).spawn(move || {
                let mut head = [0u8; 4];
                loop {
                    if s.read_exact(&mut head).is_err() {
                        break;
                    }
                    let len = u32::from_le_bytes(head) as usize;
                    if len == 0 || len > MAX_FRAME {
                        break;
                    }
                    let mut body = vec![0u8; len];
                    if s.read_exact(&mut body).is_err() {
                        break;
                    }
                    let kind = body.remove(0);
                    let f = if kind == 1 { Frame::Bytes(body) } else { Frame::Json(body) };
                    if ftx.send(f).is_err() {
                        break;
                    }
                    wake();
                }
                dead.store(true, Ordering::Relaxed);
                wake();
            })?;
        }
        Ok(Link { tx, rx, queued, dead, closed: false, stream })
    }

    fn frame(&mut self, kind: u8, body: &[u8]) {
        if self.is_closed() {
            return;
        }
        if self.backlog() + body.len() > MAX_LINK_BACKLOG {
            self.close();
            return;
        }
        let mut b = Vec::with_capacity(body.len() + 5);
        b.extend_from_slice(&((body.len() + 1) as u32).to_le_bytes());
        b.push(kind);
        b.extend_from_slice(body);
        self.queued.fetch_add(b.len(), Ordering::Relaxed);
        if self.tx.send(b).is_err() {
            self.close();
        }
    }

    pub fn send<T: serde::Serialize>(&mut self, msg: &T) {
        if let Ok(b) = serde_json::to_vec(msg) {
            self.frame(0, &b);
        }
    }

    pub fn send_json(&mut self, body: &[u8]) {
        self.frame(0, body);
    }

    pub fn send_bytes(&mut self, body: &[u8]) {
        self.frame(1, body);
    }

    /// Bytes still waiting to go out.
    pub fn backlog(&self) -> usize {
        self.queued.load(Ordering::Relaxed)
    }

    /// Nothing to do: the writer thread sends as soon as something is queued.
    pub fn flush(&mut self) {}

    /// The other end's address.
    pub fn peer_ip(&self) -> Option<std::net::IpAddr> {
        self.stream.peer_addr().ok().map(|a| a.ip())
    }

    /// Every frame that has arrived.
    pub fn recv(&mut self) -> Vec<Frame> {
        self.rx.try_iter().collect()
    }

    pub fn is_closed(&self) -> bool {
        self.closed || self.dead.load(Ordering::Relaxed)
    }

    /// Ends the connection once what's queued has gone.
    pub fn close_after_sending(&mut self) {
        let (tx, _) = channel();
        // Dropping the sender lets the writer finish the queue, then shut the socket.
        drop(std::mem::replace(&mut self.tx, tx));
        self.closed = true;
    }

    pub fn close(&mut self) {
        self.closed = true;
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        // The writer sends what's left, then closes; the reader ends when the socket does.
        let (tx, _) = channel();
        drop(std::mem::replace(&mut self.tx, tx));
    }
}

/// Accepts connections on a thread of its own (waking the window for each). Dropping it stops
/// listening and frees the port.
pub struct Listener {
    pub rx: Receiver<TcpStream>,
    port: u16,
    stop: std::sync::Arc<AtomicBool>,
}

impl Listener {
    pub fn new(l: std::net::TcpListener, wake: Waker) -> std::io::Result<Listener> {
        let (tx, rx) = channel();
        l.set_nonblocking(false)?;
        let port = l.local_addr()?.port();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::Builder::new().name("listen".into()).spawn(move || {
            for s in l.incoming() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                match s {
                    Ok(s) => {
                        if tx.send(s).is_err() {
                            break;
                        }
                        wake();
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        })?;
        Ok(Listener { rx, port, stop })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // Wake the waiting accept with a connection of our own, so the thread ends.
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], self.port)), Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn addresses_are_real_and_labelled() {
        for (ip, label) in super::addresses() {
            let ip: std::net::Ipv4Addr = ip.parse().unwrap();
            assert!(!ip.is_loopback() && !ip.is_link_local(), "{}", ip);
            assert!(["ZeroTier", "Tailscale", "Hamachi", "LAN", "Internet"].contains(&label));
        }
        assert_eq!(super::split_address("10.147.17.5:7778", 1), ("10.147.17.5".to_string(), 7778));
        assert_eq!(super::split_address("10.147.17.5", 7778), ("10.147.17.5".to_string(), 7778));
    }
}
