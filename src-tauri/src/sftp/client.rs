//! SFTP client over any byte pipe (an `ssh -s ... sftp` child in the app,
//! a local `sftp-server` in tests).
//!
//! One reader thread hands each reply to the request with the same id, so
//! several transfers can share the connection and each can keep many reads
//! or writes in flight. When the pipe ends, every waiting request gets
//! [`SftpError::Lost`].

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use super::proto::{self, Attrs, NameEntry, Packet, Reply};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SftpError {
    /// The server answered with a status other than OK.
    Status { code: u32, message: String },
    /// The pipe closed or a write to it failed.
    Lost,
    /// The server sent something that is not valid SFTP.
    Protocol(String),
    /// The caller's cancel flag was set while waiting.
    Cancelled,
    /// No `VERSION` within the start timeout.
    Timeout,
}

impl SftpError {
    pub fn code(&self) -> Option<u32> {
        match self {
            SftpError::Status { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl std::fmt::Display for SftpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SftpError::Status { code, message } => match *code {
                proto::FX_NO_SUCH_FILE => f.write_str("文件或文件夹不存在"),
                proto::FX_PERMISSION_DENIED => f.write_str("权限不足"),
                proto::FX_NO_CONNECTION | proto::FX_CONNECTION_LOST => f.write_str("连接已断开"),
                _ if message.is_empty() || message == "Failure" => {
                    write!(f, "远端操作失败（代码 {code}）")
                }
                _ => write!(f, "远端操作失败：{message}"),
            },
            SftpError::Lost => f.write_str("连接已断开"),
            SftpError::Protocol(detail) => write!(f, "SFTP 协议错误：{detail}"),
            SftpError::Cancelled => f.write_str("已取消"),
            SftpError::Timeout => f.write_str("等待服务器响应超时"),
        }
    }
}

type Slot = Sender<Result<Reply, SftpError>>;

#[derive(Default)]
struct Pending {
    closed: bool,
    waiting: HashMap<u32, Slot>,
}

/// One reply on its way. Dropping it discards the reply when it arrives.
pub struct Ticket {
    rx: Receiver<Result<Reply, SftpError>>,
}

impl Ticket {
    /// Wait for the reply. With `cancel`, gives up once the flag is set.
    pub fn wait(self, cancel: Option<&AtomicBool>) -> Result<Reply, SftpError> {
        loop {
            if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
                return Err(SftpError::Cancelled);
            }
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Err(SftpError::Lost),
            }
        }
    }
}

pub struct SftpClient {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Arc<Mutex<Pending>>,
    next_id: AtomicU32,
    extensions: Vec<(String, String)>,
}

impl SftpClient {
    /// Send `INIT` and wait up to `timeout` for `VERSION`.
    pub fn start(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        timeout: Duration,
    ) -> Result<Arc<SftpClient>, SftpError> {
        let mut writer: Box<dyn Write + Send> = Box::new(writer);
        let mut init = Packet::new(proto::FXP_INIT);
        init.u32(3);
        writer
            .write_all(&init.finish())
            .and_then(|_| writer.flush())
            .map_err(|_| SftpError::Lost)?;

        let pending = Arc::new(Mutex::new(Pending::default()));
        let (version_tx, version_rx) = mpsc::channel();
        let for_reader = Arc::clone(&pending);
        thread::Builder::new()
            .name("sftp-reader".to_string())
            .spawn(move || read_loop(reader, for_reader, version_tx))
            .map_err(|_| SftpError::Protocol("无法启动读取线程".to_string()))?;

        let extensions = match version_rx.recv_timeout(timeout) {
            Ok(Ok(Reply::Version {
                version,
                extensions,
            })) => {
                if version < 3 {
                    return Err(SftpError::Protocol(format!("不支持的版本 {version}")));
                }
                extensions
            }
            Ok(Ok(_)) => return Err(SftpError::Protocol("首个回复不是 VERSION".to_string())),
            Ok(Err(err)) => return Err(err),
            Err(RecvTimeoutError::Timeout) => return Err(SftpError::Timeout),
            Err(RecvTimeoutError::Disconnected) => return Err(SftpError::Lost),
        };
        Ok(Arc::new(SftpClient {
            writer: Mutex::new(writer),
            pending,
            next_id: AtomicU32::new(1),
            extensions,
        }))
    }

    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions.iter().any(|(ext, _)| ext == name)
    }

    pub fn is_closed(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed
    }

    /// Register a request, then let `build` write its body with the new id.
    pub fn send(&self, build: impl FnOnce(u32) -> Packet) -> Result<Ticket, SftpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            if pending.closed {
                return Err(SftpError::Lost);
            }
            pending.waiting.insert(id, tx);
        }
        let bytes = build(id).finish();
        let written = {
            let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            writer.write_all(&bytes).and_then(|_| writer.flush())
        };
        if written.is_err() {
            close_all(&self.pending);
            return Err(SftpError::Lost);
        }
        Ok(Ticket { rx })
    }

    fn call(&self, build: impl FnOnce(u32) -> Packet) -> Result<Reply, SftpError> {
        self.send(build)?.wait(None)
    }

    pub fn realpath(&self, path: &str) -> Result<String, SftpError> {
        match self.call(|id| path_packet(proto::FXP_REALPATH, id, path))? {
            Reply::Name { entries, .. } => entries
                .into_iter()
                .next()
                .map(|entry| entry.filename)
                .ok_or_else(|| SftpError::Protocol("REALPATH 没有返回路径".to_string())),
            other => Err(unexpected(other)),
        }
    }

    pub fn stat(&self, path: &str) -> Result<Attrs, SftpError> {
        expect_attrs(self.call(|id| path_packet(proto::FXP_STAT, id, path))?)
    }

    pub fn lstat(&self, path: &str) -> Result<Attrs, SftpError> {
        expect_attrs(self.call(|id| path_packet(proto::FXP_LSTAT, id, path))?)
    }

    /// Every entry of a directory except `.` and `..`.
    pub fn read_dir(&self, path: &str) -> Result<Vec<NameEntry>, SftpError> {
        let handle = expect_handle(self.call(|id| path_packet(proto::FXP_OPENDIR, id, path))?)?;
        let mut all = Vec::new();
        let result = loop {
            match self.call(|id| handle_packet(proto::FXP_READDIR, id, &handle)) {
                Ok(Reply::Name { entries, .. }) => {
                    all.extend(
                        entries
                            .into_iter()
                            .filter(|entry| entry.filename != "." && entry.filename != ".."),
                    );
                }
                Ok(Reply::Status { code, .. }) if code == proto::FX_EOF => break Ok(all),
                Ok(other) => break Err(status_or_unexpected(other)),
                Err(err) => break Err(err),
            }
        };
        let _ = self.close(&handle);
        result
    }

    pub fn open(&self, path: &str, flags: u32, attrs: &Attrs) -> Result<Vec<u8>, SftpError> {
        expect_handle(self.call(|id| {
            let mut packet = Packet::with_id(proto::FXP_OPEN, id);
            packet.str(path).u32(flags).attrs(attrs);
            packet
        })?)
    }

    pub fn close(&self, handle: &[u8]) -> Result<(), SftpError> {
        expect_ok(self.call(|id| handle_packet(proto::FXP_CLOSE, id, handle))?)
    }

    pub fn send_read(&self, handle: &[u8], offset: u64, len: u32) -> Result<Ticket, SftpError> {
        self.send(|id| {
            let mut packet = Packet::with_id(proto::FXP_READ, id);
            packet.bytes(handle).u64(offset).u32(len);
            packet
        })
    }

    pub fn send_write(&self, handle: &[u8], offset: u64, data: &[u8]) -> Result<Ticket, SftpError> {
        self.send(|id| {
            let mut packet = Packet::with_id(proto::FXP_WRITE, id);
            packet.bytes(handle).u64(offset).bytes(data);
            packet
        })
    }

    pub fn fsetstat(&self, handle: &[u8], attrs: &Attrs) -> Result<(), SftpError> {
        expect_ok(self.call(|id| {
            let mut packet = Packet::with_id(proto::FXP_FSETSTAT, id);
            packet.bytes(handle).attrs(attrs);
            packet
        })?)
    }

    pub fn mkdir(&self, path: &str, attrs: &Attrs) -> Result<(), SftpError> {
        expect_ok(self.call(|id| {
            let mut packet = Packet::with_id(proto::FXP_MKDIR, id);
            packet.str(path).attrs(attrs);
            packet
        })?)
    }

    pub fn remove(&self, path: &str) -> Result<(), SftpError> {
        expect_ok(self.call(|id| path_packet(proto::FXP_REMOVE, id, path))?)
    }

    /// Free bytes on the file system holding `path`, when the server
    /// supports `statvfs@openssh.com`.
    pub fn available_bytes(&self, path: &str) -> Option<u64> {
        if !self.has_extension("statvfs@openssh.com") {
            return None;
        }
        let reply = self
            .call(|id| {
                let mut packet = Packet::with_id(proto::FXP_EXTENDED, id);
                packet.str("statvfs@openssh.com").str(path);
                packet
            })
            .ok()?;
        match reply {
            Reply::ExtendedReply { data, .. } => proto::statvfs_available(&data),
            _ => None,
        }
    }
}

fn path_packet(kind: u8, id: u32, path: &str) -> Packet {
    let mut packet = Packet::with_id(kind, id);
    packet.str(path);
    packet
}

fn handle_packet(kind: u8, id: u32, handle: &[u8]) -> Packet {
    let mut packet = Packet::with_id(kind, id);
    packet.bytes(handle);
    packet
}

fn unexpected(reply: Reply) -> SftpError {
    SftpError::Protocol(format!("意外的回复 {reply:?}").chars().take(120).collect())
}

fn status_or_unexpected(reply: Reply) -> SftpError {
    match reply {
        Reply::Status { code, message, .. } => SftpError::Status { code, message },
        other => unexpected(other),
    }
}

pub fn expect_ok(reply: Reply) -> Result<(), SftpError> {
    match reply {
        Reply::Status { code, .. } if code == proto::FX_OK => Ok(()),
        other => Err(status_or_unexpected(other)),
    }
}

fn expect_handle(reply: Reply) -> Result<Vec<u8>, SftpError> {
    match reply {
        Reply::Handle { handle, .. } => Ok(handle),
        other => Err(status_or_unexpected(other)),
    }
}

fn expect_attrs(reply: Reply) -> Result<Attrs, SftpError> {
    match reply {
        Reply::Attrs { attrs, .. } => Ok(attrs),
        other => Err(status_or_unexpected(other)),
    }
}

/// Data for a READ: `Some(bytes)`, or `None` at end of file.
pub fn expect_data(reply: Reply) -> Result<Option<Vec<u8>>, SftpError> {
    match reply {
        Reply::Data { data, .. } => Ok(Some(data)),
        Reply::Status { code, .. } if code == proto::FX_EOF => Ok(None),
        other => Err(status_or_unexpected(other)),
    }
}

fn close_all(pending: &Mutex<Pending>) {
    let waiting = {
        let mut guard = pending.lock().unwrap_or_else(PoisonError::into_inner);
        guard.closed = true;
        std::mem::take(&mut guard.waiting)
    };
    for (_, slot) in waiting {
        let _ = slot.send(Err(SftpError::Lost));
    }
}

fn read_packet(reader: &mut impl Read) -> Result<Vec<u8>, SftpError> {
    let mut len = [0u8; 4];
    reader.read_exact(&mut len).map_err(|_| SftpError::Lost)?;
    let len = u32::from_be_bytes(len);
    if len == 0 || len > proto::MAX_PACKET {
        return Err(SftpError::Protocol(format!("数据包长度 {len} 无效")));
    }
    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body).map_err(|_| SftpError::Lost)?;
    Ok(body)
}

fn read_loop(
    mut reader: impl Read,
    pending: Arc<Mutex<Pending>>,
    version: Sender<Result<Reply, SftpError>>,
) {
    let first = read_packet(&mut reader)
        .and_then(|body| proto::decode_reply(&body).map_err(SftpError::Protocol));
    let ok = matches!(first, Ok(Reply::Version { .. }));
    let _ = version.send(first);
    if !ok {
        close_all(&pending);
        return;
    }
    loop {
        let reply = match read_packet(&mut reader)
            .and_then(|body| proto::decode_reply(&body).map_err(SftpError::Protocol))
        {
            Ok(reply) => reply,
            Err(_) => break,
        };
        let Some(id) = reply.id() else {
            break;
        };
        let slot = pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waiting
            .remove(&id);
        if let Some(slot) = slot {
            let _ = slot.send(Ok(reply));
        }
    }
    close_all(&pending);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipe end that reads from a fixed byte string.
    struct Script(std::io::Cursor<Vec<u8>>);

    impl Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    #[test]
    fn garbage_before_version_is_a_protocol_error() {
        // A login script that prints "Welcome!\n" before sftp starts.
        let reader = Script(std::io::Cursor::new(b"Welcome!\nmore text".to_vec()));
        let err = SftpClient::start(reader, std::io::sink(), Duration::from_secs(2))
            .err()
            .unwrap();
        assert!(matches!(err, SftpError::Protocol(_)), "{err:?}");
    }

    #[test]
    fn closed_pipe_before_version_is_lost() {
        let reader = Script(std::io::Cursor::new(Vec::new()));
        let err = SftpClient::start(reader, std::io::sink(), Duration::from_secs(2))
            .err()
            .unwrap();
        assert_eq!(err, SftpError::Lost);
    }

    #[test]
    fn requests_after_the_pipe_ends_fail_as_lost() {
        let mut version = Packet::new(proto::FXP_VERSION);
        version.u32(3);
        let reader = Script(std::io::Cursor::new(version.finish()));
        let client = SftpClient::start(reader, std::io::sink(), Duration::from_secs(2)).unwrap();
        // The reader hits end of input right after VERSION.
        let err = client.realpath(".").unwrap_err();
        assert_eq!(err, SftpError::Lost);
        assert!(client.is_closed());
        assert_eq!(err.to_string(), "连接已断开");
    }

    #[test]
    fn status_messages_are_in_chinese() {
        let denied = SftpError::Status {
            code: proto::FX_PERMISSION_DENIED,
            message: "Permission denied".to_string(),
        };
        assert_eq!(denied.to_string(), "权限不足");
        let failure = SftpError::Status {
            code: proto::FX_FAILURE,
            message: "Failure".to_string(),
        };
        assert_eq!(failure.to_string(), "远端操作失败（代码 4）");
    }
}
