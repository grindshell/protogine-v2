//! The engine side of `pg.filesystem`: the game's read-only files, layered with a writable save
//! directory. Natively, save directories are directories on disk. On the web they're entries in
//! IndexedDB, which `web/index.html` loads into memory before the game starts, so every call is
//! still synchronous. It has no Lua dependency.
//!
//! Reads look in the save directory first, then in the game, like Love2D. `t.appendidentity`
//! swaps the order.

use std::collections::BTreeSet;

use crate::vfs::{self, Info, Kind, Vfs, VfsError};

pub struct Filesystem {
    game: Vfs,
    saves: Box<dyn Saves>,
    identity: String,
    /// Whether reads look in the game before the save directory.
    append: bool,
}

#[derive(Clone, Copy)]
enum Layer {
    Save,
    Game,
}

impl Filesystem {
    /// The game's files, with the platform's save directories. The identity starts as the
    /// game's name.
    pub fn new(game: Vfs) -> Filesystem {
        Filesystem::with_saves(game, platform_saves())
    }

    fn with_saves(game: Vfs, saves: Box<dyn Saves>) -> Filesystem {
        Filesystem {
            identity: game.name.clone(),
            game,
            saves,
            append: false,
        }
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Switches to another save directory. `identity` must be a single path segment.
    pub fn set_identity(&mut self, identity: &str, append: bool) -> Result<(), String> {
        if !vfs::is_segment(identity) {
            return Err(format!("invalid identity '{identity}'"));
        }
        self.identity = identity.to_string();
        self.append = append;
        Ok(())
    }

    /// The game's name, which is the default identity.
    pub fn game_name(&self) -> &str {
        &self.game.name
    }

    /// Where the save directory is: a path natively.
    pub fn save_directory(&self) -> String {
        self.saves.location(&self.identity)
    }

    /// Where the game was mounted from.
    pub fn source(&self) -> &str {
        &self.game.source
    }

    fn layers(&self) -> [Layer; 2] {
        if self.append {
            [Layer::Game, Layer::Save]
        } else {
            [Layer::Save, Layer::Game]
        }
    }

    fn layer_info(&self, layer: Layer, path: &str) -> Option<Info> {
        match layer {
            Layer::Save => self.saves.info(&self.identity, path),
            Layer::Game => self.game.info(path),
        }
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let normalized =
            vfs::normalize(path).ok_or_else(|| VfsError::InvalidPath(path.to_string()))?;
        for layer in self.layers() {
            if self
                .layer_info(layer, &normalized)
                .is_some_and(|info| info.kind == Kind::File)
            {
                return match layer {
                    Layer::Save => self.saves.read(&self.identity, &normalized),
                    Layer::Game => self.game.read(&normalized),
                };
            }
        }
        // Not a file anywhere. Point out a save file whose name differs only in case, or else
        // let the game explain.
        if let Err(e @ VfsError::NotFound { hint: Some(_), .. }) =
            self.saves.read(&self.identity, &normalized)
        {
            return Err(e);
        }
        self.game.read(&normalized)
    }

    pub fn read_string(&self, path: &str) -> Result<String, VfsError> {
        String::from_utf8(self.read(path)?)
            .map_err(|_| VfsError::Io(format!("'{path}' is not valid UTF-8")))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.info(path).is_some()
    }

    /// What's at `path`, in whichever comes first of the save directory and the game.
    pub fn info(&self, path: &str) -> Option<Info> {
        let path = vfs::normalize_dir(path)?;
        self.layers()
            .into_iter()
            .find_map(|layer| self.layer_info(layer, &path))
    }

    /// The names in a directory of the save directory and the game, merged and sorted.
    pub fn list(&self, dir: &str) -> Vec<String> {
        let Some(dir) = vfs::normalize_dir(dir) else {
            return Vec::new();
        };
        let mut names = self.saves.list(&self.identity, &dir);
        names.extend(self.game.list(&dir));
        names.into_iter().collect()
    }

    /// Where the file or directory at `path` comes from: the save directory or the game.
    pub fn real_directory(&self, path: &str) -> Option<String> {
        let path = vfs::normalize_dir(path)?;
        let layer = self
            .layers()
            .into_iter()
            .find(|&layer| self.layer_info(layer, &path).is_some())?;
        Some(match layer {
            Layer::Save => self.save_directory(),
            Layer::Game => self.game.source.clone(),
        })
    }

    /// Writes a file in the save directory, creating its parent directories.
    pub fn write(&mut self, path: &str, data: &[u8], append: bool) -> Result<(), String> {
        let path = save_path(path)?;
        self.saves
            .write(&self.identity, &path, data, append)
            .map_err(|e| format!("could not write '{path}': {e}"))
    }

    /// Creates a directory in the save directory, and its parents.
    pub fn create_directory(&mut self, path: &str) -> Result<(), String> {
        let path = save_path(path)?;
        self.saves
            .create_dir(&self.identity, &path)
            .map_err(|e| format!("could not create '{path}': {e}"))
    }

    /// Removes a file or empty directory from the save directory.
    pub fn remove(&mut self, path: &str) -> Result<(), String> {
        let path = save_path(path)?;
        if self.saves.info(&self.identity, &path).is_none() {
            return Err(format!("could not remove '{path}': it doesn't exist"));
        }
        self.saves
            .remove(&self.identity, &path)
            .map_err(|e| format!("could not remove '{path}': {e}"))
    }
}

fn save_path(path: &str) -> Result<String, String> {
    vfs::normalize(path).ok_or_else(|| VfsError::InvalidPath(path.to_string()).to_string())
}

/// Where save directories live, one for each identity. Paths are normalized, and `""` is the
/// save directory itself.
trait Saves {
    fn location(&self, identity: &str) -> String;
    fn info(&self, identity: &str, path: &str) -> Option<Info>;
    fn read(&self, identity: &str, path: &str) -> Result<Vec<u8>, VfsError>;
    fn list(&self, identity: &str, dir: &str) -> BTreeSet<String>;
    fn write(&self, identity: &str, path: &str, data: &[u8], append: bool) -> Result<(), String>;
    fn create_dir(&self, identity: &str, path: &str) -> Result<(), String>;
    /// Removes a file or empty directory that exists.
    fn remove(&self, identity: &str, path: &str) -> Result<(), String>;
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_saves() -> Box<dyn Saves> {
    Box::new(disk::DiskSaves::new())
}

#[cfg(target_arch = "wasm32")]
fn platform_saves() -> Box<dyn Saves> {
    Box::new(keyed::KeyedSaves(web::JsStore))
}

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use std::{
        collections::BTreeSet,
        env, fs,
        io::Write,
        path::{Path, PathBuf},
    };

    use super::Saves;
    use crate::vfs::{self, Info, VfsError};

    /// Save directories on disk, under the platform's directory for application data.
    pub struct DiskSaves {
        /// `None` if the platform has no data directory, in which case nothing can be saved.
        root: Option<PathBuf>,
    }

    impl DiskSaves {
        pub fn new() -> DiskSaves {
            DiskSaves { root: root() }
        }

        #[cfg(test)]
        pub fn at(root: PathBuf) -> DiskSaves {
            DiskSaves { root: Some(root) }
        }

        fn dir(&self, identity: &str) -> Result<PathBuf, String> {
            let root = self.root.as_ref().ok_or("there's no save directory")?;
            Ok(root.join(identity))
        }
    }

    /// `PROTOGINE_SAVE_DIR`, for tests, or `protogine` in the platform's data directory.
    fn root() -> Option<PathBuf> {
        if let Some(dir) = env::var_os("PROTOGINE_SAVE_DIR") {
            return Some(dir.into());
        }
        let home = || env::var_os("HOME").map(PathBuf::from);
        let data = if cfg!(windows) {
            env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            home().map(|h| h.join("Library/Application Support"))
        } else {
            env::var_os("XDG_DATA_HOME")
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
                .or_else(|| home().map(|h| h.join(".local/share")))
        };
        Some(data?.join("protogine"))
    }

    fn io_error(e: std::io::Error) -> String {
        e.to_string()
    }

    impl Saves for DiskSaves {
        fn location(&self, identity: &str) -> String {
            self.dir(identity)
                .map_or_else(|e| e, |dir| dir.display().to_string())
        }

        fn info(&self, identity: &str, path: &str) -> Option<Info> {
            vfs::dir_info(&self.dir(identity).ok()?, path)
        }

        fn read(&self, identity: &str, path: &str) -> Result<Vec<u8>, VfsError> {
            let dir = self.dir(identity).map_err(VfsError::Io)?;
            vfs::read_dir_file(&dir, path)
        }

        fn list(&self, identity: &str, dir: &str) -> BTreeSet<String> {
            match self.dir(identity) {
                Ok(root) => vfs::dir_list(&root, dir),
                Err(_) => BTreeSet::new(),
            }
        }

        fn write(
            &self,
            identity: &str,
            path: &str,
            data: &[u8],
            append: bool,
        ) -> Result<(), String> {
            let full = self.dir(identity)?.join(path);
            if full.is_dir() {
                return Err("it's a directory".to_string());
            }
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).map_err(io_error)?;
            }
            let mut file = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&full)
                .map_err(io_error)?;
            file.write_all(data).map_err(io_error)
        }

        fn create_dir(&self, identity: &str, path: &str) -> Result<(), String> {
            fs::create_dir_all(self.dir(identity)?.join(path)).map_err(io_error)
        }

        fn remove(&self, identity: &str, path: &str) -> Result<(), String> {
            let full: PathBuf = self.dir(identity)?.join(Path::new(path));
            if full.is_dir() {
                fs::remove_dir(&full).map_err(|_| "the directory isn't empty".to_string())
            } else {
                fs::remove_file(&full).map_err(io_error)
            }
        }
    }
}

/// Save directories in a flat key-value store, as on the web: a file is the key
/// `identity/path`, and a directory is the key `identity/path/` with no data.
#[cfg(any(test, target_arch = "wasm32"))]
mod keyed {
    use std::collections::BTreeSet;

    use super::Saves;
    use crate::vfs::{Info, Kind, VfsError};

    pub trait Store {
        fn keys(&self) -> Vec<String>;
        fn read(&self, key: &str) -> Option<Vec<u8>>;
        fn size(&self, key: &str) -> Option<u64>;
        /// Seconds since the Unix epoch.
        fn modtime(&self, key: &str) -> Option<f64>;
        fn write(&self, key: &str, data: &[u8]);
        fn remove(&self, key: &str);
    }

    pub struct KeyedSaves<S>(pub S);

    fn key(identity: &str, path: &str) -> String {
        if path.is_empty() {
            format!("{identity}/")
        } else {
            format!("{identity}/{path}")
        }
    }

    fn dir_key(identity: &str, path: &str) -> String {
        if path.is_empty() {
            format!("{identity}/")
        } else {
            format!("{identity}/{path}/")
        }
    }

    impl<S: Store> KeyedSaves<S> {
        fn kind(&self, keys: &[String], identity: &str, path: &str) -> Option<Kind> {
            let (file, dir) = (key(identity, path), dir_key(identity, path));
            if !path.is_empty() && keys.contains(&file) {
                Some(Kind::File)
            } else if keys.iter().any(|k| k.starts_with(&dir)) {
                Some(Kind::Directory)
            } else {
                None
            }
        }

        /// Checks that `path` can become a file or directory: none of its parents is a file.
        fn check_parents(&self, keys: &[String], identity: &str, path: &str) -> Result<(), String> {
            let mut parent = String::new();
            for segment in path.split('/').take(path.split('/').count() - 1) {
                if !parent.is_empty() {
                    parent.push('/');
                }
                parent.push_str(segment);
                if self.kind(keys, identity, &parent) == Some(Kind::File) {
                    return Err(format!("'{parent}' is a file"));
                }
            }
            Ok(())
        }

        /// Adds directory keys for `path`'s parents, so they outlive the files in them.
        fn add_parents(&self, keys: &[String], identity: &str, path: &str) {
            let mut parent = String::new();
            for segment in path.split('/').take(path.split('/').count() - 1) {
                if !parent.is_empty() {
                    parent.push('/');
                }
                parent.push_str(segment);
                let dir = dir_key(identity, &parent);
                if !keys.contains(&dir) {
                    self.0.write(&dir, &[]);
                }
            }
        }
    }

    impl<S: Store> Saves for KeyedSaves<S> {
        fn location(&self, identity: &str) -> String {
            format!("indexeddb:protogine/{identity}")
        }

        fn info(&self, identity: &str, path: &str) -> Option<Info> {
            let keys = self.0.keys();
            match self.kind(&keys, identity, path)? {
                Kind::File => {
                    let key = key(identity, path);
                    Some(Info {
                        kind: Kind::File,
                        size: self.0.size(&key),
                        modtime: self.0.modtime(&key).map(|t| t as i64),
                    })
                }
                Kind::Directory => Some(Info {
                    modtime: self.0.modtime(&dir_key(identity, path)).map(|t| t as i64),
                    ..Info::directory()
                }),
            }
        }

        fn read(&self, identity: &str, path: &str) -> Result<Vec<u8>, VfsError> {
            self.0.read(&key(identity, path)).ok_or_else(|| {
                let prefix = format!("{identity}/");
                let hint = self.0.keys().into_iter().find_map(|k| {
                    let candidate = k.strip_prefix(&prefix)?;
                    candidate
                        .eq_ignore_ascii_case(path)
                        .then(|| candidate.to_string())
                });
                VfsError::NotFound {
                    path: path.to_string(),
                    hint,
                }
            })
        }

        fn list(&self, identity: &str, dir: &str) -> BTreeSet<String> {
            let prefix = dir_key(identity, dir);
            self.0
                .keys()
                .iter()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter_map(|rest| rest.split('/').next())
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        }

        fn write(
            &self,
            identity: &str,
            path: &str,
            data: &[u8],
            append: bool,
        ) -> Result<(), String> {
            let keys = self.0.keys();
            self.check_parents(&keys, identity, path)?;
            if self.kind(&keys, identity, path) == Some(Kind::Directory) {
                return Err("it's a directory".to_string());
            }
            let key = key(identity, path);
            let data = match (append, self.0.read(&key)) {
                (true, Some(mut existing)) => {
                    existing.extend_from_slice(data);
                    existing
                }
                _ => data.to_vec(),
            };
            self.add_parents(&keys, identity, path);
            self.0.write(&key, &data);
            Ok(())
        }

        fn create_dir(&self, identity: &str, path: &str) -> Result<(), String> {
            let keys = self.0.keys();
            self.check_parents(&keys, identity, path)?;
            match self.kind(&keys, identity, path) {
                Some(Kind::File) => Err("it's a file".to_string()),
                Some(Kind::Directory) => Ok(()),
                None => {
                    self.add_parents(&keys, identity, path);
                    self.0.write(&dir_key(identity, path), &[]);
                    Ok(())
                }
            }
        }

        fn remove(&self, identity: &str, path: &str) -> Result<(), String> {
            let keys = self.0.keys();
            match self.kind(&keys, identity, path) {
                Some(Kind::File) => self.0.remove(&key(identity, path)),
                Some(Kind::Directory) => {
                    let dir = dir_key(identity, path);
                    if keys.iter().any(|k| k.starts_with(&dir) && *k != dir) {
                        return Err("the directory isn't empty".to_string());
                    }
                    self.0.remove(&dir);
                }
                None => {}
            }
            Ok(())
        }
    }
}

/// The store in `web/index.html`, over save files loaded from IndexedDB.
#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn keys() -> Vec<String>;
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn read(key: &str) -> Option<Vec<u8>>;
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn size(key: &str) -> Option<f64>;
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn modtime(key: &str) -> Option<f64>;
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn write(key: &str, data: &[u8]);
        #[wasm_bindgen(js_namespace = protogineSaves)]
        fn remove(key: &str);
    }

    pub struct JsStore;

    impl super::keyed::Store for JsStore {
        fn keys(&self) -> Vec<String> {
            keys()
        }

        fn read(&self, key: &str) -> Option<Vec<u8>> {
            read(key)
        }

        fn size(&self, key: &str) -> Option<u64> {
            size(key).map(|s| s as u64)
        }

        fn modtime(&self, key: &str) -> Option<f64> {
            modtime(key)
        }

        fn write(&self, key: &str, data: &[u8]) {
            write(key, data);
        }

        fn remove(&self, key: &str) {
            remove(key);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::{cell::RefCell, collections::BTreeMap, io::Cursor, io::Write, path::PathBuf};

    use super::*;

    /// An in-memory store, standing in for the web's.
    #[derive(Default)]
    struct MemoryStore(RefCell<BTreeMap<String, Vec<u8>>>);

    impl keyed::Store for MemoryStore {
        fn keys(&self) -> Vec<String> {
            self.0.borrow().keys().cloned().collect()
        }

        fn read(&self, key: &str) -> Option<Vec<u8>> {
            self.0.borrow().get(key).cloned()
        }

        fn size(&self, key: &str) -> Option<u64> {
            self.0.borrow().get(key).map(|d| d.len() as u64)
        }

        fn modtime(&self, key: &str) -> Option<f64> {
            self.0.borrow().contains_key(key).then_some(1.0)
        }

        fn write(&self, key: &str, data: &[u8]) {
            self.0.borrow_mut().insert(key.to_string(), data.to_vec());
        }

        fn remove(&self, key: &str) {
            self.0.borrow_mut().remove(key);
        }
    }

    fn game() -> Vfs {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in [("main.lua", "main"), ("data/level.txt", "level 1")] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        Vfs::mount_zip(&bytes, Some("tester"), "tester.zip").unwrap()
    }

    /// Runs the same checks on the web's layout and on disk.
    fn each_backend(check: impl Fn(&mut Filesystem)) {
        check(&mut Filesystem::with_saves(
            game(),
            Box::new(keyed::KeyedSaves(MemoryStore::default())),
        ));

        let root = std::env::temp_dir().join(format!(
            "protogine-fs-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        check(&mut Filesystem::with_saves(
            game(),
            Box::new(disk::DiskSaves::at(PathBuf::from(&root))),
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn writes_and_reads_saves() {
        each_backend(|fs| {
            assert_eq!(fs.identity(), "tester");
            assert!(fs.list("").contains(&"main.lua".to_string()));
            fs.write("scores/best.txt", b"10", false).unwrap();
            fs.write("scores/best.txt", b"0", true).unwrap();
            assert_eq!(fs.read("scores/best.txt").unwrap(), b"100");
            assert_eq!(fs.info("scores").unwrap().kind, Kind::Directory);
            assert_eq!(fs.info("scores/best.txt").unwrap().size, Some(3));
            assert_eq!(fs.list("scores"), ["best.txt"]);
            assert_eq!(fs.list("/"), ["data", "main.lua", "scores"]);
            assert_eq!(
                fs.real_directory("scores/best.txt"),
                Some(fs.save_directory())
            );
            assert_eq!(fs.real_directory("main.lua").as_deref(), Some("tester.zip"));

            // Saves shadow the game's files, unless the identity is appended.
            fs.write("data/level.txt", b"level 2", false).unwrap();
            assert_eq!(fs.read("data/level.txt").unwrap(), b"level 2");
            fs.set_identity("tester", true).unwrap();
            assert_eq!(fs.read("data/level.txt").unwrap(), b"level 1");
            fs.set_identity("tester", false).unwrap();

            assert!(fs.remove("scores").is_err());
            fs.remove("scores/best.txt").unwrap();
            assert_eq!(fs.info("scores").unwrap().kind, Kind::Directory);
            fs.remove("scores").unwrap();
            assert!(fs.info("scores").is_none());
            assert!(fs.remove("scores").is_err());

            fs.create_directory("a/b").unwrap();
            assert_eq!(fs.info("a/b").unwrap().kind, Kind::Directory);
            assert!(fs.write("a", b"x", false).is_err());
            fs.write("file", b"x", false).unwrap();
            assert!(fs.create_directory("file/sub").is_err());
            assert!(fs.write("../escape", b"x", false).is_err());

            // Each identity has its own directory.
            fs.set_identity("other", false).unwrap();
            assert!(fs.info("file").is_none());
            assert!(fs.set_identity("a/b", false).is_err());
        });
    }

    #[test]
    fn save_paths_are_case_sensitive() {
        each_backend(|fs| {
            fs.write("Save.txt", b"x", false).unwrap();
            let error = fs.read("save.txt").unwrap_err().to_string();
            assert!(error.contains("found 'Save.txt'"), "{error}");
        });
    }
}
