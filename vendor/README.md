# Vendored dependencies

## luars

`luars/` is [luars](https://crates.io/crates/luars) 0.26.3, the package published to crates.io from [CppCXY/lua-rs](https://github.com/CppCXY/lua-rs) commit `2f8c204`. `[patch.crates-io]` in the root `Cargo.toml` uses it in place of the crates.io release. It's MIT-licensed; `luars/LICENSE` is upstream's license file.

It has two changes from the published package:

- **A compiler fix**, in `luars-concat-jump-target.patch`. luars merged a concatenation into the previous `CONCAT`, and folded a string constant into the previous `LOADK`, even when the current instruction was a jump target. The path that jumped there then lost part of the string: `"A" .. (yes and "C" or ("w" .. w))` gave `"A"`, and so did `(yes and "A" or "B") .. "C"`. The patch adds the jump-target check that C Lua's `previousinstruction()` makes, plus a regression test in `src/test/test_operators.rs`. `tests/lua/suites/lifecycle.lua` checks the fix from game code.
- **`[lints.rust] warnings = "allow"`** in `luars/Cargo.toml`. A path dependency's warnings aren't capped like a registry dependency's, and luars has a few dead-code warnings on wasm32.

The root workspace excludes `luars/`, so `cargo fmt`, `cargo clippy --workspace` and `cargo test --workspace` leave it alone. Don't edit it except to apply patches recorded here.

### Updating or removing it

Once a luars release includes the fix (upstream `main` didn't as of 2026-09-26), delete `vendor/`, remove the `[patch.crates-io]` section and the `exclude` entry from the root `Cargo.toml`, and bump `luars` there. Keep the concatenation checks in the lifecycle suite.

To move to a newer luars that still needs the fix, copy the new package from `~/.cargo/registry/src/*/luars-<version>/`, deleting `.cargo-ok`, `.cargo_vcs_info.json`, `Cargo.toml.orig` and `Cargo.lock`. Apply the patch with `git apply -p3 --directory=vendor/luars vendor/luars-concat-jump-target.patch`, and re-add the `[lints.rust]` table.
