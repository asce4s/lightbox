//! Single-instance client and server for the GUI (SPEC-VIEWER §V2.3).

use crate::error::{AppError, Result};
use crate::launch::Launch;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// This process owns the endpoint, or a running instance accepted the launch.
pub enum Acquire {
    /// Bind succeeded. Drain [`InstanceServer::poll`] on the UI thread.
    Server(InstanceServer),
    /// The running instance acknowledged the launch. The caller should exit.
    Forwarded,
}

/// Background accept loop. Messages become [`Launch`] values for the window pump.
pub struct InstanceServer {
    inner: lb_ipc::Server,
}

impl InstanceServer {
    /// Binds the endpoint for this data directory.
    pub fn bind(data_dir: &Path) -> Result<Self> {
        lb_ipc::Server::bind(data_dir)
            .map(|inner| Self { inner })
            .map_err(ipc_error)
    }

    /// The next launch forwarded by another process.
    pub fn poll(&self) -> Option<Launch> {
        self.inner.try_recv().map(request_to_launch)
    }
}

/// Connects to a running instance or becomes the server.
///
/// When `bind` loses a race, `connect` is retried three times, 50 ms apart.
pub fn acquire(data_dir: &Path, launch: &Launch) -> Result<Acquire> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Some(response) = try_forward(data_dir, launch, &cwd) {
        return accepted(response);
    }
    match InstanceServer::bind(data_dir) {
        Ok(server) => Ok(Acquire::Server(server)),
        Err(error) if is_taken(&error) => {
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(50));
                if let Some(response) = try_forward(data_dir, launch, &cwd) {
                    return accepted(response);
                }
            }
            Err(AppError::Invalid(
                "could not connect to the running Lightbox".into(),
            ))
        }
        Err(error) => Err(error),
    }
}

fn try_forward(data_dir: &Path, launch: &Launch, cwd: &Path) -> Option<lb_ipc::Response> {
    let request = launch_to_request(launch, cwd);
    lb_ipc::send(data_dir, &request).ok()
}

fn accepted(response: lb_ipc::Response) -> Result<Acquire> {
    if response.ok {
        Ok(Acquire::Forwarded)
    } else {
        Err(AppError::Invalid(response.error.unwrap_or_else(|| {
            "the running Lightbox rejected the launch".to_owned()
        })))
    }
}

fn is_taken(error: &AppError) -> bool {
    matches!(error, AppError::Invalid(message) if message.contains("already taken"))
}

fn launch_to_request(launch: &Launch, cwd: &Path) -> lb_ipc::Request {
    match launch {
        Launch::View { paths, .. } => lb_ipc::Request::open_viewer(
            paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            cwd.to_string_lossy().into_owned(),
        ),
        Launch::Library | Launch::UnregisterFileTypes => lb_ipc::Request::show_library(),
    }
}

fn request_to_launch(request: lb_ipc::Request) -> Launch {
    match request {
        lb_ipc::Request::ShowLibrary => Launch::Library,
        lb_ipc::Request::OpenViewer { paths, cwd } => {
            let cwd = PathBuf::from(cwd);
            let paths = paths
                .into_iter()
                .map(|path| {
                    let path = PathBuf::from(path);
                    if path.is_absolute() {
                        path
                    } else {
                        cwd.join(path)
                    }
                })
                .collect();
            Launch::View { paths, perf: false }
        }
    }
}

fn ipc_error(error: lb_ipc::IpcError) -> AppError {
    match error {
        lb_ipc::IpcError::Io(error) => AppError::Io(error),
        other => AppError::Invalid(other.to_string()),
    }
}
