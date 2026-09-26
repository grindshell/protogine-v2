-- A tour of the core lifecycle, pg.graphics, input, audio and pg.math.

local shapes = require("shapes")
local scenery = require("scenery")
local controls = require("controls")

local time = 0
local sprite, frames
local title_font, body_font

function pg.load(args)
  print("demo loaded with " .. #args .. " argument(s)")

  pg.graphics.setBackgroundColor(0.1, 0.1, 0.15)
  pg.graphics.setDefaultFilter("nearest")

  sprite = pg.graphics.newImage("sprites.png")
  frames = {
    pg.graphics.newQuad(0, 0, 16, 16),
    pg.graphics.newQuad(16, 0, 16, 16),
  }
  title_font = pg.graphics.newFont(32)
  body_font = pg.graphics.getFont()

  scenery.load()
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
  g.print("protogine", 20, 16)

  g.setFont(body_font)
  g.setColor(0.7, 0.7, 0.8)
  g.print(("fps %d   %dx%d"):format(pg.timer.getFPS(), g.getDimensions()), 20, 56)

  shapes.draw(20, 100)
  scenery.draw(time)

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
