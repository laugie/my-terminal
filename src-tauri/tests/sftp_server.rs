//! The SFTP client and transfers against a real OpenSSH `sftp-server`,
//! started directly over pipes (no sshd, no network). Both "local" and
//! "remote" are folders on this machine.
//!
//! Skipped with a note when no `sftp-server` binary is installed.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use my_terminal_lib::sftp::client::SftpClient;
use my_terminal_lib::sftp::local::list_remote;
use my_terminal_lib::sftp::transfer::{download, upload, FailKind};

const CANDIDATES: [&str; 4] = [
    "/usr/libexec/sftp-server",
    "/usr/lib/openssh/sftp-server",
    "/usr/libexec/openssh/sftp-server",
    "/usr/lib/ssh/sftp-server",
];

struct Server {
    child: Child,
    client: Arc<SftpClient>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start() -> Option<Server> {
    let Some(binary) = CANDIDATES.iter().find(|path| Path::new(path).is_file()) else {
        eprintln!("skip: no sftp-server binary in {CANDIDATES:?}");
        return None;
    };
    let mut child = Command::new(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sftp-server");
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let client = SftpClient::start(stdout, stdin, Duration::from_secs(10)).expect("handshake");
    Some(Server { child, client })
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mt-sftp-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        // macOS puts temp files under a /var -> /private/var link.
        Self(fs::canonicalize(&dir).unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn text(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = Command::new("chmod")
            .args(["-R", "u+rwx"])
            .arg(&self.0)
            .status();
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Bytes that are not all the same, so a misplaced chunk is caught.
fn pattern(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

fn tree(root: &Path) {
    fs::create_dir_all(root.join("nested/deeper")).unwrap();
    fs::create_dir_all(root.join("empty")).unwrap();
    fs::write(root.join("small.txt"), b"hello sftp\n").unwrap();
    // Not a multiple of the chunk size, and larger than the whole window.
    fs::write(
        root.join("nested/big.bin"),
        pattern(3 * 1024 * 1024 + 123, 7),
    )
    .unwrap();
    fs::write(root.join("nested/deeper/zero.bin"), b"").unwrap();
    fs::write(root.join("中文 名字.txt"), "内容".as_bytes()).unwrap();
}

fn assert_same_tree(a: &Path, b: &Path) {
    let mut names_a: Vec<_> = fs::read_dir(a)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let mut names_b: Vec<_> = fs::read_dir(b)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names_a.sort();
    names_b.sort();
    assert_eq!(names_a, names_b, "{} vs {}", a.display(), b.display());
    for name in names_a {
        let (pa, pb) = (a.join(&name), b.join(&name));
        if pa.is_dir() {
            assert!(pb.is_dir(), "{} is not a folder", pb.display());
            assert_same_tree(&pa, &pb);
        } else {
            assert_eq!(
                fs::read(&pa).unwrap(),
                fs::read(&pb).unwrap(),
                "{}",
                pa.display()
            );
        }
    }
}

#[test]
fn folder_round_trip_keeps_every_byte_and_reports_progress() {
    let Some(server) = start() else { return };
    let local = Scratch::new("local");
    let remote = Scratch::new("remote");
    let back = Scratch::new("back");
    let source = local.path().join("project");
    tree(&source);

    let cancel = AtomicBool::new(false);
    let mut last = (0, 0);
    let mut calls = 0;
    upload(
        &server.client,
        &source,
        remote.text(),
        &cancel,
        &mut |done, total| {
            assert!(done <= total);
            last = (done, total);
            calls += 1;
        },
    )
    .expect("upload");
    let expected = 11 + 3 * 1024 * 1024 + 123 + "内容".len() as u64;
    assert_eq!(last, (expected, expected));
    assert!(calls > 10, "only {calls} progress reports");
    assert_same_tree(&source, &remote.path().join("project"));

    let remote_project = format!("{}/project", remote.text());
    download(
        &server.client,
        &remote_project,
        back.path(),
        &cancel,
        &mut |_, _| {},
    )
    .expect("download");
    assert_same_tree(&source, &back.path().join("project"));

    // A second upload overwrites in place and merges folders.
    fs::write(source.join("small.txt"), b"changed").unwrap();
    upload(
        &server.client,
        &source,
        remote.text(),
        &cancel,
        &mut |_, _| {},
    )
    .expect("again");
    assert_eq!(
        fs::read(remote.path().join("project/small.txt")).unwrap(),
        b"changed"
    );
}

#[test]
fn single_file_download_and_listing() {
    let Some(server) = start() else { return };
    let remote = Scratch::new("remote-list");
    let back = Scratch::new("back-list");
    fs::create_dir(remote.path().join("docs")).unwrap();
    fs::write(remote.path().join("a.bin"), pattern(70_000, 3)).unwrap();

    let home = server.client.realpath(".").expect("realpath");
    assert!(home.starts_with('/'), "{home}");

    let listing = list_remote(&server.client, remote.text()).expect("list");
    assert_eq!(listing.path, remote.text());
    let names: Vec<_> = listing
        .entries
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["docs", "a.bin"]);
    assert!(listing.entries[0].dir);
    assert_eq!(listing.entries[1].size, 70_000);
    assert_eq!(listing.entries[1].path, format!("{}/a.bin", remote.text()));
    assert!(listing.parent.is_some());

    let cancel = AtomicBool::new(false);
    download(
        &server.client,
        &listing.entries[1].path,
        back.path(),
        &cancel,
        &mut |_, _| {},
    )
    .expect("download");
    assert_eq!(
        fs::read(back.path().join("a.bin")).unwrap(),
        pattern(70_000, 3)
    );
}

#[test]
fn a_read_only_destination_is_permission_denied() {
    let Some(server) = start() else { return };
    let local = Scratch::new("local-ro");
    let remote = Scratch::new("remote-ro");
    let file = local.path().join("f.txt");
    fs::write(&file, b"data").unwrap();
    let locked = remote.path().join("locked");
    fs::create_dir(&locked).unwrap();
    Command::new("chmod")
        .arg("555")
        .arg(&locked)
        .status()
        .unwrap();
    if fs::write(locked.join("probe"), b"x").is_ok() {
        eprintln!("skip: running as a user that ignores folder permissions");
        return;
    }
    let cancel = AtomicBool::new(false);
    let err = upload(
        &server.client,
        &file,
        locked.to_str().unwrap(),
        &cancel,
        &mut |_, _| {},
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::Permission, "{err:?}");
    assert!(err.message.contains("权限不足"), "{}", err.message);

    // Download into a read-only local folder.
    let local_locked = local.path().join("locked");
    fs::create_dir(&local_locked).unwrap();
    fs::write(remote.path().join("g.txt"), b"data").unwrap();
    Command::new("chmod")
        .arg("555")
        .arg(&local_locked)
        .status()
        .unwrap();
    let source = format!("{}/g.txt", remote.text());
    let err = download(
        &server.client,
        &source,
        &local_locked,
        &cancel,
        &mut |_, _| {},
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::Permission, "{err:?}");
}

#[test]
fn a_missing_source_is_not_found() {
    let Some(server) = start() else { return };
    let back = Scratch::new("back-missing");
    let cancel = AtomicBool::new(false);
    let err = download(
        &server.client,
        "/definitely/not/here.txt",
        back.path(),
        &cancel,
        &mut |_, _| {},
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::NotFound, "{err:?}");
}

#[test]
fn cancel_stops_and_removes_the_partial_file() {
    let Some(server) = start() else { return };
    let local = Scratch::new("local-cancel");
    let remote = Scratch::new("remote-cancel");
    let file = local.path().join("big.bin");
    fs::write(&file, pattern(8 * 1024 * 1024, 11)).unwrap();
    let cancel = AtomicBool::new(false);
    let err = upload(
        &server.client,
        &file,
        remote.text(),
        &cancel,
        &mut |done, _| {
            if done > 256 * 1024 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::Cancelled, "{err:?}");
    assert!(
        !remote.path().join("big.bin").exists(),
        "partial upload left behind"
    );

    // The connection is still usable after a cancel.
    let cancel = AtomicBool::new(false);
    upload(
        &server.client,
        &file,
        remote.text(),
        &cancel,
        &mut |_, _| {},
    )
    .expect("after cancel");
    assert_eq!(
        fs::read(remote.path().join("big.bin")).unwrap().len(),
        8 * 1024 * 1024
    );

    let back = Scratch::new("back-cancel");
    fs::write(back.path().join("big.bin"), b"original").unwrap();
    let cancel = AtomicBool::new(false);
    let source = format!("{}/big.bin", remote.text());
    let err = download(
        &server.client,
        &source,
        back.path(),
        &cancel,
        &mut |done, _| {
            if done > 256 * 1024 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::Cancelled, "{err:?}");
    // A cancelled overwrite keeps the original and leaves no part file.
    assert_eq!(fs::read(back.path().join("big.bin")).unwrap(), b"original");
    let names: Vec<_> = fs::read_dir(back.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["big.bin"]);
}

#[test]
fn read_only_files_can_be_copied_again() {
    let Some(server) = start() else { return };
    let local = Scratch::new("local-0444");
    let remote = Scratch::new("remote-0444");
    let back = Scratch::new("back-0444");
    let file = local.path().join("ro.txt");
    fs::write(&file, b"read only").unwrap();
    Command::new("chmod")
        .arg("444")
        .arg(&file)
        .status()
        .unwrap();
    let cancel = AtomicBool::new(false);
    for _ in 0..2 {
        upload(
            &server.client,
            &file,
            remote.text(),
            &cancel,
            &mut |_, _| {},
        )
        .expect("upload");
    }
    let source = format!("{}/ro.txt", remote.text());
    Command::new("chmod")
        .arg("444")
        .arg(&source)
        .status()
        .unwrap();
    for _ in 0..2 {
        download(
            &server.client,
            &source,
            back.path(),
            &cancel,
            &mut |_, _| {},
        )
        .expect("download");
    }
    assert_eq!(fs::read(back.path().join("ro.txt")).unwrap(), b"read only");
}

#[test]
fn a_dropped_connection_is_reported_as_lost() {
    let Some(mut server) = start() else { return };
    let local = Scratch::new("local-lost");
    let remote = Scratch::new("remote-lost");
    let file = local.path().join("big.bin");
    fs::write(&file, pattern(16 * 1024 * 1024, 5)).unwrap();
    let pid = server.child.id();
    let mut killed = false;
    let cancel = AtomicBool::new(false);
    let err = upload(
        &server.client,
        &file,
        remote.text(),
        &cancel,
        &mut |done, _| {
            if !killed && done > 512 * 1024 {
                killed = true;
                let _ = Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status();
            }
        },
    )
    .unwrap_err();
    assert_eq!(err.kind, FailKind::Connection, "{err:?}");
    assert_eq!(err.message, "连接已断开");
    let _ = server.child.wait();
    assert!(server.client.is_closed());
    let err = server.client.realpath(".").unwrap_err();
    assert_eq!(err.to_string(), "连接已断开");
}
