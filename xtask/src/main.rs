//! Build helpers, run with `cargo xtask <command>`.
//!
//! - `web [--release]`: build for wasm32, run wasm-bindgen, and assemble `target/web/`.
//! - `serve [--release] [--port N]`: `web`, then serve `target/web/` on localhost.

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
const USAGE: &str = "usage: cargo xtask <web|serve> [--release] [--port N]";

type Result<T = ()> = std::result::Result<T, String>;

struct Options {
    release: bool,
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
        port: 8080,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => opts.release = true,
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

    println!("web build written to {}", out.display());
    Ok(out)
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
        _ => "application/octet-stream",
    }
}
