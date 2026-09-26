-- Callbacks, arguments, quitting, and the sandboxed standard library.
-- tests/lua.rs runs this suite with the extra arguments "one" and "two".

local t = require("harness")

local suite = {}

function suite.run(args)
  t.check("args", #args == 3 and args[2] == "one" and args[3] == "two", table.concat(args, ","))

  -- Sandbox
  t.check("no io", io == nil)
  t.check("os restricted", os.execute == nil and os.exit == nil and os.getenv == nil
    and type(os.time) == "function" and type(os.clock) == "function" and type(os.date) == "function")
  t.check("debug restricted", debug.getinfo == nil and type(debug.traceback) == "function")
  local chunk, err = load("\27Lua", "bin", "b")
  t.check("binary chunks rejected", chunk == nil, err)
  t.check("load with env", load("return x", "env", "t", { x = 7 })() == 7)
  t.check("load keeps globals", load("return type(pg)")() == "table")

  -- Files and modules
  t.check("require", require("fixtures.answer").answer == 42)
  t.check("require caches", require("fixtures.answer") == require("fixtures.answer") and answer_loads == 1,
    answer_loads)
  t.check("package.loaded", package.loaded["fixtures.answer"] == require("fixtures.answer"))
  local pkg = require("fixtures.pkg")
  t.check("require init.lua", pkg.name == "fixtures.pkg" and pkg.path == "fixtures/pkg/init.lua", pkg.path)
  t.errors("require missing",
    "module 'nope.mod' not found:\n\tno file 'nope/mod.lua'\n\tno file 'nope/mod/init.lua'", require, "nope.mod")
  local fn = loadfile("fixtures/answer.lua")
  t.check("loadfile", fn ~= nil and fn().answer == 42)
  local missing, message = loadfile("nope.lua")
  t.check("loadfile missing", missing == nil and message == "file not found: 'nope.lua'", message)
  t.errors("dofile missing", "file not found: 'nope.lua'", dofile, "nope.lua")
  t.errors("paths can't escape", "invalid path: '../main.lua'", dofile, "../main.lua")

  -- Timer and window
  t.check("getTime", type(pg.timer.getTime()) == "number")
  t.check("getFPS is an integer", math.type(pg.timer.getFPS()) == "integer")
  t.check("getDPIScale", pg.window.getDPIScale() > 0, pg.window.getDPIScale())
  t.check("not fullscreen", pg.window.getFullscreen() == false)
end

local frames = 0

function pg.update(dt)
  frames = frames + 1
  if frames == 1 then
    t.check("dt matches getDelta", dt >= 0 and dt == pg.timer.getDelta(), dt)
    t.check("dt is capped", dt <= 10)
  elseif frames == 3 then
    pg.event.quit()
  end
end

local draws = 0

function pg.draw()
  draws = draws + 1
end

-- The first quit is canceled and asks to quit again; the second goes through.
local quits = 0

function pg.quit()
  quits = quits + 1
  if quits == 1 then
    pg.event.quit()
    return true
  end
  t.check("quit can be canceled", quits == 2 and frames >= 4, frames)
  t.check("draw runs every frame", draws == frames, draws .. " draws, " .. frames .. " frames")
  t.summary()
  return false
end

return suite
