use std::io::{self, Read};

/// Copy one PTY read through unchanged.
///
/// Incomplete UTF-8 sequences are kept byte for byte so the terminal decoder
/// can finish them on a later read. This does not decode or replace text.
pub fn forward_pty_bytes(chunk: &[u8]) -> Vec<u8> {
    chunk.to_vec()
}

/// Read one chunk from the PTY.
///
/// `Ok(None)` is end of output. A short read, including one that ends inside
/// a multibyte UTF-8 sequence, is returned whole.
pub fn read_chunk<R: Read>(reader: &mut R, buf: &mut [u8]) -> io::Result<Option<Vec<u8>>> {
    if buf.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty buffer"));
    }
    loop {
        match reader.read(buf) {
            Ok(0) => return Ok(None),
            Ok(n) => return Ok(Some(forward_pty_bytes(&buf[..n]))),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct ByteAtATime<'a> {
        data: &'a [u8],
        index: usize,
        interrupt_once: bool,
    }

    impl Read for ByteAtATime<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.interrupt_once {
                self.interrupt_once = false;
                return Err(io::Error::new(io::ErrorKind::Interrupted, "retry"));
            }
            if self.index >= self.data.len() || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.data[self.index];
            self.index += 1;
            Ok(1)
        }
    }

    #[test]
    fn partial_utf8_is_not_replaced_or_dropped() {
        let partial = forward_pty_bytes(&[0xE4, 0xBD]);
        assert_eq!(partial, vec![0xE4, 0xBD]);
        assert!(std::str::from_utf8(&partial).is_err());
        assert_ne!(partial, "\u{FFFD}".as_bytes());
    }

    #[test]
    fn split_multibyte_utf8_is_forwarded_intact() {
        let text = "a你b";
        let original = text.as_bytes();
        let mut reader = ByteAtATime {
            data: original,
            index: 0,
            interrupt_once: true,
        };
        let mut buf = [0u8; 8];
        let mut forwarded = Vec::new();
        let mut saw_invalid = false;
        loop {
            match read_chunk(&mut reader, &mut buf).unwrap() {
                Some(chunk) => {
                    if std::str::from_utf8(&chunk).is_err() {
                        saw_invalid = true;
                    }
                    forwarded.extend_from_slice(&chunk);
                }
                None => break,
            }
        }
        assert!(
            saw_invalid,
            "a split multibyte sequence must pass through undecoded"
        );
        assert_eq!(forwarded, original);
        assert_eq!(std::str::from_utf8(&forwarded).unwrap(), text);
    }

    #[test]
    fn empty_read_is_eof() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut buf = [0u8; 4];
        assert!(read_chunk(&mut reader, &mut buf).unwrap().is_none());
    }
}
