-- Loaded with require("shapes") to exercise the module loader.

local shapes = {}

function shapes.draw(x, y)
  local g = pg.graphics

  g.setColor(0.9, 0.3, 0.3)
  g.rectangle("fill", x, y, 100, 70)

  g.setLineWidth(3)
  g.setColor(0.3, 0.9, 0.4)
  g.rectangle("line", x + 120, y, 100, 70)

  g.setColor(0.3, 0.5, 1)
  g.circle("fill", x + 280, y + 35, 35)
  g.circle("line", x + 360, y + 35, 35)

  g.setColor(1, 0.8, 0.2)
  g.ellipse("fill", x + 470, y + 35, 50, 25)

  g.setColor(0.8, 0.4, 1)
  g.polygon("fill", x, y + 180, x + 50, y + 100, x + 100, y + 180)
  g.polygon("line", { x + 120, y + 180, x + 170, y + 100, x + 220, y + 180 })

  g.setColor(1, 1, 1, 0.6)
  g.line(x + 240, y + 180, x + 270, y + 100, x + 300, y + 180, x + 330, y + 100)

  g.setPointSize(6)
  g.setColor(1, 1, 1)
  g.points(x + 360, y + 110, x + 380, y + 140, x + 400, y + 170)
end

return shapes
