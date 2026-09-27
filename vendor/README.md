# Vendored dependencies

Both are published crates with a few changes, recorded as patches next to them.

## luars

`luars/` is [luars](https://crates.io/crates/luars) 0.26.3, the package published to crates.io from [CppCXY/lua-rs](https://github.com/CppCXY/lua-rs) commit `2f8c204`. `[patch.crates-io]` in the root `Cargo.toml` uses it in place of the crates.io release. It's MIT-licensed; `luars/LICENSE` is upstream's license file.

It has two changes from the published package:

- **A compiler fix**, in `luars-concat-jump-target.patch`. luars merged a concatenation into the previous `CONCAT`, and folded a string constant into the previous `LOADK`, even when the current instruction was a jump target. The path that jumped there then lost part of the string: `"A" .. (yes and "C" or ("w" .. w))` gave `"A"`, and so did `(yes and "A" or "B") .. "C"`. The patch adds the jump-target check that C Lua's `previousinstruction()` makes, plus a regression test in `src/test/test_operators.rs`. `tests/lua/suites/lifecycle.lua` checks the fix from game code.
- **`[lints.rust] warnings = "allow"`** in `luars/Cargo.toml`. A path dependency's warnings aren't capped like a registry dependency's, and luars has a few dead-code warnings on wasm32.

The root workspace excludes `luars/`, so `cargo fmt`, `cargo clippy --workspace` and `cargo test --workspace` leave it alone. Don't edit it except to apply patches recorded here.

### Updating or removing it

Once a luars release includes the fix (upstream `main` didn't as of 2026-09-26), delete `vendor/`, remove the `[patch.crates-io]` section and the `exclude` entry from the root `Cargo.toml`, and bump `luars` there. Keep the concatenation checks in the lifecycle suite.

To move to a newer luars that still needs the fix, copy the new package from `~/.cargo/registry/src/*/luars-<version>/`, deleting `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.toml.orig` and `Cargo.lock`. Apply the patch with `git apply -p3 --directory=vendor/luars vendor/luars-concat-jump-target.patch`, and re-add the `[lints.rust]` table.

## macroquad

`macroquad/` is [macroquad](https://crates.io/crates/macroquad) 0.4.16, the package published to crates.io from [not-fl3/macroquad](https://github.com/not-fl3/macroquad) commit `5e9b5ca`. `[patch.crates-io]` in the root `Cargo.toml` uses it in place of the crates.io release. It's licensed under MIT or Apache-2.0; `macroquad/LICENSE-MIT` and `macroquad/LICENSE-APACHE` are upstream's license files.

It keeps only what builds the library: `src/`, `Cargo.toml`, `README.md` and the licenses. `examples/`, `tests/`, `js/`, `.github/`, `.gitignore`, `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.toml.orig` and `Cargo.lock` are left out.

`macroquad-shaders.patch` holds every change from the published package, for `pg.graphics` shaders (see `src/shader.rs`):

- **`pg_` names.** The attributes, uniforms and textures that macroquad's shaders share (`position`, `texcoord`, `color0`, `normal`, `Model`, `Projection`, `_Time`, `Texture` and `_ScreenTexture`) get a `pg_` prefix, in `src/quad_gl.rs`, `src/ui.rs` and the docs in `src/material.rs`. A game's shader code is compiled together with them, and Love2D's vertex entry point is a function named `position`, which would clash with the attribute.
- **More materials.** macroquad kept at most 32 pipelines, and panicked when a material needed another. Each shader makes one per blend mode it draws with, so the limit is 1024, and running out returns `Error::UnknownError("too many materials")` from `load_material` instead.
- **`Cargo.toml`** drops the `[[example]]` and `[[test]]` targets, the Android metadata and the dependency profile, which Cargo ignores in a dependency anyway. It adds `[lints.rust] warnings = "allow"`, as for luars.

The root workspace excludes `macroquad/` too. Don't edit it except to apply patches recorded here.

### Updating it

To move to a newer macroquad, copy the new package from `~/.cargo/registry/src/*/macroquad-<version>/` into `vendor/macroquad/`, keeping only the files listed above, then apply the patch with `git apply vendor/macroquad-shaders.patch` and fix whatever no longer applies. Check the engine's own shaders in `src/graphics.rs` and `src/shader.rs` against macroquad's, and update `web/gl.js` if the miniquad version changes (see AGENTS.md).
