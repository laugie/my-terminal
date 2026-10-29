//! Upload and download of one file or folder. A folder is walked first so the
//! total size is known, then created and copied in order. Existing files at
//! the destination are overwritten and existing folders are merged. A
//! download goes to a hidden part file first, so a failed overwrite keeps the
//! local original; an upload writes the remote file in place.
//!
//! Symbolic links to files are copied as files. Links to folders inside a
//! folder are skipped so a link loop cannot recurse forever.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use super::client::{expect_data, expect_ok, SftpClient, SftpError, Ticket};
use super::proto::{self, Attrs};

/// Bytes per READ / WRITE request. Every SFTP server accepts 32 KiB.
pub const CHUNK: u32 = 32 * 1024;
/// Requests kept in flight per file: 32 × 32 KiB = 1 MiB, enough to fill
/// a link with a few hundred milliseconds of round trip.
pub const WINDOW: usize = 32;
/// Deepest folder nesting walked.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    Permission,
    DiskFull,
    Connection,
    NotFound,
    Cancelled,
    Other,
}

impl FailKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FailKind::Permission => "permission",
            FailKind::DiskFull => "diskFull",
            FailKind::Connection => "connection",
            FailKind::NotFound => "notFound",
            FailKind::Cancelled => "cancelled",
            FailKind::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferError {
    pub kind: FailKind,
    pub message: String,
}

impl TransferError {
    fn new(kind: FailKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn cancelled() -> Self {
        Self::new(FailKind::Cancelled, "已取消")
    }
}

/// What a transfer reports while it runs: bytes copied and bytes in total.
pub type OnProgress<'a> = &'a mut dyn FnMut(u64, u64);

struct Item {
    remote: String,
    local: PathBuf,
    dir: bool,
    size: u64,
    mode: Option<u32>,
    mtime: Option<u32>,
}

/// Total bytes of a plan, for the first progress report.
fn total_of(items: &[Item]) -> u64 {
    items
        .iter()
        .filter(|item| !item.dir)
        .map(|item| item.size)
        .sum()
}

/// `dir` + `/` + `name`, without doubling a trailing slash.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Last component of a remote path. `None` for `/` or an empty path.
pub fn remote_name(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

/// Whether a remote file name can be created locally without leaving the
/// destination folder. `windows` applies the Windows naming rules.
pub fn local_name_ok(name: &str, windows: bool) -> bool {
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    if name.contains('/') || name.contains('\0') {
        return false;
    }
    if windows {
        if name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        }) {
            return false;
        }
        if name.ends_with('.') || name.ends_with(' ') {
            return false;
        }
        let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
            return false;
        }
    }
    true
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), TransferError> {
    if cancel.load(Ordering::Relaxed) {
        Err(TransferError::cancelled())
    } else {
        Ok(())
    }
}

/// Map a local IO error. `path` is shown to the user.
pub fn local_error(err: &io::Error, path: &Path) -> TransferError {
    let shown = path.display();
    if is_disk_full(err) {
        return TransferError::new(FailKind::DiskFull, format!("本地磁盘已满：{shown}"));
    }
    match err.kind() {
        io::ErrorKind::PermissionDenied => {
            TransferError::new(FailKind::Permission, format!("本地权限不足：{shown}"))
        }
        io::ErrorKind::NotFound => {
            TransferError::new(FailKind::NotFound, format!("本地文件不存在：{shown}"))
        }
        _ if cfg!(windows) && err.raw_os_error() == Some(32) => {
            TransferError::new(FailKind::Other, format!("文件正被其他程序占用：{shown}"))
        }
        _ => TransferError::new(FailKind::Other, format!("本地读写失败：{shown}（{err}）")),
    }
}

fn is_disk_full(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::StorageFull {
        return true;
    }
    let Some(code) = err.raw_os_error() else {
        return false;
    };
    if cfg!(windows) {
        // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL, ERROR_DISK_QUOTA_EXCEEDED.
        matches!(code, 39 | 112 | 1295)
    } else if cfg!(target_os = "macos") {
        // ENOSPC, EDQUOT.
        matches!(code, 28 | 69)
    } else {
        // ENOSPC, EDQUOT on Linux.
        matches!(code, 28 | 122)
    }
}

/// Map a remote error. `path` is shown to the user.
pub fn remote_error(err: SftpError, path: &str) -> TransferError {
    match err {
        SftpError::Lost => TransferError::new(FailKind::Connection, "连接已断开"),
        SftpError::Cancelled => TransferError::cancelled(),
        SftpError::Protocol(_) | SftpError::Timeout => {
            TransferError::new(FailKind::Connection, err.to_string())
        }
        SftpError::Status { code, .. } => match code {
            proto::FX_PERMISSION_DENIED => {
                TransferError::new(FailKind::Permission, format!("远端权限不足：{path}"))
            }
            proto::FX_NO_SUCH_FILE => {
                TransferError::new(FailKind::NotFound, format!("远端文件不存在：{path}"))
            }
            proto::FX_NO_CONNECTION | proto::FX_CONNECTION_LOST => {
                TransferError::new(FailKind::Connection, "连接已断开")
            }
            _ => TransferError::new(FailKind::Other, format!("{err}：{path}")),
        },
    }
}

/// A failed remote write. OpenSSH reports a full disk as a plain FAILURE, so
/// ask for the free space before calling it something else.
fn remote_write_error(client: &SftpClient, err: SftpError, path: &str, dir: &str) -> TransferError {
    if err.code() == Some(proto::FX_FAILURE) {
        if let Some(free) = client.available_bytes(dir) {
            if free < u64::from(CHUNK) * 2 {
                return TransferError::new(FailKind::DiskFull, format!("远端磁盘已满：{path}"));
            }
        }
        return TransferError::new(FailKind::Other, format!("远端写入失败：{path}"));
    }
    remote_error(err, path)
}

// ---------------------------------------------------------------- download

/// Copy the remote file or folder at `source` into the local `dest_dir`.
pub fn download(
    client: &SftpClient,
    source: &str,
    dest_dir: &Path,
    cancel: &AtomicBool,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let name =
        remote_name(source).ok_or_else(|| TransferError::new(FailKind::Other, "不能下载根目录"))?;
    if !local_name_ok(name, cfg!(windows)) {
        return Err(TransferError::new(
            FailKind::Other,
            format!("文件名在本机无效：{name}"),
        ));
    }
    let top = client
        .stat(source)
        .map_err(|err| remote_error(err, source))?;
    let mut items = Vec::new();
    let root = Item {
        remote: source.to_string(),
        local: dest_dir.join(name),
        dir: top.is_dir(),
        size: top.size.unwrap_or(0),
        mode: top.permissions,
        mtime: top.mtime(),
    };
    let walk_from = root.dir.then(|| (root.remote.clone(), root.local.clone()));
    items.push(root);
    if let Some((remote, local)) = walk_from {
        walk_remote(client, &remote, &local, 1, cancel, &mut items)?;
    }

    let total = total_of(&items);
    let mut done = 0u64;
    progress(done, total);
    for item in &items {
        check_cancel(cancel)?;
        if item.dir {
            make_local_dir(&item.local)?;
            continue;
        }
        download_file(client, item, cancel, &mut done, total, progress)?;
    }
    progress(done, total);
    Ok(())
}

fn walk_remote(
    client: &SftpClient,
    remote: &str,
    local: &Path,
    depth: usize,
    cancel: &AtomicBool,
    items: &mut Vec<Item>,
) -> Result<(), TransferError> {
    check_cancel(cancel)?;
    if depth > MAX_DEPTH {
        return Err(TransferError::new(
            FailKind::Other,
            format!("文件夹层级过深：{remote}"),
        ));
    }
    let mut entries = client
        .read_dir(remote)
        .map_err(|err| remote_error(err, remote))?;
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));
    for entry in entries {
        let path = remote_join(remote, &entry.filename);
        if !local_name_ok(&entry.filename, cfg!(windows)) {
            return Err(TransferError::new(
                FailKind::Other,
                format!("文件名在本机无效：{path}"),
            ));
        }
        let child_local = local.join(&entry.filename);
        let mut attrs = entry.attrs;
        if attrs.is_link() {
            match client.stat(&path) {
                Ok(target) if target.is_file() && !target.is_dir() => attrs = target,
                // Dangling links and links to folders are not followed.
                _ => continue,
            }
        }
        if attrs.is_dir() {
            items.push(Item {
                remote: path.clone(),
                local: child_local.clone(),
                dir: true,
                size: 0,
                mode: attrs.permissions,
                mtime: attrs.mtime(),
            });
            walk_remote(client, &path, &child_local, depth + 1, cancel, items)?;
        } else if attrs.is_file() {
            items.push(Item {
                remote: path,
                local: child_local,
                dir: false,
                size: attrs.size.unwrap_or(0),
                mode: attrs.permissions,
                mtime: attrs.mtime(),
            });
        }
    }
    Ok(())
}

fn make_local_dir(path: &Path) -> Result<(), TransferError> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Err(TransferError::new(
            FailKind::Other,
            format!("已有同名文件，无法创建文件夹：{}", path.display()),
        )),
        Err(err) => Err(local_error(&err, path)),
    }
}

fn download_file(
    client: &SftpClient,
    item: &Item,
    cancel: &AtomicBool,
    done: &mut u64,
    total: u64,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let handle = client
        .open(&item.remote, proto::OPEN_READ, &Attrs::default())
        .map_err(|err| remote_error(err, &item.remote))?;
    // Write next to the destination and rename at the end, so a failed or
    // cancelled overwrite leaves the original file as it was.
    let part = part_path(&item.local);
    let mut file = match File::create(&part) {
        Ok(file) => file,
        Err(err) => {
            let _ = client.close(&handle);
            return Err(local_error(&err, &item.local));
        }
    };
    let result = read_into(
        client, &handle, item, &mut file, cancel, done, total, progress,
    )
    .and_then(|_| file.flush().map_err(|err| local_error(&err, &item.local)));
    if result.is_ok() {
        finish_local(&file, item);
    }
    drop(file);
    let _ = client.close(&handle);
    let result = result.and_then(|_| {
        replace_file(&part, &item.local).map_err(|err| local_error(&err, &item.local))
    });
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// A hidden, short sibling name for the file being downloaded.
fn part_path(dest: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    dest.with_file_name(format!(".my-terminal-{}-{n}.part", std::process::id()))
}

/// Rename over an existing file. On Windows a scanner may hold a new file for
/// a moment, so a sharing or access error is retried briefly.
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let mut tries = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err)
                if cfg!(windows)
                    && tries < 5
                    && matches!(err.raw_os_error(), Some(5) | Some(32)) =>
            {
                tries += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => return Err(err),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn read_into(
    client: &SftpClient,
    handle: &[u8],
    item: &Item,
    file: &mut File,
    cancel: &AtomicBool,
    done: &mut u64,
    total: u64,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let remote_err = |err| remote_error(err, &item.remote);
    // (offset, length, reply), oldest first. Data is written in this order.
    let mut inflight: std::collections::VecDeque<(u64, u32, Ticket)> =
        std::collections::VecDeque::new();
    let mut next = 0u64;
    let mut at_eof = false;
    loop {
        while !at_eof && inflight.len() < WINDOW && next < item.size {
            let len = (item.size - next).min(u64::from(CHUNK)) as u32;
            let ticket = client.send_read(handle, next, len).map_err(remote_err)?;
            inflight.push_back((next, len, ticket));
            next += u64::from(len);
        }
        let Some((offset, len, ticket)) = inflight.pop_front() else {
            break;
        };
        let reply = ticket.wait(Some(cancel)).map_err(remote_err)?;
        match expect_data(reply).map_err(remote_err)? {
            Some(data) => {
                if data.is_empty() || data.len() > len as usize {
                    return Err(TransferError::new(
                        FailKind::Connection,
                        "SFTP 协议错误：读取长度无效",
                    ));
                }
                file.write_all(&data)
                    .map_err(|err| local_error(&err, &item.local))?;
                *done += data.len() as u64;
                progress(*done, total);
                let got = data.len() as u32;
                if got < len {
                    // Short read: ask for the rest before anything queued after it.
                    let rest = client
                        .send_read(handle, offset + u64::from(got), len - got)
                        .map_err(remote_err)?;
                    inflight.push_front((offset + u64::from(got), len - got, rest));
                }
            }
            None => {
                // The file shrank since the walk. Keep what was there.
                at_eof = true;
                inflight.clear();
            }
        }
    }
    Ok(())
}

fn finish_local(file: &File, item: &Item) {
    if let Some(mtime) = item.mtime {
        let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs(u64::from(mtime)));
    }
    #[cfg(unix)]
    if let Some(mode) = item.mode {
        use std::os::unix::fs::PermissionsExt;
        // Keep owner write so the file can be overwritten by the next copy.
        let _ = file.set_permissions(fs::Permissions::from_mode(mode & 0o777 | 0o200));
    }
}

// ---------------------------------------------------------------- upload

/// Copy the local file or folder at `source` into the remote `dest_dir`.
pub fn upload(
    client: &SftpClient,
    source: &Path,
    dest_dir: &str,
    cancel: &AtomicBool,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| TransferError::new(FailKind::Other, "不能上传磁盘根目录"))?;
    let top = fs::metadata(source).map_err(|err| local_error(&err, source))?;
    let mut items = Vec::new();
    let root = Item {
        remote: remote_join(dest_dir, &name),
        local: source.to_path_buf(),
        dir: top.is_dir(),
        size: if top.is_dir() { 0 } else { top.len() },
        mode: local_mode(&top),
        mtime: local_mtime(&top),
    };
    let walk_from = root.dir.then(|| (root.local.clone(), root.remote.clone()));
    items.push(root);
    if let Some((local, remote)) = walk_from {
        walk_local(&local, &remote, 1, cancel, &mut items)?;
    }

    let total = total_of(&items);
    let mut done = 0u64;
    progress(done, total);
    for item in &items {
        check_cancel(cancel)?;
        if item.dir {
            make_remote_dir(client, &item.remote)?;
            continue;
        }
        upload_file(client, item, dest_dir, cancel, &mut done, total, progress)?;
    }
    progress(done, total);
    Ok(())
}

fn walk_local(
    local: &Path,
    remote: &str,
    depth: usize,
    cancel: &AtomicBool,
    items: &mut Vec<Item>,
) -> Result<(), TransferError> {
    check_cancel(cancel)?;
    if depth > MAX_DEPTH {
        return Err(TransferError::new(
            FailKind::Other,
            format!("文件夹层级过深：{}", local.display()),
        ));
    }
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(local)
        .map_err(|err| local_error(&err, local))?
        .collect::<Result<_, _>>()
        .map_err(|err| local_error(&err, local))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_remote = remote_join(remote, &name);
        let kind = entry.file_type().map_err(|err| local_error(&err, &path))?;
        let meta = if kind.is_symlink() {
            match fs::metadata(&path) {
                Ok(meta) if meta.is_file() => meta,
                _ => continue,
            }
        } else {
            entry.metadata().map_err(|err| local_error(&err, &path))?
        };
        if meta.is_dir() {
            items.push(Item {
                remote: child_remote.clone(),
                local: path.clone(),
                dir: true,
                size: 0,
                mode: local_mode(&meta),
                mtime: local_mtime(&meta),
            });
            walk_local(&path, &child_remote, depth + 1, cancel, items)?;
        } else if meta.is_file() {
            items.push(Item {
                remote: child_remote,
                local: path,
                dir: false,
                size: meta.len(),
                mode: local_mode(&meta),
                mtime: local_mtime(&meta),
            });
        }
    }
    Ok(())
}

fn local_mode(meta: &fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

fn local_mtime(meta: &fs::Metadata) -> Option<u32> {
    let modified = meta.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    u32::try_from(secs).ok()
}

fn make_remote_dir(client: &SftpClient, path: &str) -> Result<(), TransferError> {
    match client.mkdir(path, &Attrs::default()) {
        Ok(()) => Ok(()),
        Err(err @ SftpError::Status { .. }) => match client.stat(path) {
            Ok(attrs) if attrs.is_dir() => Ok(()),
            Ok(_) => Err(TransferError::new(
                FailKind::Other,
                format!("已有同名文件，无法创建文件夹：{path}"),
            )),
            Err(_) => Err(remote_error(err, path)),
        },
        Err(err) => Err(remote_error(err, path)),
    }
}

fn upload_file(
    client: &SftpClient,
    item: &Item,
    dest_dir: &str,
    cancel: &AtomicBool,
    done: &mut u64,
    total: u64,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let mut file = File::open(&item.local).map_err(|err| local_error(&err, &item.local))?;
    // Owner write stays on so the next upload can overwrite the file.
    let attrs = Attrs {
        permissions: item.mode.map(|mode| mode | 0o200),
        ..Attrs::default()
    };
    let handle = client
        .open(
            &item.remote,
            proto::OPEN_WRITE | proto::OPEN_CREAT | proto::OPEN_TRUNC,
            &attrs,
        )
        .map_err(|err| remote_error(err, &item.remote))?;
    let written = write_from(
        client, &handle, item, &mut file, dest_dir, cancel, done, total, progress,
    );
    if written.is_ok() {
        if let Some(mtime) = item.mtime {
            let _ = client.fsetstat(
                &handle,
                &Attrs {
                    times: Some((mtime, mtime)),
                    ..Attrs::default()
                },
            );
        }
    }
    let closed = client.close(&handle);
    let result = written.and_then(|_| {
        closed.map_err(|err| remote_write_error(client, err, &item.remote, dest_dir))
    });
    if let Err(err) = &result {
        if err.kind != FailKind::Connection {
            let _ = client.remove(&item.remote);
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn write_from(
    client: &SftpClient,
    handle: &[u8],
    item: &Item,
    file: &mut File,
    dest_dir: &str,
    cancel: &AtomicBool,
    done: &mut u64,
    total: u64,
    progress: OnProgress<'_>,
) -> Result<(), TransferError> {
    let mut inflight: std::collections::VecDeque<(u64, Ticket)> = std::collections::VecDeque::new();
    let mut offset = 0u64;
    let mut buf = vec![0u8; CHUNK as usize];
    let mut ended = false;
    loop {
        while !ended && inflight.len() < WINDOW {
            check_cancel(cancel)?;
            let read = read_full(file, &mut buf).map_err(|err| local_error(&err, &item.local))?;
            if read == 0 {
                ended = true;
                break;
            }
            let ticket = client
                .send_write(handle, offset, &buf[..read])
                .map_err(|err| remote_error(err, &item.remote))?;
            offset += read as u64;
            inflight.push_back((read as u64, ticket));
        }
        let Some((len, ticket)) = inflight.pop_front() else {
            break;
        };
        let reply = ticket
            .wait(Some(cancel))
            .map_err(|err| remote_error(err, &item.remote))?;
        expect_ok(reply).map_err(|err| remote_write_error(client, err, &item.remote, dest_dir))?;
        *done += len;
        progress(*done, total);
    }
    Ok(())
}

/// Fill `buf` unless the file ends first.
fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_join_does_not_double_slashes() {
        assert_eq!(remote_join("/", "etc"), "/etc");
        assert_eq!(remote_join("/home/me", "a.txt"), "/home/me/a.txt");
        assert_eq!(remote_join("/home/me/", "a.txt"), "/home/me/a.txt");
        assert_eq!(remote_join("/C:/Users", "x"), "/C:/Users/x");
    }

    #[test]
    fn remote_name_is_the_last_component() {
        assert_eq!(remote_name("/home/me/a.txt"), Some("a.txt"));
        assert_eq!(remote_name("/home/me/docs/"), Some("docs"));
        assert_eq!(remote_name("relative"), Some("relative"));
        assert_eq!(remote_name("/"), None);
        assert_eq!(remote_name(""), None);
        assert_eq!(remote_name("/home/.."), None);
    }

    #[test]
    fn remote_names_that_escape_the_folder_are_refused() {
        for name in ["", ".", "..", "a/b", "nul\0"] {
            assert!(!local_name_ok(name, false), "{name:?}");
            assert!(!local_name_ok(name, true), "{name:?}");
        }
        assert!(local_name_ok("report.txt", true));
        assert!(local_name_ok("a:b", false));
        for name in [
            "..\\evil", "a:b", "x?", "trail.", "trail ", "CON", "com1.txt", "LPT9",
        ] {
            assert!(!local_name_ok(name, true), "{name:?}");
        }
        assert!(local_name_ok("console.log", true));
        assert!(local_name_ok("COM10", true));
    }

    #[test]
    fn local_errors_are_grouped_by_cause() {
        let path = Path::new("/x/y");
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(local_error(&denied, path).kind, FailKind::Permission);
        let missing = io::Error::from(io::ErrorKind::NotFound);
        assert_eq!(local_error(&missing, path).kind, FailKind::NotFound);
        let full = io::Error::from(io::ErrorKind::StorageFull);
        let full = local_error(&full, path);
        assert_eq!(full.kind, FailKind::DiskFull);
        assert!(full.message.starts_with("本地磁盘已满"), "{}", full.message);
        #[cfg(unix)]
        {
            let raw = io::Error::from_raw_os_error(28);
            assert_eq!(local_error(&raw, path).kind, FailKind::DiskFull);
        }
    }

    #[test]
    fn remote_errors_are_grouped_by_cause() {
        let status = |code| SftpError::Status {
            code,
            message: String::new(),
        };
        assert_eq!(
            remote_error(status(proto::FX_PERMISSION_DENIED), "/r").kind,
            FailKind::Permission
        );
        assert_eq!(
            remote_error(status(proto::FX_NO_SUCH_FILE), "/r").kind,
            FailKind::NotFound
        );
        assert_eq!(
            remote_error(SftpError::Lost, "/r").kind,
            FailKind::Connection
        );
        assert_eq!(
            remote_error(status(proto::FX_CONNECTION_LOST), "/r").kind,
            FailKind::Connection
        );
        assert_eq!(
            remote_error(SftpError::Cancelled, "/r").kind,
            FailKind::Cancelled
        );
        assert_eq!(
            remote_error(status(proto::FX_FAILURE), "/r").kind,
            FailKind::Other
        );
    }
}
