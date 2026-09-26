//! The game's read-only filesystem, mounted before any Lua runs.
//!
//! Paths are relative to the game root, `/`-separated, and case-sensitive on every platform,
//! so a game that works natively also works from a zip on the web.

use std::{
    collections::{BTreeSet, HashMap},
    fmt, fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub struct Vfs {
    /// The game's name, from its directory or archive. It's the default save identity.
    pub name: String,
    /// Where the game was mounted from, as `pg.filesystem.getSource` reports it.
    pub source: String,
    files: Files,
}

enum Files {
    /// A game directory on disk (native only).
    Dir(PathBuf),
    /// A game archive unpacked into memory, keyed by normalized path.
    Memory(HashMap<String, Vec<u8>>),
}

/// The name of a game whose directory or archive name can't be used.
const DEFAULT_NAME: &str = "game";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
}

/// What `pg.filesystem.getInfo` reports about a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub kind: Kind,
    /// In bytes, for files.
    pub size: Option<u64>,
    /// Seconds since the Unix epoch, where known.
    pub modtime: Option<i64>,
}

impl Info {
    pub fn directory() -> Info {
        Info {
            kind: Kind::Directory,
            size: None,
            modtime: None,
        }
    }
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
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let source = absolute.display().to_string();
        if path.is_dir() {
            let name = absolute.file_name().and_then(|n| n.to_str());
            Ok(Vfs {
                name: game_name(name),
                source,
                files: Files::Dir(absolute),
            })
        } else {
            let bytes =
                fs::read(path).map_err(|e| VfsError::Io(format!("{}: {e}", path.display())))?;
            let name = absolute.file_stem().and_then(|n| n.to_str());
            Vfs::mount_zip(&bytes, name, source)
        }
    }

    /// Unpacks a zip archive into memory. Without a `name` (the archive's file name, minus its
    /// extension), the game is named after the archive's top-level directory, if it has one.
    ///
    /// Games are expected at the archive root, like `.love` files. Archives made by zipping the
    /// game's folder itself (so everything sits under one top-level directory) are accepted too.
    pub fn mount_zip(
        bytes: &[u8],
        name: Option<&str>,
        source: impl Into<String>,
    ) -> Result<Vfs, VfsError> {
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

        let mut top = None;
        if !files.contains_key("main.lua")
            && let Some(prefix) = single_top_level_dir(&files)
        {
            files = files
                .into_iter()
                .map(|(path, data)| (path[prefix.len() + 1..].to_string(), data))
                .collect();
            top = Some(prefix);
        }
        Ok(Vfs {
            name: game_name(name.or(top.as_deref())),
            source: source.into(),
            files: Files::Memory(files),
        })
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let normalized = normalize(path).ok_or_else(|| VfsError::InvalidPath(path.to_string()))?;
        match &self.files {
            Files::Dir(root) => read_dir_file(root, &normalized),
            Files::Memory(files) => files.get(&normalized).cloned().ok_or_else(|| {
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

    /// What's at a normalized path, or `""` for the root.
    pub fn info(&self, path: &str) -> Option<Info> {
        if path.is_empty() {
            return Some(Info::directory());
        }
        match &self.files {
            Files::Dir(root) => dir_info(root, path),
            Files::Memory(files) => match files.get(path) {
                Some(data) => Some(Info {
                    kind: Kind::File,
                    size: Some(data.len() as u64),
                    modtime: None,
                }),
                None => {
                    let prefix = format!("{path}/");
                    let is_dir = files.keys().any(|p| p.starts_with(&prefix));
                    is_dir.then(Info::directory)
                }
            },
        }
    }

    /// The names in a normalized directory path, or `""` for the root.
    pub fn list(&self, dir: &str) -> BTreeSet<String> {
        match &self.files {
            Files::Dir(root) => dir_list(root, dir),
            Files::Memory(files) => {
                let prefix = if dir.is_empty() {
                    String::new()
                } else {
                    format!("{dir}/")
                };
                files
                    .keys()
                    .filter_map(|path| path.strip_prefix(&prefix))
                    .filter_map(|rest| rest.split('/').next())
                    .map(str::to_string)
                    .collect()
            }
        }
    }
}

/// The game's name if it can be a save identity, or a default.
fn game_name(name: Option<&str>) -> String {
    name.filter(|n| is_segment(n))
        .unwrap_or(DEFAULT_NAME)
        .to_string()
}

/// Whether `name` is a single valid path segment, like a save identity.
pub fn is_segment(name: &str) -> bool {
    normalize(name).is_some_and(|n| n == name && !n.contains('/'))
}

/// Normalizes a game path: strips empty and `.` segments, and rejects `..`, backslashes and
/// drive prefixes so paths can't escape the game root.
pub fn normalize(path: &str) -> Option<String> {
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

/// Like [`normalize`], but for a directory: the root (`""`, `"/"` or `"."`) is `""`.
pub fn normalize_dir(path: &str) -> Option<String> {
    if path.split('/').all(|s| s.is_empty() || s == ".") {
        return Some(String::new());
    }
    normalize(path)
}

/// Finds `root/path` on disk, requiring every component to match the on-disk name exactly,
/// even on case-insensitive filesystems.
fn resolve(root: &Path, path: &str) -> Result<PathBuf, VfsError> {
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
    Ok(current)
}

/// Reads the file at a normalized path under `root`, with exact case.
pub fn read_dir_file(root: &Path, path: &str) -> Result<Vec<u8>, VfsError> {
    let full = resolve(root, path)?;
    fs::read(&full).map_err(|e| VfsError::Io(format!("{path}: {e}")))
}

/// What's at a normalized path under `root`, with exact case. `""` is `root` itself.
pub fn dir_info(root: &Path, path: &str) -> Option<Info> {
    let full = if path.is_empty() {
        root.to_path_buf()
    } else {
        resolve(root, path).ok()?
    };
    let metadata = fs::metadata(full).ok()?;
    let modtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Some(if metadata.is_dir() {
        Info {
            modtime,
            ..Info::directory()
        }
    } else {
        Info {
            kind: Kind::File,
            size: Some(metadata.len()),
            modtime,
        }
    })
}

/// The names in a normalized directory path under `root`, with exact case.
pub fn dir_list(root: &Path, dir: &str) -> BTreeSet<String> {
    let full = if dir.is_empty() {
        Ok(root.to_path_buf())
    } else {
        resolve(root, dir)
    };
    let Ok(entries) = full.and_then(|full| fs::read_dir(full).map_err(|_| not_found(dir, None)))
    else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect()
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

    fn mount(entries: &[(&str, &str)]) -> Vfs {
        Vfs::mount_zip(&zip_of(entries), None, "test.zip").unwrap()
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize("a/./b//c.lua").as_deref(), Some("a/b/c.lua"));
        assert_eq!(normalize("/main.lua").as_deref(), Some("main.lua"));
        assert_eq!(normalize("../secret"), None);
        assert_eq!(normalize("a\\b"), None);
        assert_eq!(normalize("C:/x"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize_dir("/./").as_deref(), Some(""));
        assert!(
            is_segment("my game") && !is_segment("a/b") && !is_segment("..") && !is_segment("")
        );
    }

    #[test]
    fn reads_zip_case_sensitively() {
        let vfs = mount(&[("main.lua", "x"), ("gfx/Player.png", "p")]);
        assert_eq!(vfs.read("gfx/Player.png").unwrap(), b"p");
        match vfs.read("gfx/player.png") {
            Err(VfsError::NotFound {
                hint: Some(hint), ..
            }) => assert_eq!(hint, "gfx/Player.png"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn names_games() {
        assert_eq!(mount(&[("main.lua", "x")]).name, "game");
        let nested = mount(&[("demo/main.lua", "x"), ("demo/a/b.lua", "y")]);
        assert_eq!(nested.name, "demo");
        assert_eq!(nested.read("main.lua").unwrap(), b"x");
        assert_eq!(nested.read("a/b.lua").unwrap(), b"y");
        let named = Vfs::mount_zip(&zip_of(&[("demo/main.lua", "x")]), Some("pong"), "x").unwrap();
        assert_eq!(named.name, "pong");
    }

    #[test]
    fn lists_zip_contents() {
        let vfs = mount(&[
            ("main.lua", "x"),
            ("gfx/a.png", "a"),
            ("gfx/ui/b.png", "bb"),
        ]);
        assert_eq!(
            vfs.list(""),
            BTreeSet::from(["gfx".into(), "main.lua".into()])
        );
        assert_eq!(
            vfs.list("gfx"),
            BTreeSet::from(["a.png".into(), "ui".into()])
        );
        assert_eq!(vfs.info("gfx").unwrap().kind, Kind::Directory);
        assert_eq!(vfs.info("gfx/ui/b.png").unwrap().size, Some(2));
        assert_eq!(vfs.info("gf"), None);
    }

    #[test]
    fn reads_dir_case_sensitively() {
        let root = std::env::temp_dir().join(format!("protogine-vfs-test-{}", std::process::id()));
        fs::create_dir_all(root.join("gfx")).unwrap();
        fs::write(root.join("gfx/Player.png"), "p").unwrap();

        let vfs = Vfs::mount_path(&root).unwrap();
        assert!(vfs.name.starts_with("protogine-vfs-test-"));
        assert_eq!(vfs.read("gfx/Player.png").unwrap(), b"p");
        assert!(matches!(
            vfs.read("gfx/player.png"),
            Err(VfsError::NotFound { hint: Some(_), .. })
        ));
        assert!(matches!(
            vfs.read("nope.lua"),
            Err(VfsError::NotFound { hint: None, .. })
        ));
        assert_eq!(vfs.info("gfx/Player.png").unwrap().size, Some(1));
        assert_eq!(vfs.info("gfx/player.png"), None);
        assert_eq!(vfs.list("gfx"), BTreeSet::from(["Player.png".into()]));

        fs::remove_dir_all(root).unwrap();
    }
}
