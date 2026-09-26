//! `pg.filesystem`: the game's files, and a save directory to write to.

use std::cell::Cell;

use luars::{Lua, LuaApi, LuaFunction, LuaResult, LuaTable, LuaValue};

use super::{Args, Module, SharedHost};
use crate::{filesystem::Filesystem, vfs::Kind};

/// Love2D's `read` can also return FileData; only strings are supported.
const CONTAINERS: &[(&str, ())] = &[("string", ())];
/// Love2D's `FileType`. There are no symlinks or other kinds of files in the engine's view.
const FILE_TYPES: &[(&str, Option<Kind>)] = &[
    ("file", Some(Kind::File)),
    ("directory", Some(Kind::Directory)),
    ("symlink", None),
    ("other", None),
];

pub fn install(lua: &mut Lua, pg: &LuaTable, host: &SharedHost) -> LuaResult<()> {
    // The prelude's loadfile already reads through this filesystem.
    let loadfile: LuaFunction = lua.globals().get("loadfile")?;
    let mut m = Module::new(lua)?;
    m.table.set("load", loadfile)?;

    // Registers `pg.filesystem.<name>`, handing the closure the args and the filesystem.
    macro_rules! function {
        ($name:literal, |$args:ident, $fs:ident| $body:expr) => {{
            let host = host.clone();
            m.function($name, move |$args: &mut Args| {
                let mut host = host.borrow_mut();
                let $fs: &mut Filesystem = &mut host.fs;
                $body
            })?;
        }};
    }

    // ---- reading ----

    function!("read", |args, fs| {
        // `read(name, size)`, or Love2D's `read("string", name, size)`.
        let first = if args.get(2).is_some_and(|v| v.is_string()) {
            args.option(1, "container type", CONTAINERS)?;
            2
        } else {
            1
        };
        let path = args.string(first)?;
        let size = args.opt_integer(first + 1, -1)?;
        match fs.read(&path) {
            Ok(mut bytes) => {
                if size >= 0 {
                    bytes.truncate(size as usize);
                }
                args.push_bytes(&bytes)?;
                args.state.push(bytes.len() as i64)?;
                Ok(2)
            }
            Err(e) => args.ret((LuaValue::nil(), e.to_string())),
        }
    });
    function!("lines", |args, fs| {
        let path = args.string(1)?;
        let bytes = fs.read(&path).map_err(|e| args.error(e))?;
        let lines = split_lines(&bytes);
        let next = Cell::new(0);
        let iterator = args.state.create_closure(move |state| {
            let Some(line) = lines.get(next.get()) else {
                return Ok(0);
            };
            next.set(next.get() + 1);
            let value = state.create_bytes(line)?;
            state.push_value(value)?;
            Ok(1)
        })?;
        args.state.push_value(iterator)?;
        Ok(1)
    });
    function!("getInfo", |args, fs| {
        // `getInfo(path, filtertype, info)`, where both are optional.
        let path = args.string(1)?;
        let (filter, table_at) = if args.get(2).is_some_and(|v| v.is_string()) {
            (Some(args.option(2, "file type", FILE_TYPES)?), 3)
        } else {
            (None, 2)
        };
        let given = args.table(table_at)?;
        let info = fs
            .info(&path)
            .filter(|info| filter.is_none_or(|wanted| wanted == Some(info.kind)));
        let Some(info) = info else {
            return args.ret(LuaValue::nil());
        };
        let table = match given {
            Some(table) => table,
            None => LuaApi::create_table(args.state)?,
        };
        let kind = match info.kind {
            Kind::File => "file",
            Kind::Directory => "directory",
        };
        table.set("type", kind)?;
        table.set("size", info.size.map(|s| s as i64))?;
        table.set("modtime", info.modtime)?;
        args.ret(table)
    });
    function!("getDirectoryItems", |args, fs| {
        let dir = args.string(1)?;
        let names = LuaApi::create_sequence_from(args.state, fs.list(&dir))?;
        args.ret(names)
    });
    function!("getRealDirectory", |args, fs| {
        let path = args.string(1)?;
        args.ret(fs.real_directory(&path))
    });

    // ---- writing ----

    function!("write", |args, fs| write(args, fs, false));
    function!("append", |args, fs| write(args, fs, true));
    function!("createDirectory", |args, fs| {
        let path = args.string(1)?;
        outcome(args, fs.create_directory(&path))
    });
    function!("remove", |args, fs| {
        let path = args.string(1)?;
        outcome(args, fs.remove(&path))
    });

    // ---- identity ----

    function!("getIdentity", |args, fs| args.ret(fs.identity()));
    function!("setIdentity", |args, fs| {
        let identity = args.string(1)?;
        let append = args.boolean(2);
        fs.set_identity(&identity, append)
            .map_err(|e| args.arg_error(1, e))?;
        Ok(0)
    });
    function!("getSaveDirectory", |args, fs| args.ret(fs.save_directory()));
    function!("getSource", |args, fs| args.ret(fs.source()));

    m.finish(pg, "filesystem")
}

/// `write(name, data, size)` or `append(...)`.
fn write(args: &mut Args, fs: &mut Filesystem, append: bool) -> LuaResult<usize> {
    let path = args.string(1)?;
    let mut data = args.bytes(2)?;
    let size = args.opt_integer(3, -1)?;
    if size >= 0 {
        data.truncate(size as usize);
    }
    outcome(args, fs.write(&path, &data, append))
}

/// Returns `true`, or `false` and the error, as Love2D's writing functions do.
fn outcome(args: &mut Args, result: Result<(), String>) -> LuaResult<usize> {
    match result {
        Ok(()) => args.ret(true),
        Err(e) => args.ret((false, e)),
    }
}

/// Splits text into lines at `\n`, dropping the `\r` of `\r\n`. A final newline doesn't start
/// another line.
fn split_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = bytes
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
        .collect();
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        lines.pop();
    }
    lines
}
