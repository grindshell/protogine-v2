//! The game's read-only filesystem, mounted before any Lua runs.
//!
//! Paths are relative to the game root, `/`-separated, and case-sensitive on every platform,
//! so a game that works natively also works from a zip on the web.

use std::{
    collections::HashMap,
    fmt, fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

pub enum Vfs {
    /// A game directory on disk (native only).
    Dir(PathBuf),
    /// A game archive unpacked into memory, keyed by normalized path.
    Memory(HashMap<String, Vec<u8>>),
}

#[derive(Debug)]
pub enum VfsError {
    NotFound { path: String, hint: Option<String> },
    InvalidPath(String),
    Io(String),
    Archive(String),
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VfsError::NotFound { path, hint: None } => write!(f, "file not found: '{path}'"),
            VfsError::NotFound {
                path,
                hint: Some(actual),
            } => write!(
                f,
                "file not found: '{path}' (found '{actual}'; paths are case-sensitive)"
            ),
            VfsError::InvalidPath(path) => write!(f, "invalid path: '{path}'"),
            VfsError::Io(message) | VfsError::Archive(message) => f.write_str(message),
        }
    }
}

impl Vfs {
    /// Mounts a game directory or `.zip` from a native path.
    pub fn mount_path(path: &Path) -> Result<Vfs, VfsError> {
        if path.is_dir() {
            Ok(Vfs::Dir(path.to_path_buf()))
        } else {
            let bytes =
                fs::read(path).map_err(|e| VfsError::Io(format!("{}: {e}", path.display())))?;
            Vfs::mount_zip(&bytes)
        }
    }

    /// Unpacks a zip archive into memory.
    ///
    /// Games are expected at the archive root, like `.love` files. Archives made by zipping the
    /// game's folder itself (so everything sits under one top-level directory) are accepted too.
    pub fn mount_zip(bytes: &[u8]) -> Result<Vfs, VfsError> {
        let archive_error =
            |e: zip::result::ZipError| VfsError::Archive(format!("invalid game archive: {e}"));
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;

        let mut files = HashMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(archive_error)?;
            if entry.is_dir() {
                continue;
            }
            // Some Windows tools write `\` separators, against the zip spec.
            let Some(path) = normalize(&entry.name().replace('\\', "/")) else {
                continue;
            };
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut data)
                .map_err(|e| VfsError::Archive(format!("invalid game archive: {path}: {e}")))?;
            files.insert(path, data);
        }

        if !files.contains_key("main.lua")
            && let Some(prefix) = single_top_level_dir(&files)
        {
            files = files
                .into_iter()
                .map(|(path, data)| (path[prefix.len() + 1..].to_string(), data))
                .collect();
        }
        Ok(Vfs::Memory(files))
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let normalized = normalize(path).ok_or_else(|| VfsError::InvalidPath(path.to_string()))?;
        match self {
            Vfs::Dir(root) => read_exact_case(root, &normalized),
            Vfs::Memory(files) => files.get(&normalized).cloned().ok_or_else(|| {
                let hint = files
                    .keys()
                    .find(|candidate| candidate.eq_ignore_ascii_case(&normalized))
                    .cloned();
                VfsError::NotFound {
                    path: normalized,
                    hint,
                }
            }),
        }
    }

    pub fn read_string(&self, path: &str) -> Result<String, VfsError> {
        String::from_utf8(self.read(path)?)
            .map_err(|_| VfsError::Io(format!("'{path}' is not valid UTF-8")))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.read(path).is_ok()
    }
}

/// Normalizes a game path: strips empty and `.` segments, and rejects `..`, backslashes and
/// drive prefixes so paths can't escape the game root.
fn normalize(path: &str) -> Option<String> {
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            s if s.contains(['\\', ':']) => return None,
            s => segments.push(s),
        }
    }
    (!segments.is_empty()).then(|| segments.join("/"))
}

/// Reads `root/path`, requiring every component to match the on-disk name exactly, even on
/// case-insensitive filesystems.
fn read_exact_case(root: &Path, path: &str) -> Result<Vec<u8>, VfsError> {
    let mut current = root.to_path_buf();
    let mut actual = Vec::new();
    let mut case_mismatch = false;

    for segment in path.split('/') {
        let entries = fs::read_dir(&current).map_err(|_| not_found(path, None))?;
        let mut folded_match = None;
        let mut exact = false;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == segment {
                exact = true;
                break;
            }
            if folded_match.is_none() && name.eq_ignore_ascii_case(segment) {
                folded_match = Some(name.to_string());
            }
        }
        let name = if exact {
            segment.to_string()
        } else if let Some(name) = folded_match {
            case_mismatch = true;
            name
        } else {
            return Err(not_found(path, None));
        };
        current.push(&name);
        actual.push(name);
    }

    if case_mismatch {
        return Err(not_found(path, Some(actual.join("/"))));
    }
    fs::read(&current).map_err(|e| VfsError::Io(format!("{path}: {e}")))
}

fn not_found(path: &str, hint: Option<String>) -> VfsError {
    VfsError::NotFound {
        path: path.to_string(),
        hint,
    }
}

/// Returns the directory every file lives under, if they all share exactly one.
fn single_top_level_dir(files: &HashMap<String, Vec<u8>>) -> Option<String> {
    let mut prefix: Option<&str> = None;
    for path in files.keys() {
        let (top, _) = path.split_once('/')?;
        match prefix {
            None => prefix = Some(top),
            Some(p) if p == top => {}
            Some(_) => return None,
        }
    }
    let prefix = prefix?.to_string();
    files
        .contains_key(&format!("{prefix}/main.lua"))
        .then_some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in entries {
            writer
                .start_file(*path, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize("a/./b//c.lua").as_deref(), Some("a/b/c.lua"));
        assert_eq!(normalize("/main.lua").as_deref(), Some("main.lua"));
        assert_eq!(normalize("../secret"), None);
        assert_eq!(normalize("a\\b"), None);
        assert_eq!(normalize("C:/x"), None);
        assert_eq!(normalize(""), None);
    }

    #[test]
    fn reads_zip_case_sensitively() {
        let vfs = Vfs::mount_zip(&zip_of(&[("main.lua", "x"), ("gfx/Player.png", "p")])).unwrap();
        assert_eq!(vfs.read("gfx/Player.png").unwrap(), b"p");
        match vfs.read("gfx/player.png") {
            Err(VfsError::NotFound {
                hint: Some(hint), ..
            }) => assert_eq!(hint, "gfx/Player.png"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn strips_single_top_level_dir() {
        let vfs =
            Vfs::mount_zip(&zip_of(&[("game/main.lua", "x"), ("game/a/b.lua", "y")])).unwrap();
        assert_eq!(vfs.read("main.lua").unwrap(), b"x");
        assert_eq!(vfs.read("a/b.lua").unwrap(), b"y");
    }

    #[test]
    fn reads_dir_case_sensitively() {
        let root = std::env::temp_dir().join(format!("protogine-vfs-test-{}", std::process::id()));
        fs::create_dir_all(root.join("gfx")).unwrap();
        fs::write(root.join("gfx/Player.png"), "p").unwrap();

        let vfs = Vfs::Dir(root.clone());
        assert_eq!(vfs.read("gfx/Player.png").unwrap(), b"p");
        assert!(matches!(
            vfs.read("gfx/player.png"),
            Err(VfsError::NotFound { hint: Some(_), .. })
        ));
        assert!(matches!(
            vfs.read("nope.lua"),
            Err(VfsError::NotFound { hint: None, .. })
        ));

        fs::remove_dir_all(root).unwrap();
    }
}
