//! Lightbox GUI binary: wires `lb-app` and `lb-ui`.

use lb_app::{Acquire, JobQueue, Launch, Lightbox, Platform};
use std::path::PathBuf;

fn main() {
    let process_start = std::time::Instant::now();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let platform = if cfg!(target_os = "macos") {
        Platform::Macos
    } else {
        Platform::Other
    };
    let launch = lb_app::parse_launch(&args, &cwd, platform);
    if matches!(launch, Launch::UnregisterFileTypes) {
        if let Err(error) = lb_app::unregister_file_types() {
            tracing::error!(%error, "could not unregister file types");
            eprintln!("{}", error.user_message());
            std::process::exit(2);
        }
        return;
    }
    let jobs = JobQueue::threaded(JobQueue::default_workers());
    let app = match Lightbox::open_default(jobs) {
        Ok(app) => app,
        Err(error) => {
            tracing::error!(%error, "could not open the library");
            eprintln!(
                "Lightbox could not open its library: {}",
                error.user_message()
            );
            std::process::exit(2);
        }
    };
    lb_app::install_panic_hook(app.paths().data.clone());
    if let Launch::View { paths, perf: true } = &launch {
        let Some(path) = paths.first() else {
            eprintln!("lightbox view --perf-json needs a file");
            std::process::exit(2);
        };
        match lb_app::capture_viewer_perf(&app, path, process_start) {
            Ok(report) => {
                println!("{}", report.to_json());
                return;
            }
            Err(error) => {
                tracing::error!(%error, "viewer perf capture failed");
                eprintln!("{}", error.user_message());
                std::process::exit(2);
            }
        }
    }
    let server = match lb_app::acquire(&app.paths().data, &launch) {
        Ok(Acquire::Forwarded) => return,
        Ok(Acquire::Server(server)) => server,
        Err(error) => {
            tracing::error!(%error, "single instance failed");
            eprintln!("{}", error.user_message());
            std::process::exit(2);
        }
    };
    if let Err(error) = lb_app::register_available_viewer() {
        tracing::warn!(%error, "could not register as an available photo viewer");
    }
    if matches!(launch, Launch::Library) {
        let background = app.clone();
        std::thread::spawn(move || {
            if let Err(error) = lb_app::apply_startup(&background, &Launch::Library) {
                tracing::warn!(%error, "startup scan failed");
            }
        });
    }
    lb_ui::run(app, launch, server);
}
