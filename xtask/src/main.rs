//! Build helpers, run with `cargo xtask <command>`.
//!
//! - `web [--release] [--game DIR]`: build for wasm32, run wasm-bindgen, and assemble
//!   `target/web/`. `--game` packs DIR into `game.zip`; without it the page shows the no-game
//!   screen.
//! - `serve [--release] [--game DIR] [--port N]`: `web`, then serve `target/web/` on localhost.

use std::{
    env, fs,
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    process::{Command, ExitCode},
    thread,
    time::Duration,
};

const CRATE: &str = "protogine-v2";
/// wasm-bindgen output name; `web/index.html` loads `game.js` and `game_bg.wasm`.
const OUT_NAME: &str = "game";
/// The game archive the engine fetches on startup (see `src/main.rs`).
const GAME_ARCHIVE: &str = "game.zip";
const USAGE: &str = "usage: cargo xtask <web|serve> [--release] [--game DIR] [--port N]";

type Result<T = ()> = std::result::Result<T, String>;

struct Options {
    release: bool,
    game: Option<PathBuf>,
    port: u16,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "web" => {
            parse_options(rest).and_then(|o| build_web(&o)).map(drop)
        }
        Some((cmd, rest)) if cmd == "serve" => parse_options(rest).and_then(|o| serve(&o)),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_options(args: &[String]) -> Result<Options> {
    let mut opts = Options {
        release: false,
        game: None,
        port: 8080,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => opts.release = true,
            "--game" => opts.game = Some(args.next().ok_or("--game needs a directory")?.into()),
            "--port" => {
                let value = args.next().ok_or("--port needs a value")?;
                opts.port = value
                    .parse()
                    .map_err(|_| format!("invalid port: {value}"))?;
            }
            other => return Err(format!("unknown argument: {other}\n{USAGE}")),
        }
    }
    Ok(opts)
}

fn root_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn target_dir(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

fn run(cmd: &mut Command) -> Result {
    let status = cmd
        .status()
        .map_err(|e| format!("failed to run {cmd:?}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} exited with {status}"))
    }
}

fn build_web(opts: &Options) -> Result<PathBuf> {
    let root = root_dir();
    let target = target_dir(&root);
    let profile = if opts.release { "release" } else { "debug" };

    let mut cargo = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.current_dir(&root).args([
        "build",
        "--package",
        CRATE,
        "--target",
        "wasm32-unknown-unknown",
    ]);
    if opts.release {
        cargo.arg("--release");
    }
    run(&mut cargo)?;

    let out = target.join("web");
    fs::create_dir_all(&out).map_err(|e| format!("create {}: {e}", out.display()))?;

    let wasm = target
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join(format!("{CRATE}.wasm"));
    run(Command::new("wasm-bindgen")
        .arg(&wasm)
        .arg("--out-dir")
        .arg(&out)
        .args(["--out-name", OUT_NAME, "--target", "web", "--no-typescript"]))
    .map_err(|e| {
        format!(
            "{e}\nwasm-bindgen-cli must be installed and match the wasm-bindgen version in Cargo.lock:\n  \
             cargo install wasm-bindgen-cli --version <version> --locked"
        )
    })?;

    let js_path = out.join(format!("{OUT_NAME}.js"));
    let js =
        fs::read_to_string(&js_path).map_err(|e| format!("read {}: {e}", js_path.display()))?;
    fs::write(&js_path, patch_bindgen_js(&js)?)
        .map_err(|e| format!("write {}: {e}", js_path.display()))?;

    for file in ["index.html", "gl.js"] {
        let src = root.join("web").join(file);
        fs::copy(&src, out.join(file)).map_err(|e| format!("copy {}: {e}", src.display()))?;
    }

    let archive = out.join(GAME_ARCHIVE);
    match &opts.game {
        Some(game) => pack_game(game, &archive)?,
        None => match fs::remove_file(&archive) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("remove {}: {e}", archive.display())),
        },
    }

    println!("web build written to {}", out.display());
    Ok(out)
}

/// Zips a game directory with `main.lua` at the archive root.
fn pack_game(game: &Path, archive: &Path) -> Result {
    if !game.join("main.lua").is_file() {
        return Err(format!("{} has no main.lua", game.display()));
    }

    let mut files = Vec::new();
    collect_files(game, game, &mut files)?;
    files.sort();

    let file =
        fs::File::create(archive).map_err(|e| format!("create {}: {e}", archive.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    for (name, path) in &files {
        let data = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let write_error =
            |e: &dyn std::fmt::Display| format!("write {name} to {}: {e}", archive.display());
        zip.start_file(name.as_str(), options)
            .map_err(|e| write_error(&e))?;
        zip.write_all(&data).map_err(|e| write_error(&e))?;
    }
    zip.finish()
        .map_err(|e| format!("finish {}: {e}", archive.display()))?;

    println!(
        "packed {} files from {} into {GAME_ARCHIVE}",
        files.len(),
        game.display()
    );
    Ok(())
}

/// Collects `(archive name, path)` for every file under `dir`, skipping hidden entries.
fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result {
    let entries = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("read {}: {e}", dir.display()))?
            .path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, files)?;
        } else {
            let relative = path.strip_prefix(root).unwrap();
            let name = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((name, path));
        }
    }
    Ok(())
}

/// Rewrites wasm-bindgen's `--target web` glue so miniquad's gl.js can own instantiation.
///
/// gl.js instantiates the module with its own `env` imports and then calls `main`, so the glue
/// must neither instantiate the module nor run `__wbindgen_start` (which also calls `main`).
/// This drops the glue's imports of the `env` module (gl.js supplies those) and appends two
/// exports used by `web/index.html`:
/// - `bindgenImports()` returns the wasm-bindgen import modules to merge into gl.js's imports.
/// - `attachBindgen(exports)` points the glue at the instance gl.js created.
fn patch_bindgen_js(js: &str) -> Result<String> {
    for marker in ["function __wbg_get_imports()", "wasm = instance.exports;"] {
        if !js.contains(marker) {
            return Err(format!(
                "unexpected wasm-bindgen output (missing `{marker}`); patch_bindgen_js needs updating"
            ));
        }
    }

    let mut out: String = js
        .lines()
        .filter(|line| {
            let line = line.trim();
            let env_import = line.starts_with("import * as ")
                && line.trim_end_matches(';').ends_with(r#"from "env""#);
            let env_entry = line.starts_with(r#""env": "#);
            !env_import && !env_entry
        })
        .flat_map(|line| [line, "\n"])
        .collect();

    if out.contains(r#"from "env""#) || out.contains(r#""env":"#) {
        return Err("wasm-bindgen output still references `env` after patching".into());
    }

    out.push_str(
        r#"
// ---- Appended by `cargo xtask web` (see xtask/src/main.rs) ----

export function bindgenImports() {
    return __wbg_get_imports();
}

export function attachBindgen(instanceExports) {
    wasm = instanceExports;
    // What `__wbindgen_start` does, minus calling `main`, which gl.js does itself.
    for (const module of Object.values(bindgenImports())) {
        module.__wbindgen_init_externref_table?.();
    }
}
"#,
    );
    Ok(out)
}

fn serve(opts: &Options) -> Result {
    let dir = build_web(opts)?;
    let listener = TcpListener::bind(("127.0.0.1", opts.port))
        .map_err(|e| format!("bind port {}: {e}", opts.port))?;
    println!(
        "serving {} at http://127.0.0.1:{}/",
        dir.display(),
        opts.port
    );

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let dir = dir.clone();
        // One thread per connection: browsers open idle speculative connections that would
        // otherwise block every other request.
        thread::spawn(move || {
            if let Err(e) = handle_request(stream, &dir) {
                eprintln!("request failed: {e}");
            }
        });
    }
    Ok(())
}

/// Minimal static file server: GET only, one request per connection.
fn handle_request(mut stream: TcpStream, dir: &Path) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? <= 2 {
            break;
        }
    }

    let url_path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let rel = url_path
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    let rel = Path::new(if rel.is_empty() { "index.html" } else { rel });

    let file = rel
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| dir.join(rel))
        .and_then(|path| fs::read(&path).ok().map(|body| (path, body)));

    let (status, content_type, body) = match file {
        Some((path, body)) => ("200 OK", content_type(&path), body),
        None => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found".to_vec(),
        ),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}
