//! Running external tools with a timeout; faked in tests.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    /// Executable to run.
    pub program: PathBuf,
    /// Arguments in order.
    pub args: Vec<OsString>,
}

/// What a finished command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// Whether the process exited with status 0.
    pub success: bool,
    /// Everything written to standard output.
    pub stdout: Vec<u8>,
    /// Everything written to standard error.
    pub stderr: Vec<u8>,
}

/// Runs commands. Production uses [`SystemRunner`].
pub trait CommandRunner: Send + Sync + std::fmt::Debug {
    /// Runs `spec` to completion.
    ///
    /// # Errors
    /// Fails when the program cannot be started or exceeds its time limit.
    fn run(&self, spec: &CommandSpec) -> io::Result<CommandOutput>;
}

/// Spawns real processes and kills them after a time limit.
#[derive(Debug, Clone, Copy)]
pub struct SystemRunner {
    /// How long a command may run.
    pub timeout: Duration,
}

impl Default for SystemRunner {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

fn drain(mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        buf
    })
}

impl CommandRunner for SystemRunner {
    fn run(&self, spec: &CommandSpec) -> io::Result<CommandOutput> {
        let mut child = Command::new(&spec.program)
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("{} did not finish in time", spec.program.display()),
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let collect = |h: Option<std::thread::JoinHandle<Vec<u8>>>| {
            h.and_then(|h| h.join().ok()).unwrap_or_default()
        };
        Ok(CommandOutput {
            success: status.success(),
            stdout: collect(out),
            stderr: collect(err),
        })
    }
}
