-- A tour of the core lifecycle, pg.graphics (including canvases and shaders), input, audio,
-- pg.math, pg.filesystem and pg.system.

local shapes = require("shapes")
local scenery = require("scenery")
local retro = require("retro")
local controls = require("controls")

local time = 0
local runs, platform
local sprite, frames
local title_font, body_font

-- A vertex shader that waves the title. VertexPosition is in screen units, so the wave runs
-- across the screen whatever the transform.
local wave
local WAVE = [[
extern number time;

vec4 position(mat4 transform_projection, vec4 vertex_position)
{
    vertex_position.y += 4.0 * sin(vertex_position.x / 30.0 + time * 3.0);
    return transform_projection * vertex_position;
}
]]

function pg.load(args)
  print("demo loaded with " .. #args .. " argument(s)")

  -- Count runs in the save directory. `read` returns nil and a message the first time.
  runs = (tonumber((pg.filesystem.read("runs.txt"))) or 0) + 1
  pg.filesystem.write("runs.txt", tostring(runs))

  local power, percent = pg.system.getPowerInfo()
  if percent then
    power = ("%s %d%%"):format(power, percent)
  end
  platform = ("%s, %d cores, power %s"):format(
    pg.system.getOS(), pg.system.getProcessorCount(), power)

  pg.graphics.setBackgroundColor(0.1, 0.1, 0.15)
  pg.graphics.setDefaultFilter("nearest")

  sprite = pg.graphics.newImage("sprites.png")
  frames = {
    pg.graphics.newQuad(0, 0, 16, 16),
    pg.graphics.newQuad(16, 0, 16, 16),
  }
  title_font = pg.graphics.newFont(32)
  body_font = pg.graphics.getFont()
  wave = pg.graphics.newShader(WAVE)

  scenery.load()
  retro.load()
  controls.load()
end

function pg.update(dt)
  time = time + dt
  controls.update(dt)
end

function pg.draw()
  local g = pg.graphics
  local width = g.getWidth()

  g.setFont(title_font)
  g.setColor(1, 1, 1)
  wave:send("time", time)
  g.setShader(wave)
  g.print("protogine", 20, 16)
  g.setShader()

  g.setFont(body_font)
  g.setColor(0.7, 0.7, 0.8)
  local height = g.getHeight()
  g.print(("fps %d   %dx%d   run %d   %s"):format(
    pg.timer.getFPS(), width, height, runs, platform), 20, 56)

  shapes.draw(20, 100)
  scenery.draw(time)
  retro.draw(695, 100, time)

  -- A sprite spinning around its center, animated with quads.
  g.push()
  g.translate(620, 190)
  g.rotate(time)
  g.setColor(1, 1, 1)
  local frame = frames[math.floor(time * 2) % 2 + 1]
  g.draw(sprite, frame, 0, 0, 0, 6, 6, 8, 8)
  g.pop()

  g.setColor(1, 1, 1)
  g.printf(
    "This paragraph is wrapped by printf to fit in 360 pixels and centered within that width.",
    20, 330, 360, "center")
  g.printf("Right-aligned in the window.", 0, 400, width - 20, "right")

  controls.draw(20, 425)
end

function pg.resize(w, h)
  print(("resized to %dx%d"):format(w, h))
end
