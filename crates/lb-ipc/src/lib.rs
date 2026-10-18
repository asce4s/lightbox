//! Local single-instance protocol (SPEC-VIEWER §V2.3).
//!
//! One Lightbox GUI process owns an endpoint derived from the canonical data directory.
//! A second launch connects, sends one newline-delimited JSON message, and exits.
#![warn(missing_docs)]

use interprocess::ConnectWaitMode;
use interprocess::local_socket::traits::Listener as SocketListen;
use interprocess::local_socket::traits::Stream as SocketStream;
use interprocess::local_socket::{
    ConnectOptions, GenericFilePath, Listener, ListenerNonblockingMode, ListenerOptions, Name,
    ToFsName,
};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Protocol version written into every message.
pub const PROTOCOL_VERSION: u32 = 1;

/// Messages larger than this are rejected.
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// How long a client waits for the acknowledgement.
const ACK_TIMEOUT: Duration = Duration::from_secs(1);

/// A launch forwarded to the running instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Open the viewer on these paths. Relative paths are resolved against `cwd`.
    OpenViewer {
        /// File paths as the sender wrote them.
        paths: Vec<String>,
        /// Working directory of the sending process.
        cwd: String,
    },
    /// Raise the library window.
    ShowLibrary,
}

impl Request {
    /// `open_viewer` message.
    #[must_use]
    pub fn open_viewer(paths: Vec<String>, cwd: String) -> Self {
        Self::OpenViewer { paths, cwd }
    }

    /// `show_library` message.
    #[must_use]
    pub fn show_library() -> Self {
        Self::ShowLibrary
    }
}

/// Acknowledgement from the running instance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Response {
    /// Protocol version.
    pub v: u32,
    /// Whether the running instance accepted the launch.
    pub ok: bool,
    /// Why `ok` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Failure talking to the instance endpoint.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// The socket or pipe could not be used.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// Another process already owns the endpoint.
    #[error("the instance endpoint is already taken")]
    AddrInUse,
    /// The peer did not answer in time.
    #[error("timed out waiting for the instance")]
    Timeout,
    /// The bytes were not a protocol message.
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// Crate marker. The wire format uses [`PROTOCOL_VERSION`].
#[must_use]
pub fn protocol_version() -> u32 {
    PROTOCOL_VERSION
}

/// `lightbox-{first 16 hex of blake3(canonical data dir)}`.
#[must_use]
pub fn endpoint_id(data_dir: &Path) -> String {
    let canonical = std::fs::canonicalize(data_dir).unwrap_or_else(|_| absolute_fallback(data_dir));
    let hash = blake3::hash(canonical.as_os_str().as_encoded_bytes());
    let hex = hash.to_hex();
    let short = hex.get(..16).map_or(hex.as_str(), |s| s);
    format!("lightbox-{short}")
}

/// Filesystem path (Unix) or named-pipe path (Windows) for `data_dir`.
#[must_use]
pub fn socket_path(data_dir: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\{}", endpoint_id(data_dir)))
    }
    #[cfg(not(windows))]
    {
        runtime_dir().join(endpoint_id(data_dir))
    }
}

/// The process that owns the endpoint. Dropping it stops the accept thread.
pub struct Server {
    stop: Arc<AtomicBool>,
    incoming: Receiver<Request>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Binds the endpoint for `data_dir`, removing a stale socket whose connect is refused.
    ///
    /// Fails with [`IpcError::AddrInUse`] when another live server already owns the name.
    pub fn bind(data_dir: &Path) -> Result<Self, IpcError> {
        let path = socket_path(data_dir);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        match listen(&path) {
            Ok(listener) => spawn(listener),
            Err(error) if is_in_use(&error) => recover_stale(&path),
            Err(error) => Err(error.into()),
        }
    }

    /// The next launch the accept thread has queued, if any.
    pub fn try_recv(&self) -> Option<Request> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Sends `request` to the server for `data_dir` and waits for the acknowledgement.
pub fn send(data_dir: &Path, request: &Request) -> Result<Response, IpcError> {
    let line = encode_request(request);
    send_line(data_dir, line.as_bytes())
}

/// Sends one JSON line (a trailing newline is added when missing).
///
/// Used to probe version and command rejection. A live server answers `ok: false`
/// for an unknown `v` or `cmd` and stays up.
pub fn send_line(data_dir: &Path, line: &[u8]) -> Result<Response, IpcError> {
    let path = socket_path(data_dir);
    let mut stream = connect_path(&path)?;
    stream.write_all(line)?;
    if !line.ends_with(b"\n") {
        stream.write_all(b"\n")?;
    }
    stream.flush()?;
    let _ = SocketStream::set_nonblocking(&stream, true);
    let bytes = read_reply(&mut stream)?;
    serde_json::from_slice(&bytes).map_err(|error| IpcError::Protocol(error.to_string()))
}

fn encode_request(request: &Request) -> String {
    match request {
        Request::ShowLibrary => r#"{"v":1,"cmd":"show_library"}"#.to_owned(),
        Request::OpenViewer { paths, cwd } => serde_json::json!({
            "v": PROTOCOL_VERSION,
            "cmd": "open_viewer",
            "paths": paths,
            "cwd": cwd,
        })
        .to_string(),
    }
}

#[derive(Deserialize)]
struct RawRequest {
    v: u32,
    cmd: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    cwd: String,
}

fn parse_request(bytes: &[u8]) -> Result<Request, String> {
    let raw: RawRequest =
        serde_json::from_slice(bytes).map_err(|error| format!("invalid message: {error}"))?;
    if raw.v != PROTOCOL_VERSION {
        return Err(format!("unknown version {}", raw.v));
    }
    match raw.cmd.as_str() {
        "open_viewer" => Ok(Request::OpenViewer {
            paths: raw.paths,
            cwd: raw.cwd,
        }),
        "show_library" => Ok(Request::ShowLibrary),
        other => Err(format!("unknown command {other}")),
    }
}

fn ok_response() -> Response {
    Response {
        v: PROTOCOL_VERSION,
        ok: true,
        error: None,
    }
}

fn reject_response(error: impl Into<String>) -> Response {
    Response {
        v: PROTOCOL_VERSION,
        ok: false,
        error: Some(error.into()),
    }
}

fn spawn(listener: Listener) -> Result<Server, IpcError> {
    SocketListen::set_nonblocking(&listener, ListenerNonblockingMode::Accept)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let stop_thread = Arc::clone(&stop);
    let thread = thread::Builder::new()
        .name("lb-ipc".to_owned())
        .spawn(move || serve(listener, tx, stop_thread))
        .map_err(IpcError::Io)?;
    Ok(Server {
        stop,
        incoming: rx,
        thread: Some(thread),
    })
}

fn serve(listener: Listener, tx: Sender<Request>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match SocketListen::accept(&listener) {
            Ok(mut stream) => handle_client(&mut stream, &tx),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
}

fn handle_client(stream: &mut interprocess::local_socket::Stream, tx: &Sender<Request>) {
    let response = match read_request(stream) {
        Ok(bytes) => match parse_request(&bytes) {
            Ok(request) => {
                let _ = tx.send(request);
                ok_response()
            }
            Err(error) => reject_response(error),
        },
        Err(IpcError::Protocol(error)) => reject_response(error),
        Err(_) => reject_response("could not read message"),
    };
    let _ = write_response(stream, &response);
}

fn read_request(stream: &mut impl Read) -> Result<Vec<u8>, IpcError> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let mut too_big = false;
    loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        let chunk = &tmp[..n];
        if let Some(pos) = chunk.iter().position(|byte| *byte == b'\n') {
            if buf.len().saturating_add(pos) > MAX_MESSAGE_BYTES {
                too_big = true;
            } else if !too_big {
                buf.extend_from_slice(&chunk[..pos]);
            }
            break;
        }
        if buf.len().saturating_add(n) > MAX_MESSAGE_BYTES {
            too_big = true;
        } else if !too_big {
            buf.extend_from_slice(chunk);
        }
    }
    if too_big {
        Err(IpcError::Protocol("message exceeds 1 MiB".to_owned()))
    } else {
        Ok(buf)
    }
}

fn read_reply(stream: &mut interprocess::local_socket::Stream) -> Result<Vec<u8>, IpcError> {
    let deadline = Instant::now() + ACK_TIMEOUT;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        if buf.len() > MAX_MESSAGE_BYTES {
            return Err(IpcError::Protocol("message exceeds 1 MiB".to_owned()));
        }
        if buf.contains(&b'\n') {
            break;
        }
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                if Instant::now() >= deadline {
                    return Err(IpcError::Timeout);
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(pos) = buf.iter().position(|byte| *byte == b'\n') {
        buf.truncate(pos);
    }
    Ok(buf)
}

fn write_response(stream: &mut impl Write, response: &Response) -> Result<(), IpcError> {
    let mut line =
        serde_json::to_vec(response).map_err(|error| IpcError::Protocol(error.to_string()))?;
    line.push(b'\n');
    stream.write_all(&line)?;
    stream.flush()?;
    Ok(())
}

fn recover_stale(path: &Path) -> Result<Server, IpcError> {
    if endpoint_is_live(path) {
        return Err(IpcError::AddrInUse);
    }
    let _ = std::fs::remove_file(path);
    match listen(path) {
        Ok(listener) => spawn(listener),
        Err(error) if is_in_use(&error) => Err(IpcError::AddrInUse),
        Err(error) => Err(error.into()),
    }
}

fn endpoint_is_live(path: &Path) -> bool {
    for attempt in 0..5 {
        match connect_path(path) {
            Ok(_) => return true,
            Err(IpcError::Io(error)) if is_dead(&error) => {
                if attempt == 4 {
                    return false;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return true,
        }
    }
    false
}

fn listen(path: &Path) -> std::io::Result<Listener> {
    let listener = create_listener(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(listener)
}

fn create_listener(path: &Path) -> std::io::Result<Listener> {
    #[cfg(unix)]
    {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        let name = socket_name(path)?;
        match ListenerOptions::new()
            .name(name)
            .reclaim_name(true)
            .mode(0o600)
            .create_sync()
        {
            Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
                let name = socket_name(path)?;
                ListenerOptions::new()
                    .name(name)
                    .reclaim_name(true)
                    .create_sync()
            }
            other => other,
        }
    }
    #[cfg(not(unix))]
    {
        let name = socket_name(path)?;
        ListenerOptions::new()
            .name(name)
            .reclaim_name(true)
            .create_sync()
    }
}

fn connect_path(path: &Path) -> Result<interprocess::local_socket::Stream, IpcError> {
    let name = socket_name(path)?;
    ConnectOptions::new()
        .name(name)
        .wait_mode(ConnectWaitMode::Timeout(ACK_TIMEOUT))
        .connect_sync()
        .map_err(IpcError::Io)
}

fn socket_name(path: &Path) -> std::io::Result<Name<'static>> {
    path.to_path_buf().to_fs_name::<GenericFilePath>()
}

fn is_in_use(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AddrInUse
}

fn is_dead(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::NotFound
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
    )
}

fn absolute_fallback(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

#[cfg(not(windows))]
fn runtime_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        env_path("TMPDIR").unwrap_or_else(|| PathBuf::from("/tmp"))
    } else {
        env_path("XDG_RUNTIME_DIR")
            .or_else(|| env_path("TMPDIR"))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
    }
}

#[cfg(not(windows))]
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
