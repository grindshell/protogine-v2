# protogine-v2

Protogine is a 2D game framework in the spirit of [LÖVE (Love2D)](https://love2d.org/): games are written in **Lua**, and a Rust host runs them. The host uses **macroquad** for the window, main loop, rendering and input, **luars** for the Lua runtime, and **kira** for audio. The Rust binary is the engine. A game is a folder of Lua scripts and assets that the engine loads and runs.

**Status:** the core lifecycle, `pg.graphics`, input (`pg.keyboard`, `pg.mouse`, `pg.touch`), `pg.audio`, `pg.math`, `pg.filesystem` and `pg.system` work on native and web; `games/demo` exercises them. See [docs/api.md](docs/api.md) for exactly what is implemented.

## Stack

| Crate       | Version | Role                                                                |
| ----------- | ------- | ------------------------------------------------------------------- |
| `macroquad` | 0.4     | Window, async main loop, drawing, input, file/asset loading         |
| `luars`     | 0.26    | Pure-Rust Lua 5.5 runtime (compiler, VM, GC, stdlib) and host API   |
| `kira`      | 0.12    | Audio playback and mixing                                           |

### Why luars instead of mlua/Luau

The project originally targeted Luau through `mlua`. Luau is C++, and it can't be compiled for `wasm32-unknown-unknown` (macroquad's web target) because that target has no libc or libc++. mlua only supports WASM through emscripten, which macroquad doesn't support. luars is pure Rust, so it builds for every target with no C toolchain.

Trade-offs:

- Scripts are Lua 5.5, not Luau.
- luars is an interpreter with no JIT.
- It's a young crate, and its README examples lag behind the 0.26 API.

### luars API notes

- Most high-level methods live on the `LuaApi` trait, which must be in scope: `use luars::{Lua, LuaApi, SafeOption, Stdlib};`.
- Create a state with `Lua::new(SafeOption::default())`.
- Open libraries with `open_stdlib(Stdlib::X)` or `open_stdlibs(&[...])`. The README's `load_stdlibs` doesn't exist.
- `eval`, `execute`, `register_function`, `create_function` and `globals` are on `LuaApi`. `load(src)` returns a `Chunk` builder.
- For userdata, use `#[derive(LuaUserData)]` with `#[lua_methods]`, then `register_type_of::<T>(name)`.
  - Rename methods to Love2D names with `#[lua(name = "getWidth")]`.
  - Don't give a method the Rust name `type_name`, because it collides with `UserDataTrait::type_name`.
  - Reading an unknown field on userdata raises an error instead of returning nil.
  - `pub` fields become Lua fields, so keep a userdata's fields private or `pub(super)`.
  - `#[lua_methods]` can't take variadic arguments, return `self`, or give standard argument errors. For those, use the `methods!` macro in `src/api/mod.rs` instead, which writes the methods against `Args`. It replaces the method lookup that the derive calls; `src/api/math.rs` uses it.
- **Errors carry no message.** `LuaError` is a bare enum, and the message lives in VM state. Call `lua.get_error_message(e)` right away; it can only be read once.
- **Typed callbacks** (`create_function`) can't take a variable number of arguments. Their argument errors don't name the function, and if one returns `LuaResult`, the error message is lost ("Runtime Error"). `pg.*` functions therefore use raw callbacks through `api::Args` (see `src/api/mod.rs`).
- `LuaTable`, `LuaFunction` and `Value` hold raw pointers into the VM, so drop them before the `Lua` that owns them. `Game` declares `lua` as its last field for this reason.
- Async support comes through `register_async_function` and the `LuaAsyncApi` trait (`eval_async`, `call_async*`). Whether these work under macroquad's executor is still unverified.
- `Lua` is `!Send` by default. The `unsafe-send` feature only adds unchecked `unsafe impl Send`. Don't enable it, because macroquad's loop is single-threaded anyway.
- Source code is in `~/.cargo/registry/src/*/luars-*/src/`, and `src/lua_api/mod.rs` has the full trait surface.

## Architecture

- **Lua API:** specified in [docs/api.md](docs/api.md). It's Love2D-shaped under a single global, `pg`, with callbacks like `pg.update(dt)` and modules like `pg.graphics`. This Lua-facing API is the product. Update the doc in the same change as any API change, and record every divergence from Love2D there.
- **Files are mounted up front:** native reads the game directory or `.zip`, and the web fetches `game.zip` before any Lua runs. Every file API, and `require`, is synchronous on every platform.
- **Save directories:** a writable directory per identity, layered over the game's files so reads check it first. Natively it's a directory under the platform's data directory, or under `PROTOGINE_SAVE_DIR` if that's set (the tests set it). On the web it's IndexedDB. `web/index.html` loads every saved file into memory before starting the wasm, and exposes them to Rust as `window.protogineSaves`, a synchronous key-value store that writes back to IndexedDB in the background.
- **Audio:** each game owns a kira `AudioManager`, created on first use and dropped when the game stops. Sources are decoded into memory up front, even `"stream"` ones. Don't switch to kira's `StreamingSoundData`: in kira 0.12.4, its decode thread spins forever at the end of an OGG Vorbis file after a seek, so the sound hangs just before the end and looping breaks. MP3, FLAC and WAV streams are fine. Streaming also doesn't exist on wasm.

Source layout:

| Path | Role |
| --- | --- |
| `src/main.rs` | Entry point. Natively it mounts the game and runs `conf.lua` before opening the window. On the web it opens the window, then fetches `game.zip`. |
| `src/engine.rs` | The main loop, a state machine over the no-game, game, error and blank screens. It also handles quitting, and drains the input queue every frame. |
| `src/game.rs` | `Game`, which owns the Lua state and runs the lifecycle (`conf.lua`, `main.lua`, `pg.*` callbacks). `Host` is the engine state that `pg.*` functions share through `Rc<RefCell<_>>`. |
| `src/api/` | The `pg.*` bindings, one file per module. `mod.rs` has the `Args` helper and the `methods!` macro. `prelude.lua` runs first in every game and sets up `require`, `print`, the restricted `os`/`debug`, and `invoke` (`xpcall` plus a traceback). |
| `src/graphics.rs` | The engine side of `pg.graphics`: state, the transform stack, shapes, images and text on top of macroquad. It has no Lua dependency. |
| `src/input.rs` | The engine side of input. It reads macroquad's event queue (one subscriber for the whole run, since macroquad can't unsubscribe), turns it into Love2D-style events, and tracks the state the getters report. It normalizes wheel units, counts multi-clicks, and turns the primary touch into mouse events. It has no Lua dependency, and its event logic is unit-tested. |
| `src/audio.rs` | The engine side of `pg.audio`: the lazily created `AudioManager`, and `Source`, which tracks the play, pause and stop state the game asked for, since kira applies commands a block late. It also keeps playing sources reachable after the game drops them, like Love2D's source pool. It has no Lua dependency. |
| `src/math.rs` | The engine side of `pg.math`: Love2D's random number generator, noise, triangulation and Bézier curves, ported so that seeds and noise give the same results as Love2D. It has no Lua dependency. |
| `src/vfs.rs` | The game's read-only, case-sensitive files: a directory or an in-memory zip. It also names the game, which is the default save identity. |
| `src/filesystem.rs` | The engine side of `pg.filesystem`: the save directory, on disk or in the web's key-value store, layered over the game's files. It has no Lua dependency, and its unit tests run against both kinds of save directory. |
| `src/system.rs` | The engine side of `pg.system`: the platform's name, the clipboard, power, and opening URLs, with a module per platform. It calls Windows through `winapi`, macOS through `core-foundation-sys` and IOKit, and reads Linux's `/sys/class/power_supply`. It has no Lua dependency. |
| `src/screens.rs` | The no-game and error screens. |
| `src/conf.rs` | `Conf`, filled in by `pg.conf(t)`, and its conversion to a window config. |

## Targets

- **Native desktop:** Windows, macOS, Linux.
- **Web:** macroquad's wasm build (`wasm32-unknown-unknown` plus its JS loader).

Keep shared code WASM-compatible:

- Don't use `std::thread`, blocking `std::fs`, or `std::time::Instant` in shared paths. `Instant::now()` panics on `wasm32-unknown-unknown`.
- Use macroquad equivalents instead, such as `load_file` and `get_time`.
- Gate native-only code with `#[cfg(not(target_arch = "wasm32"))]`.

### Web build: wasm-bindgen + miniquad's loader

macroquad normally loads through miniquad's `gl.js`, which only supplies the `env` import module. kira (cpal's WebAudio backend) and luars (`js_sys::Date::now()`, plus chrono's `wasmbind`) also need wasm-bindgen imports. So the web build runs wasm-bindgen and then glues the two loaders together.

`cargo xtask web` (in `xtask/src/main.rs`) does the following:

1. Builds `protogine-v2` for `wasm32-unknown-unknown`.
2. Runs `wasm-bindgen --target web --out-name game`.
3. Patches `game.js`:
   - It drops the glue's `import ... from "env"` lines, because gl.js supplies `env`.
   - It appends `bindgenImports()` and `attachBindgen(exports)`.
   - The patch fails loudly if a new wasm-bindgen changes the glue's shape.
4. Copies `web/index.html` and `web/gl.js` into `target/web/`.
5. With `--game DIR`, zips the game into `game.zip` under a top-level directory named after `DIR`. The engine strips that directory and takes its name as the game's name, the default save identity.

In `web/index.html`, a gl.js plugin handles the wasm-bindgen side:

- `register_plugin` merges the wasm-bindgen imports into gl.js's import object.
- `on_init` calls `attachBindgen(wasm_exports)`, which points the glue at the instance and initializes the externref table.

gl.js then calls `main`. `__wbindgen_start` is deliberately never called, because for a bin crate it also runs `main`.

Requirements and notes:

- `wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock`, currently 0.2.129. Install it with `cargo install wasm-bindgen-cli --version 0.2.129 --locked`. Reinstall whenever `Cargo.lock` bumps wasm-bindgen.
- `web/gl.js` is vendored from miniquad at the commit that miniquad 0.4.11 was published from (`4f13d4a`). Update it whenever the miniquad version in `Cargo.lock` changes.
- Browsers keep an `AudioContext` suspended until a user gesture. `web/index.html` wraps the `AudioContext` constructor to track the contexts cpal creates, and resumes them on every pointerdown, keydown and touchend.
- The engine calls JavaScript through wasm-bindgen imports: the save store in `src/filesystem.rs` (`window.protogineSaves`), and the clipboard, battery and URL opening in `src/system.rs` (`window.protogineSystem`). `web/index.html` sets both up before loading the wasm. `wasm-bindgen` is a direct dependency for wasm32 only, and the loader merges its imports like any others.

## Commands

```sh
cargo run -- games/demo      # run a game natively (a directory or .zip); no argument shows the no-game screen
cargo build --release
cargo clippy --workspace --all-targets
cargo fmt --all
cargo test                   # unit tests plus the Lua regression tests (opens windows; see Tests)
cargo xtask web [--release] [--game DIR]                # web build into target/web/, packing DIR as game.zip
cargo xtask serve [--release] [--game DIR] [--port N]   # web build, then serve at http://127.0.0.1:8080/
```

The repo is a Cargo workspace: the root package is the engine, and `xtask/` holds the build tooling. `.claude/launch.json` has a `web` config that runs `cargo xtask serve --game games/demo`.

## Tests

`cargo test` runs the Rust unit tests and the Lua regression tests in `tests/lua.rs`. The Lua tests run the engine binary, so each one opens a window briefly, and they need a display. The audio suite skips itself if there's no audio device.

- **Suites:** `tests/lua/` is a test game with one suite per area in `tests/lua/suites/` (`lifecycle`, `graphics`, `input`, `audio`, `math`, `filesystem` and `system`).
  - Suites use the helpers in `tests/lua/harness.lua`. `t.check` and `t.errors` print `ok` or `FAIL` lines, and `t.finish()` prints the summary and quits.
  - Run one suite by hand with `cargo run -- tests/lua graphics`.
  - When you change an API, add checks for it to its suite, asserting exact error messages. To add a suite, create `suites/<name>.lua` and a `#[test]` in `tests/lua.rs`.
- **Error reports:** `tests/lua.rs` also writes small failing games to the temp dir and checks their error reports: tracebacks, and syntax, `conf.lua` and callback errors.
  - These rely on `PROTOGINE_EXIT_ON_ERROR`. When it's set, the native engine logs the report to stderr and exits with status 1 instead of showing the error screen.
- **Saves:** every engine run in `tests/lua.rs` sets `PROTOGINE_SAVE_DIR` to the temp dir, so tests never touch real saves. The `filesystem` suite runs twice on a fresh save directory, and the second run checks what the first one wrote.
- **System:** the `system` suite only replaces the clipboard's contents where the `CI` environment variable is set. It never opens a URL; it only checks that disallowed ones are refused.
- **Event handling:** input events can't be injected into a real window, so `src/input.rs` unit-tests the event logic directly.

### CI

`.github/workflows/ci.yml` runs on pushes to `master` and on pull requests. Every job installs the pinned toolchain with `rustup toolchain install`.

- **Format and lint:** `cargo fmt --check`, then clippy with `-D warnings` for native and for `wasm32`.
- **Test** runs on Linux, Windows and macOS.
  - Linux runs everything under `xvfb-run`, with Mesa's software OpenGL. There's no sound card, so the audio suite skips itself.
  - Windows and macOS runners have no usable OpenGL, so they only build everything and run the unit tests.
- **Web build:** `cargo xtask web --release --game games/demo`, with `wasm-bindgen-cli` pinned to the version in `Cargo.lock`. The result is uploaded as the `web-demo` artifact.

Linux builds need `pkg-config libasound2-dev libdbus-1-dev libx11-dev libxi-dev libgl1-mesa-dev`. kira's default features pull in D-Bus, through cpal's realtime audio thread.

## Conventions

- Rust edition 2024. The toolchain is pinned in `rust-toolchain.toml` (1.98.1, with clippy, rustfmt and the wasm32 target). rustup installs it on first use, and CI installs the same version. Bump it on purpose, and fix any new clippy lints in the same change.
- Game scripts are **Lua 5.5**, not Luau. That means no type annotations, backtick string interpolation, `continue`, or compound assignment. `goto`, integer subtypes, and `<const>`/`<close>` are available.
- Don't expose luars' `io` or `os` libraries to game scripts wholesale. `os.exit` calls `std::process::exit` and `os.execute` spawns processes. Open a vetted subset of libraries and route file access through the engine's own API.
- Script errors must not bring down the host with a Rust panic. Surface them with a Lua traceback.
- Before adding a dependency, check that it builds for `wasm32` and still runs under `cargo xtask serve`. wasm-bindgen imports are handled by the loader. Any other non-`env` import module is not.
