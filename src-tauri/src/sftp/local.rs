//! Listing for both panes. Local folders are read here; remote listings are
//! turned into the same shape.
//!
//! On Windows the empty path is the drive list ("此电脑"), and going up from
//! a drive root leads there.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::client::SftpClient;
use super::transfer::remote_join;

/// Most entries returned for one folder.
pub const MAX_ENTRIES: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    /// Full path, ready to pass back as a source or a destination.
    pub path: String,
    pub dir: bool,
    pub link: bool,
    pub size: u64,
    /// Seconds since the epoch.
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    /// `None` at the top. On Windows a drive root's parent is `""`, the drive list.
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
    /// More entries existed than [`MAX_ENTRIES`].
    pub truncated: bool,
}

/// Folders first, then names without regard to case.
pub fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

fn finish(path: String, parent: Option<String>, mut entries: Vec<Entry>) -> Listing {
    let truncated = entries.len() > MAX_ENTRIES;
    sort_entries(&mut entries);
    entries.truncate(MAX_ENTRIES);
    Listing {
        path,
        parent,
        entries,
        truncated,
    }
}

// ---------------------------------------------------------------- local

pub fn list_local(path: &str) -> Result<Listing, String> {
    if path.is_empty() {
        return list_drives();
    }
    let dir = PathBuf::from(path);
    if !dir.is_absolute() {
        return Err("本地路径必须是绝对路径".to_string());
    }
    let dir = fs::canonicalize(&dir).map_err(|err| local_list_error(&err))?;
    let dir = strip_verbatim(dir);
    let read = fs::read_dir(&dir).map_err(|err| local_list_error(&err))?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let full = entry.path();
        let link = entry
            .file_type()
            .map(|kind| kind.is_symlink())
            .unwrap_or(false);
        // Follow links so a link to a folder can be opened.
        let meta = fs::metadata(&full).or_else(|_| entry.metadata());
        let (is_dir, size, modified) = match meta {
            Ok(meta) => (
                meta.is_dir(),
                if meta.is_dir() { 0 } else { meta.len() },
                meta.modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs() as i64),
            ),
            Err(_) => (false, 0, None),
        };
        if hidden_system_entry(&name) {
            continue;
        }
        entries.push(Entry {
            name,
            path: full.to_string_lossy().into_owned(),
            dir: is_dir,
            link,
            size,
            modified,
        });
        if entries.len() > MAX_ENTRIES {
            break;
        }
    }
    let parent = local_parent(&dir);
    Ok(finish(dir.to_string_lossy().into_owned(), parent, entries))
}

/// macOS and Windows bookkeeping files nobody wants to copy.
fn hidden_system_entry(name: &str) -> bool {
    matches!(name, ".DS_Store" | "desktop.ini" | "Thumbs.db")
}

fn local_list_error(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::PermissionDenied => "没有权限打开这个文件夹".to_string(),
        std::io::ErrorKind::NotFound => "文件夹不存在".to_string(),
        _ => format!("无法打开文件夹：{err}"),
    }
}

/// `canonicalize` on Windows returns `\\?\C:\...`; show the usual form.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

fn local_parent(dir: &Path) -> Option<String> {
    match dir.parent() {
        Some(parent) => Some(parent.to_string_lossy().into_owned()),
        None if cfg!(windows) => Some(String::new()),
        None => None,
    }
}

fn list_drives() -> Result<Listing, String> {
    if !cfg!(windows) {
        return list_local("/");
    }
    let mut entries = Vec::new();
    for letter in b'A'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        if Path::new(&root).is_dir() {
            entries.push(Entry {
                name: format!("{}:", letter as char),
                path: root,
                dir: true,
                link: false,
                size: 0,
                modified: None,
            });
        }
    }
    Ok(Listing {
        path: String::new(),
        parent: None,
        entries,
        truncated: false,
    })
}

// ---------------------------------------------------------------- remote

/// Parent of a canonical remote path. `None` at `/`.
pub fn remote_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(index) => Some(trimmed[..index].to_string()),
        None => None,
    }
}

pub fn list_remote(client: &SftpClient, path: &str) -> Result<Listing, String> {
    let path = client.realpath(path).map_err(|err| err.to_string())?;
    let raw = client.read_dir(&path).map_err(|err| err.to_string())?;
    let mut entries = Vec::with_capacity(raw.len().min(MAX_ENTRIES + 1));
    for item in raw.into_iter().take(MAX_ENTRIES + 1) {
        let full = remote_join(&path, &item.filename);
        let link = item.attrs.is_link();
        let attrs = if link {
            client.stat(&full).unwrap_or(item.attrs)
        } else {
            item.attrs
        };
        entries.push(Entry {
            name: item.filename,
            path: full,
            dir: attrs.is_dir(),
            link,
            size: if attrs.is_dir() {
                0
            } else {
                attrs.size.unwrap_or(0)
            },
            modified: attrs.mtime().map(i64::from),
        });
    }
    let parent = remote_parent(&path);
    Ok(finish(path, parent, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, dir: bool) -> Entry {
        Entry {
            name: name.to_string(),
            path: format!("/{name}"),
            dir,
            link: false,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn folders_come_first_then_names_ignoring_case() {
        let mut entries = vec![
            entry("b.txt", false),
            entry("Zeta", true),
            entry("A.txt", false),
            entry("alpha", true),
        ];
        sort_entries(&mut entries);
        let names: Vec<_> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zeta", "A.txt", "b.txt"]);
    }

    #[test]
    fn remote_parent_walks_up_to_root() {
        assert_eq!(remote_parent("/home/me"), Some("/home".to_string()));
        assert_eq!(remote_parent("/home"), Some("/".to_string()));
        assert_eq!(remote_parent("/home/me/"), Some("/home".to_string()));
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_parent("/C:/Users"), Some("/C:".to_string()));
    }

    #[test]
    fn verbatim_prefixes_are_removed() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\Users\me")),
            PathBuf::from(r"C:\Users\me")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(strip_verbatim(PathBuf::from("/tmp")), PathBuf::from("/tmp"));
    }

    #[test]
    fn local_listing_has_full_paths_and_a_parent() {
        let dir = std::env::temp_dir().join(format!("mt-local-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("file.bin"), b"12345").unwrap();
        fs::write(dir.join(".DS_Store"), b"x").unwrap();
        let listing = list_local(dir.to_str().unwrap()).unwrap();
        let names: Vec<_> = listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect();
        assert_eq!(names, ["sub", "file.bin"]);
        assert!(listing.entries[0].dir);
        assert_eq!(listing.entries[1].size, 5);
        assert!(Path::new(&listing.entries[1].path).is_file());
        assert!(listing.parent.is_some());
        assert!(list_local("relative/path").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
