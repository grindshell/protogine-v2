# protogine-v2

Protogine is a 2D game framework in the spirit of [LÖVE (Love2D)](https://love2d.org/): games are written in **Lua**, and a Rust host runs them. The host uses **macroquad** for the window, main loop, rendering and input, **luars** for the Lua runtime, and **kira** for audio. The Rust binary is the engine. A game is a folder of Lua scripts and assets that the engine loads and runs.

**Status:** the core lifecycle, `pg.graphics` and input (`pg.keyboard`, `pg.mouse`, `pg.touch`) work on native and web; `games/demo` exercises them. Audio isn't implemented yet. See [docs/api.md](docs/api.md) for exactly what is implemented.

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
- **Errors carry no message.** `LuaError` is a bare enum, and the message lives in VM state. Call `lua.get_error_message(e)` right away; it can only be read once.
- **Typed callbacks** (`create_function`) can't take a variable number of arguments. Their argument errors don't name the function, and if one returns `LuaResult`, the error message is lost ("Runtime Error"). `pg.*` functions therefore use raw callbacks through `api::Args` (see `src/api/mod.rs`).
- `LuaTable`, `LuaFunction` and `Value` hold raw pointers into the VM, so drop them before the `Lua` that owns them. `Game` declares `lua` as its last field for this reason.
- Async support comes through `register_async_function` and the `LuaAsyncApi` trait (`eval_async`, `call_async*`). Whether these work under macroquad's executor is still unverified.
- `Lua` is `!Send` by default. The `unsafe-send` feature only adds unchecked `unsafe impl Send`. Don't enable it, because macroquad's loop is single-threaded anyway.
- Source code is in `~/.cargo/registry/src/*/luars-*/src/`, and `src/lua_api/mod.rs` has the full trait surface.

## Architecture

- **Lua API:** specified in [docs/api.md](docs/api.md). It's Love2D-shaped under a single global, `pg`, with callbacks like `pg.update(dt)` and modules like `pg.graphics`. This Lua-facing API is the product. Update the doc in the same change as any API change, and record every divergence from Love2D there.
- **Files are mounted up front:** native reads the game directory or `.zip`, and the web fetches `game.zip` before any Lua runs. Every file API, and `require`, is synchronous on every platform.
- **Audio (planned):** the host owns kira's `AudioManager`. Sounds and handles reach Lua as userdata.

Source layout:

| Path | Role |
| --- | --- |
| `src/main.rs` | Entry point. Natively it mounts the game and runs `conf.lua` before opening the window. On the web it opens the window, then fetches `game.zip`. |
| `src/engine.rs` | The main loop, a state machine over the no-game, game, error and blank screens. It also handles quitting, and drains the input queue every frame. |
| `src/game.rs` | `Game`, which owns the Lua state and runs the lifecycle (`conf.lua`, `main.lua`, `pg.*` callbacks). `Host` is the engine state that `pg.*` functions share through `Rc<RefCell<_>>`. |
| `src/api/` | The `pg.*` bindings, one file per module. `mod.rs` has the `Args` helper. `prelude.lua` runs first in every game and sets up `require`, `print`, the restricted `os`/`debug`, and `invoke` (`xpcall` plus a traceback). |
| `src/graphics.rs` | The engine side of `pg.graphics`: state, the transform stack, shapes, images and text on top of macroquad. It has no Lua dependency. |
| `src/input.rs` | The engine side of input. It reads macroquad's event queue (one subscriber for the whole run, since macroquad can't unsubscribe), turns it into Love2D-style events, and tracks the state the getters report. It normalizes wheel units, counts multi-clicks, and turns the primary touch into mouse events. It has no Lua dependency, and its event logic is unit-tested. |
| `src/vfs.rs` | The game's read-only, case-sensitive filesystem: a directory or an in-memory zip. |
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

In `web/index.html`, a gl.js plugin handles the wasm-bindgen side:

- `register_plugin` merges the wasm-bindgen imports into gl.js's import object.
- `on_init` calls `attachBindgen(wasm_exports)`, which points the glue at the instance and initializes the externref table.

gl.js then calls `main`. `__wbindgen_start` is deliberately never called, because for a bin crate it also runs `main`.

Requirements and notes:

- `wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock`, currently 0.2.129. Install it with `cargo install wasm-bindgen-cli --version 0.2.129 --locked`. Reinstall whenever `Cargo.lock` bumps wasm-bindgen.
- `web/gl.js` is vendored from miniquad at the commit that miniquad 0.4.11 was published from (`4f13d4a`). Update it whenever the miniquad version in `Cargo.lock` changes.
- Browsers keep an `AudioContext` suspended until a user gesture. Resuming audio on the first click or keypress still needs handling.

## Commands

```sh
cargo run -- games/demo      # run a game natively (a directory or .zip); no argument shows the no-game screen
cargo build --release
cargo clippy --workspace --all-targets
cargo fmt --all
cargo test
cargo xtask web [--release] [--game DIR]                # web build into target/web/, packing DIR as game.zip
cargo xtask serve [--release] [--game DIR] [--port N]   # web build, then serve at http://127.0.0.1:8080/
```

The repo is a Cargo workspace: the root package is the engine, and `xtask/` holds the build tooling. `.claude/launch.json` has a `web` config that runs `cargo xtask serve --game games/demo`.

To check API behavior quickly, write a scratch game whose `pg.load` checks results with `pcall` and `print`s them, then calls `pg.event.quit()`. Run it natively and read stdout; the process exits by itself.

## Conventions

- Rust edition 2024.
- Game scripts are **Lua 5.5**, not Luau. That means no type annotations, backtick string interpolation, `continue`, or compound assignment. `goto`, integer subtypes, and `<const>`/`<close>` are available.
- Don't expose luars' `io` or `os` libraries to game scripts wholesale. `os.exit` calls `std::process::exit` and `os.execute` spawns processes. Open a vetted subset of libraries and route file access through the engine's own API.
- Script errors must not bring down the host with a Rust panic. Surface them with a Lua traceback.
- Before adding a dependency, check that it builds for `wasm32` and still runs under `cargo xtask serve`. wasm-bindgen imports are handled by the loader. Any other non-`env` import module is not.
