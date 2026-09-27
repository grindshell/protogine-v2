-- Canvases, blend modes and shaders: a tiny scene drawn at 24x16 pixels and scaled up 4x, with
-- a glow drawn in "add" mode. The canvas is translucent, so it's drawn with premultiplied alpha,
-- through a scanline shader loaded from crt.glsl.

local retro = {}

local canvas, crt

function retro.load()
  -- main.lua sets the default filter to "nearest", so the scaled-up pixels stay sharp.
  canvas = pg.graphics.newCanvas(24, 16)
  crt = pg.graphics.newShader("crt.glsl")
end

function retro.draw(x, y, time)
  local g = pg.graphics

  canvas:renderTo(function()
    g.clear()
    g.setColor(0.2, 0.3, 0.6, 0.6)
    g.rectangle("fill", 0, 0, 24, 13)
    g.setColor(0.2, 0.5, 0.3)
    g.rectangle("fill", 0, 13, 24, 3)

    local bx = 12 + 8 * math.sin(time * 1.7)
    local by = 6 + 3 * math.cos(time * 2.3)
    g.setBlendMode("add")
    g.setColor(1, 0.6, 0.2, 0.3)
    for radius = 5, 1, -1 do
      g.circle("fill", bx, by, radius)
    end
    g.setBlendMode("alpha")
  end)

  crt:send("time", time)
  g.setShader(crt)
  g.setColor(1, 1, 1)
  g.setBlendMode("alpha", "premultiplied")
  g.draw(canvas, x, y, 0, 4)
  g.setBlendMode("alpha")
  g.setShader()

  g.setColor(0.7, 0.7, 0.8)
  g.print("canvas x4, shader", x, y + 70)
end

return retro
