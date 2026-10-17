//! Lightbox GUI binary: wires `lb-app` and `lb-ui`.

use lb_app::{JobQueue, Lightbox};

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
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
    let background = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = background.library().rescan_all() {
            tracing::warn!(%error, "startup scan failed");
        }
    });
    lb_ui::run(app);
}
