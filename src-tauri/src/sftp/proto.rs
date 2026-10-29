//! SFTP version 3 packets (draft-ietf-secsh-filexfer-02), the version OpenSSH
//! speaks. Encoding and decoding only; no IO.
//!
//! Every packet is `uint32 length`, `byte type`, then the body. All requests
//! after `INIT` carry a `uint32` request id right after the type.

pub const FXP_INIT: u8 = 1;
pub const FXP_VERSION: u8 = 2;
pub const FXP_OPEN: u8 = 3;
pub const FXP_CLOSE: u8 = 4;
pub const FXP_READ: u8 = 5;
pub const FXP_WRITE: u8 = 6;
pub const FXP_LSTAT: u8 = 7;
pub const FXP_FSETSTAT: u8 = 10;
pub const FXP_OPENDIR: u8 = 11;
pub const FXP_READDIR: u8 = 12;
pub const FXP_REMOVE: u8 = 13;
pub const FXP_MKDIR: u8 = 14;
pub const FXP_REALPATH: u8 = 16;
pub const FXP_STAT: u8 = 17;
pub const FXP_STATUS: u8 = 101;
pub const FXP_HANDLE: u8 = 102;
pub const FXP_DATA: u8 = 103;
pub const FXP_NAME: u8 = 104;
pub const FXP_ATTRS: u8 = 105;
pub const FXP_EXTENDED: u8 = 200;
pub const FXP_EXTENDED_REPLY: u8 = 201;

pub const FX_OK: u32 = 0;
pub const FX_EOF: u32 = 1;
pub const FX_NO_SUCH_FILE: u32 = 2;
pub const FX_PERMISSION_DENIED: u32 = 3;
pub const FX_FAILURE: u32 = 4;
pub const FX_NO_CONNECTION: u32 = 6;
pub const FX_CONNECTION_LOST: u32 = 7;

pub const ATTR_SIZE: u32 = 0x0000_0001;
pub const ATTR_UIDGID: u32 = 0x0000_0002;
pub const ATTR_PERMISSIONS: u32 = 0x0000_0004;
pub const ATTR_ACMODTIME: u32 = 0x0000_0008;
pub const ATTR_EXTENDED: u32 = 0x8000_0000;

pub const OPEN_READ: u32 = 0x0000_0001;
pub const OPEN_WRITE: u32 = 0x0000_0002;
pub const OPEN_CREAT: u32 = 0x0000_0008;
pub const OPEN_TRUNC: u32 = 0x0000_0010;

/// `st_mode` file type bits.
pub const S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const S_IFREG: u32 = 0o100000;
pub const S_IFLNK: u32 = 0o120000;

/// Largest packet accepted from the server. OpenSSH sends at most 256 KiB of
/// data plus a header; anything far larger is not SFTP (for example a remote
/// login script printing text before the subsystem starts).
pub const MAX_PACKET: u32 = 512 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs {
    pub size: Option<u64>,
    pub uid_gid: Option<(u32, u32)>,
    pub permissions: Option<u32>,
    /// (atime, mtime) in seconds since the epoch.
    pub times: Option<(u32, u32)>,
}

impl Attrs {
    pub fn is_dir(&self) -> bool {
        self.permissions
            .map(|mode| mode & S_IFMT == S_IFDIR)
            .unwrap_or(false)
    }

    pub fn is_link(&self) -> bool {
        self.permissions
            .map(|mode| mode & S_IFMT == S_IFLNK)
            .unwrap_or(false)
    }

    /// A regular file, or a server that does not send a file type.
    pub fn is_file(&self) -> bool {
        match self.permissions {
            Some(mode) => mode & S_IFMT == S_IFREG || mode & S_IFMT == 0,
            None => true,
        }
    }

    pub fn mtime(&self) -> Option<u32> {
        self.times.map(|(_, mtime)| mtime)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameEntry {
    pub filename: String,
    pub attrs: Attrs,
}

/// A decoded server reply. `Version` is only valid as the first packet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Version {
        version: u32,
        extensions: Vec<(String, String)>,
    },
    Status {
        id: u32,
        code: u32,
        message: String,
    },
    Handle {
        id: u32,
        handle: Vec<u8>,
    },
    Data {
        id: u32,
        data: Vec<u8>,
    },
    Name {
        id: u32,
        entries: Vec<NameEntry>,
    },
    Attrs {
        id: u32,
        attrs: Attrs,
    },
    ExtendedReply {
        id: u32,
        data: Vec<u8>,
    },
}

impl Reply {
    pub fn id(&self) -> Option<u32> {
        match self {
            Reply::Version { .. } => None,
            Reply::Status { id, .. }
            | Reply::Handle { id, .. }
            | Reply::Data { id, .. }
            | Reply::Name { id, .. }
            | Reply::Attrs { id, .. }
            | Reply::ExtendedReply { id, .. } => Some(*id),
        }
    }
}

/// Builds one request body. `finish` prepends the length.
pub struct Packet {
    buf: Vec<u8>,
}

impl Packet {
    pub fn new(kind: u8) -> Self {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.push(kind);
        Self { buf }
    }

    pub fn with_id(kind: u8, id: u32) -> Self {
        let mut packet = Self::new(kind);
        packet.u32(id);
        packet
    }

    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    pub fn bytes(&mut self, value: &[u8]) -> &mut Self {
        self.u32(value.len() as u32);
        self.buf.extend_from_slice(value);
        self
    }

    pub fn str(&mut self, value: &str) -> &mut Self {
        self.bytes(value.as_bytes())
    }

    pub fn attrs(&mut self, attrs: &Attrs) -> &mut Self {
        let mut flags = 0;
        if attrs.size.is_some() {
            flags |= ATTR_SIZE;
        }
        if attrs.uid_gid.is_some() {
            flags |= ATTR_UIDGID;
        }
        if attrs.permissions.is_some() {
            flags |= ATTR_PERMISSIONS;
        }
        if attrs.times.is_some() {
            flags |= ATTR_ACMODTIME;
        }
        self.u32(flags);
        if let Some(size) = attrs.size {
            self.u64(size);
        }
        if let Some((uid, gid)) = attrs.uid_gid {
            self.u32(uid).u32(gid);
        }
        if let Some(mode) = attrs.permissions {
            self.u32(mode);
        }
        if let Some((atime, mtime)) = attrs.times {
            self.u32(atime).u32(mtime);
        }
        self
    }

    pub fn finish(mut self) -> Vec<u8> {
        let len = (self.buf.len() - 4) as u32;
        self.buf[..4].copy_from_slice(&len.to_be_bytes());
        self.buf
    }
}

/// Reads fields from one packet body.
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| "SFTP 数据包不完整".to_string())?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    pub fn u32(&mut self) -> Result<u32, String> {
        let bytes = self.take(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn u64(&mut self) -> Result<u64, String> {
        let bytes = self.take(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(array))
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], String> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    pub fn string(&mut self) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    pub fn attrs(&mut self) -> Result<Attrs, String> {
        let flags = self.u32()?;
        let mut attrs = Attrs::default();
        if flags & ATTR_SIZE != 0 {
            attrs.size = Some(self.u64()?);
        }
        if flags & ATTR_UIDGID != 0 {
            attrs.uid_gid = Some((self.u32()?, self.u32()?));
        }
        if flags & ATTR_PERMISSIONS != 0 {
            attrs.permissions = Some(self.u32()?);
        }
        if flags & ATTR_ACMODTIME != 0 {
            attrs.times = Some((self.u32()?, self.u32()?));
        }
        if flags & ATTR_EXTENDED != 0 {
            let count = self.u32()?;
            for _ in 0..count {
                self.bytes()?;
                self.bytes()?;
            }
        }
        Ok(attrs)
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }
}

/// Decode one packet body (everything after the length).
pub fn decode_reply(body: &[u8]) -> Result<Reply, String> {
    let mut cursor = Cursor::new(body);
    let kind = cursor.u8()?;
    if kind == FXP_VERSION {
        let version = cursor.u32()?;
        let mut extensions = Vec::new();
        while !cursor.is_empty() {
            let name = cursor.string()?;
            let data = cursor.string()?;
            extensions.push((name, data));
        }
        return Ok(Reply::Version {
            version,
            extensions,
        });
    }
    let id = cursor.u32()?;
    match kind {
        FXP_STATUS => {
            let code = cursor.u32()?;
            // Version 3 servers may leave out the message and language tag.
            let message = if cursor.is_empty() {
                String::new()
            } else {
                cursor.string()?
            };
            Ok(Reply::Status { id, code, message })
        }
        FXP_HANDLE => Ok(Reply::Handle {
            id,
            handle: cursor.bytes()?.to_vec(),
        }),
        FXP_DATA => Ok(Reply::Data {
            id,
            data: cursor.bytes()?.to_vec(),
        }),
        FXP_NAME => {
            let count = cursor.u32()?;
            // Each entry takes at least 12 bytes; refuse counts the body cannot hold.
            if count as usize > body.len() / 12 + 1 {
                return Err("SFTP 数据包不完整".to_string());
            }
            let mut entries = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let filename = cursor.string()?;
                let _longname = cursor.bytes()?;
                let attrs = cursor.attrs()?;
                entries.push(NameEntry { filename, attrs });
            }
            Ok(Reply::Name { id, entries })
        }
        FXP_ATTRS => Ok(Reply::Attrs {
            id,
            attrs: cursor.attrs()?,
        }),
        FXP_EXTENDED_REPLY => Ok(Reply::ExtendedReply {
            id,
            data: body[5..].to_vec(),
        }),
        other => Err(format!("未知的 SFTP 回复类型 {other}")),
    }
}

/// `statvfs@openssh.com` reply: free bytes for an unprivileged user.
pub fn statvfs_available(data: &[u8]) -> Option<u64> {
    let mut cursor = Cursor::new(data);
    let _bsize = cursor.u64().ok()?;
    let frsize = cursor.u64().ok()?;
    let _blocks = cursor.u64().ok()?;
    let _bfree = cursor.u64().ok()?;
    let bavail = cursor.u64().ok()?;
    Some(bavail.saturating_mul(frsize))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(packet: Vec<u8>) -> Vec<u8> {
        let len = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]) as usize;
        assert_eq!(len, packet.len() - 4);
        packet[4..].to_vec()
    }

    #[test]
    fn open_request_has_length_type_id_path_flags_and_attrs() {
        let mut packet = Packet::with_id(FXP_OPEN, 7);
        packet
            .str("/tmp/a")
            .u32(OPEN_WRITE | OPEN_CREAT | OPEN_TRUNC)
            .attrs(&Attrs {
                permissions: Some(0o644),
                ..Attrs::default()
            });
        let bytes = packet.finish();
        assert_eq!(
            bytes,
            [
                &[0, 0, 0, 27][..],
                &[FXP_OPEN],
                &[0, 0, 0, 7],
                &[0, 0, 0, 6],
                b"/tmp/a",
                &[0, 0, 0, 0x1a],
                &[0, 0, 0, 4],
                &[0, 0, 0x01, 0xa4],
            ]
            .concat()
        );
    }

    #[test]
    fn version_reply_lists_extensions() {
        let mut packet = Packet::new(FXP_VERSION);
        packet.u32(3).str("statvfs@openssh.com").str("2");
        let reply = decode_reply(&body(packet.finish())).unwrap();
        assert_eq!(
            reply,
            Reply::Version {
                version: 3,
                extensions: vec![("statvfs@openssh.com".to_string(), "2".to_string())],
            }
        );
        assert_eq!(reply.id(), None);
    }

    #[test]
    fn status_without_message_is_accepted() {
        let mut packet = Packet::with_id(FXP_STATUS, 9);
        packet.u32(FX_EOF);
        let reply = decode_reply(&body(packet.finish())).unwrap();
        assert_eq!(
            reply,
            Reply::Status {
                id: 9,
                code: FX_EOF,
                message: String::new()
            }
        );
    }

    #[test]
    fn name_reply_reads_every_attribute_kind() {
        let mut packet = Packet::with_id(FXP_NAME, 3);
        packet.u32(2);
        packet.str("docs").str("drwxr-xr-x docs").attrs(&Attrs {
            size: Some(4096),
            uid_gid: Some((1000, 1000)),
            permissions: Some(S_IFDIR | 0o755),
            times: Some((1, 1_700_000_000)),
        });
        packet.str("link").str("lrwx link").attrs(&Attrs {
            permissions: Some(S_IFLNK | 0o777),
            ..Attrs::default()
        });
        let reply = decode_reply(&body(packet.finish())).unwrap();
        let Reply::Name { id, entries } = reply else {
            panic!("not a name reply");
        };
        assert_eq!(id, 3);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "docs");
        assert!(entries[0].attrs.is_dir());
        assert_eq!(entries[0].attrs.size, Some(4096));
        assert_eq!(entries[0].attrs.mtime(), Some(1_700_000_000));
        assert!(entries[1].attrs.is_link());
        assert!(!entries[1].attrs.is_file());
    }

    #[test]
    fn extended_attribute_pairs_are_skipped() {
        let mut packet = Packet::with_id(FXP_ATTRS, 1);
        packet
            .u32(ATTR_SIZE | ATTR_EXTENDED)
            .u64(5)
            .u32(1)
            .str("k")
            .str("v");
        let reply = decode_reply(&body(packet.finish())).unwrap();
        assert_eq!(
            reply,
            Reply::Attrs {
                id: 1,
                attrs: Attrs {
                    size: Some(5),
                    ..Attrs::default()
                }
            }
        );
    }

    #[test]
    fn truncated_and_unknown_packets_are_errors() {
        assert!(decode_reply(&[FXP_HANDLE, 0, 0, 0, 1, 0, 0, 0, 9, 1]).is_err());
        assert!(decode_reply(&[]).is_err());
        assert!(decode_reply(&[99, 0, 0, 0, 1]).is_err());
        // A name count the body cannot hold is refused before allocating.
        assert!(decode_reply(&[FXP_NAME, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn statvfs_reply_gives_available_bytes() {
        let mut packet = Packet::new(0);
        packet.u64(4096).u64(4096).u64(100).u64(50).u64(10);
        let data = body(packet.finish())[1..].to_vec();
        assert_eq!(statvfs_available(&data), Some(40960));
        assert_eq!(statvfs_available(&[0; 8]), None);
    }
}
