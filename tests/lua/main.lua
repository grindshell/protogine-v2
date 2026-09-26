-- Lua regression tests. Run one suite with `protogine-v2 tests/lua <suite> [args...]`;
-- tests/lua.rs runs them all under `cargo test`. See harness.lua for the output format.

function pg.load(args)
  local name = args[1]
  local ok, suite = pcall(require, "suites." .. tostring(name))
  if not ok then
    error("unknown suite '" .. tostring(name) .. "': " .. tostring(suite), 0)
  end
  suite.run(args)
end
