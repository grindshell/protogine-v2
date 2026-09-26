//! Lua regression tests: runs the engine binary on the suites in `tests/lua` and on small
//! generated games that fail on purpose, then checks the output.
//!
//! Each run opens a window for a moment. `PROTOGINE_EXIT_ON_ERROR` makes the engine exit with
//! status 1 after logging an error report, instead of showing the error screen.
//! `PROTOGINE_SAVE_DIR` keeps save directories in the temp dir.

#![cfg(not(target_arch = "wasm32"))]

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn run(game: &Path, args: &[&str]) -> Output {
    run_with_saves(game, args, &TempGame::path("saves"))
}

/// Runs the engine with save directories under `saves`.
fn run_with_saves(game: &Path, args: &[&str], saves: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_protogine-v2"))
        .arg(game)
        .args(args)
        .env("PROTOGINE_EXIT_ON_ERROR", "1")
        .env("PROTOGINE_SAVE_DIR", saves)
        .output()
        .expect("could not run the engine")
}

fn describe(output: &Output) -> String {
    format!(
        "exit: {}\n--- stdout\n{}\n--- stderr\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn suites_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lua")
}

/// Runs a suite from `tests/lua` (see `tests/lua/harness.lua` for its output format).
fn suite(game: &Path, args: &[&str]) {
    check_suite(&run(game, args));
}

fn check_suite(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", describe(output));
    if let Some(reason) = stdout.lines().find_map(|l| l.strip_prefix("skip: ")) {
        eprintln!("skipped: {reason}");
        return;
    }
    assert!(
        !stdout.lines().any(|l| l.starts_with("FAIL")),
        "{}",
        describe(output)
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("done: ") && l.ends_with(" 0 failed")),
        "the suite didn't finish\n{}",
        describe(output)
    );
}

#[test]
fn lifecycle() {
    suite(&suites_dir(), &["lifecycle", "one", "two"]);
}

#[test]
fn graphics() {
    suite(&suites_dir(), &["graphics"]);
}

#[test]
fn input() {
    suite(&suites_dir(), &["input"]);
}

#[test]
fn math() {
    suite(&suites_dir(), &["math"]);
}

#[test]
fn audio() {
    suite(&suites_dir(), &["audio"]);
}

#[test]
fn system() {
    let os = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "OS X",
        _ => "Linux",
    };
    // The clipboard check replaces the clipboard's contents, so only CI runs it.
    let clipboard = if std::env::var_os("CI").is_some() {
        "clipboard"
    } else {
        "keep-clipboard"
    };
    suite(&suites_dir(), &["system", os, clipboard]);
}

#[test]
fn filesystem() {
    // The second run checks what the first one saved.
    let saves = TempGame(TempGame::path("filesystem-saves"));
    fs::remove_dir_all(&saves.0).ok();
    check_suite(&run_with_saves(&suites_dir(), &["filesystem"], &saves.0));
    check_suite(&run_with_saves(
        &suites_dir(),
        &["filesystem", "again"],
        &saves.0,
    ));
}

/// A game directory (or zip) in the temp dir, deleted afterwards.
struct TempGame(PathBuf);

impl TempGame {
    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("protogine-test-{}-{name}", std::process::id()))
    }

    fn dir(name: &str, files: &[(&str, &str)]) -> Self {
        let root = Self::path(name);
        for (path, contents) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        TempGame(root)
    }

    /// Zips a directory, with paths relative to it.
    fn zip_of(name: &str, dir: &Path) -> Self {
        fn add(zip: &mut zip::ZipWriter<fs::File>, root: &Path, dir: &Path) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    add(zip, root, &path);
                } else {
                    let name = path.strip_prefix(root).unwrap().to_string_lossy();
                    zip.start_file(
                        name.replace('\\', "/"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                    zip.write_all(&fs::read(&path).unwrap()).unwrap();
                }
            }
        }
        let path = Self::path(&format!("{name}.zip"));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        add(&mut zip, dir, dir);
        zip.finish().unwrap();
        TempGame(path)
    }
}

impl Drop for TempGame {
    fn drop(&mut self) {
        if self.0.is_dir() {
            fs::remove_dir_all(&self.0).ok();
        } else {
            fs::remove_file(&self.0).ok();
        }
    }
}

#[test]
fn runs_from_a_zip() {
    let zip = TempGame::zip_of("suites", &suites_dir());
    suite(&zip.0, &["lifecycle", "one", "two"]);
}

/// Runs a game that must fail, and returns its error report.
fn error_report(name: &str, files: &[(&str, &str)]) -> String {
    let game = TempGame::dir(name, files);
    let output = run(&game.0, &[]);
    assert_eq!(output.status.code(), Some(1), "{}", describe(&output));
    String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n")
}

fn assert_contains(report: &str, expected: &[&str]) {
    for text in expected {
        assert!(report.contains(text), "expected {text:?} in:\n{report}");
    }
    // The engine's own frames are filtered out of tracebacks.
    for hidden in ["prelude", "xpcall"] {
        assert!(
            !report.contains(hidden),
            "unexpected {hidden:?} in:\n{report}"
        );
    }
}

#[test]
fn runtime_error_traceback() {
    let report = error_report(
        "runtime",
        &[
            (
                "main.lua",
                "local enemies = require(\"enemies\")\n\
                 \n\
                 function pg.load()\n\
                 \x20 enemies.spawn({ name = \"slime\" })\n\
                 end\n",
            ),
            (
                "enemies.lua",
                "local enemies = {}\n\
                 \n\
                 local function place(enemy)\n\
                 \x20 return enemy.position.x\n\
                 end\n\
                 \n\
                 function enemies.spawn(enemy)\n\
                 \x20 place(enemy)\n\
                 end\n\
                 \n\
                 return enemies\n",
            ),
        ],
    );
    assert_contains(
        &report,
        &[
            "enemies.lua:4: attempt to index a nil value (field 'position')",
            "stack traceback:",
            "enemies.lua:4: in upvalue 'place'",
            "enemies.lua:8: in field 'spawn'",
            "main.lua:4:",
        ],
    );
}

#[test]
fn syntax_error_in_main() {
    let report = error_report(
        "syntax",
        &[("main.lua", "function pg.load()\n  local x = (1 + 2\nend\n")],
    );
    assert_contains(&report, &["main.lua:3:"]);
}

#[test]
fn syntax_error_in_a_module() {
    let report = error_report(
        "module-syntax",
        &[
            ("main.lua", "require(\"broken\")\n"),
            ("broken.lua", "local = 5\n"),
        ],
    );
    assert_contains(&report, &["broken.lua:1:"]);
}

#[test]
fn error_in_conf() {
    let report = error_report(
        "conf-error",
        &[
            ("main.lua", ""),
            (
                "conf.lua",
                "function pg.conf(t)\n  error(\"bad conf\")\nend\n",
            ),
        ],
    );
    assert_contains(&report, &["conf.lua:2: bad conf"]);
}

#[test]
fn wrong_type_in_conf() {
    let report = error_report(
        "conf-type",
        &[
            ("main.lua", ""),
            (
                "conf.lua",
                "function pg.conf(t)\n  t.window.width = \"wide\"\nend\n",
            ),
        ],
    );
    assert_contains(
        &report,
        &["conf.lua: t.window.width has the wrong type (string)"],
    );
}

#[test]
fn identity_in_conf() {
    let report = error_report(
        "identity",
        &[
            (
                "main.lua",
                "error(\"identity \" .. pg.filesystem.getIdentity())\n",
            ),
            (
                "conf.lua",
                "function pg.conf(t)\n  t.identity = \"custom\"\nend\n",
            ),
        ],
    );
    assert_contains(&report, &["identity custom"]);

    let report = error_report(
        "bad-identity",
        &[
            ("main.lua", ""),
            (
                "conf.lua",
                "function pg.conf(t)\n  t.identity = \"a/b\"\nend\n",
            ),
        ],
    );
    assert_contains(&report, &["conf.lua: t.identity: invalid identity 'a/b'"]);
}

#[test]
fn callback_that_is_not_a_function() {
    let report = error_report("not-a-function", &[("main.lua", "pg.update = 5\n")]);
    assert_contains(&report, &["pg.update must be a function, not a number"]);
}

#[test]
fn error_in_draw() {
    let report = error_report(
        "draw",
        &[("main.lua", "function pg.draw()\n  error(\"boom\")\nend\n")],
    );
    assert_contains(
        &report,
        &["main.lua:2: boom", "main.lua:2: in function <main.lua:1>"],
    );
}

#[test]
fn missing_main() {
    let report = error_report("no-main", &[("other.lua", "")]);
    assert_contains(&report, &["file not found: 'main.lua'"]);
}
