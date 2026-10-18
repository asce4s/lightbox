//! Single-instance endpoint (SPEC-VIEWER §V2.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_ipc::{MAX_MESSAGE_BYTES, Request, Server};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

fn data_dir() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    (dir, data)
}

#[test]
fn instance_second_launch_forwards_paths() {
    let (_dir, data) = data_dir();
    let server = Server::bind(&data).unwrap();
    let response = lb_ipc::send(
        &data,
        &Request::open_viewer(vec!["/abs/a.jpg".to_owned()], "/abs".to_owned()),
    )
    .unwrap();
    assert!(response.ok);
    assert_eq!(response.v, 1);
    let request = server.try_recv().expect("server queued the launch");
    assert_eq!(
        request,
        Request::open_viewer(vec!["/abs/a.jpg".to_owned()], "/abs".to_owned())
    );
}

#[test]
fn instance_isolated_per_home() {
    let (_a, data_a) = data_dir();
    let (_b, data_b) = data_dir();
    assert_ne!(lb_ipc::socket_path(&data_a), lb_ipc::socket_path(&data_b));
    let server_a = Server::bind(&data_a).unwrap();
    let server_b = Server::bind(&data_b).unwrap();
    lb_ipc::send(
        &data_a,
        &Request::open_viewer(vec!["/only-a.jpg".to_owned()], "/".to_owned()),
    )
    .unwrap();
    assert_eq!(
        server_a.try_recv(),
        Some(Request::open_viewer(
            vec!["/only-a.jpg".to_owned()],
            "/".to_owned()
        ))
    );
    assert!(server_b.try_recv().is_none());
}

#[test]
fn instance_stale_socket_recovered() {
    let (_dir, data) = data_dir();
    let path = lb_ipc::socket_path(&data);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    leave_stale(&path);
    let server = Server::bind(&data).expect("stale endpoint is replaced");
    let response = lb_ipc::send(&data, &Request::show_library()).unwrap();
    assert!(response.ok);
    assert_eq!(server.try_recv(), Some(Request::show_library()));
}

#[test]
fn instance_protocol_rejects_unknown_version() {
    let (_dir, data) = data_dir();
    let server = Server::bind(&data).unwrap();
    let unknown_version =
        lb_ipc::send_line(&data, br#"{"v":2,"cmd":"open_viewer","paths":[],"cwd":""}"#).unwrap();
    assert!(!unknown_version.ok);
    assert!(unknown_version.error.is_some());
    let unknown_cmd = lb_ipc::send_line(&data, br#"{"v":1,"cmd":"nope"}"#).unwrap();
    assert!(!unknown_cmd.ok);
    let mut huge = vec![b'{'; MAX_MESSAGE_BYTES + 64];
    huge.push(b'\n');
    let too_big = lb_ipc::send_line(&data, &huge).unwrap();
    assert!(!too_big.ok);
    let response = lb_ipc::send(&data, &Request::show_library()).unwrap();
    assert!(response.ok);
    assert_eq!(server.try_recv(), Some(Request::show_library()));
}

#[test]
fn instance_race_one_server_wins() {
    let (_dir, data) = data_dir();
    let (tx, rx) = mpsc::channel();
    for _ in 0..8 {
        let data = data.clone();
        let tx = tx.clone();
        thread::spawn(move || {
            let _ = tx.send(Server::bind(&data));
        });
    }
    drop(tx);
    let mut wins = 0;
    let mut servers = Vec::new();
    for result in rx {
        if result.is_ok() {
            wins += 1;
        }
        servers.push(result);
    }
    assert_eq!(wins, 1, "exactly one bind owns the endpoint");
    let _ = servers;
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn leave_stale(path: &std::path::Path) {
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixListener;
    let listener = UnixListener::bind(path).unwrap();
    let raw = listener.into_raw_fd();
    drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) });
}

#[cfg(not(unix))]
fn leave_stale(_path: &std::path::Path) {}
