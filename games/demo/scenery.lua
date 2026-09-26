-- pg.math: scrolling hills made from noise (concave, so they're triangulated to fill them),
-- stars placed by a seeded generator, and a Bézier curve whose control points drift.

local scenery = {}

local left, right, top, bottom = 420, 780, 290, 395
local stars = {}
local anchors = { 430, 330, 520, 290, 680, 370, 770, 320 }
local curve
local marker = pg.math.newTransform()

function scenery.load()
  -- A fixed seed puts the stars in the same places every run.
  local rng = pg.math.newRandomGenerator(2024)
  for i = 1, 40 do
    stars[i] = { x = rng:random(left, right), y = rng:random(top, top + 60) }
  end
  curve = pg.math.newBezierCurve(anchors)
end

function scenery.draw(time)
  local g = pg.graphics
  local m = pg.math

  g.setPointSize(2)
  for i, star in ipairs(stars) do
    g.setColor(1, 1, 1, m.noise(i, time))
    g.points(star.x, star.y)
  end

  -- The outline runs along the bottom, then back along the ridge.
  local hills = { right, bottom, left, bottom }
  for x = left, right, 12 do
    hills[#hills + 1] = x
    hills[#hills + 1] = bottom - 15 - 45 * m.noise(x / 90 + time * 0.3)
  end
  g.setColor(m.colorFromBytes(46, 94, 70))
  for _, triangle in ipairs(m.triangulate(hills)) do
    g.polygon("fill", triangle)
  end

  for i = 1, curve:getControlPointCount() do
    local drift = 40 * (m.noise(i * 7.3, time * 0.4) - 0.5)
    curve:setControlPoint(i, anchors[2 * i - 1], anchors[2 * i] + drift)
  end
  g.setColor(1, 1, 1, 0.2)
  g.setLineWidth(1)
  g.line(curve:render(0))
  g.setColor(1, 0.8, 0.3)
  g.setLineWidth(2)
  g.line(curve:render())

  -- A spinning square rides along the curve.
  local x, y = curve:evaluate(time * 0.2 % 1)
  marker:setTransformation(x, y, time * 3, 1, 1, 5, 5)
  g.push()
  g.applyTransform(marker)
  g.rectangle("line", 0, 0, 10, 10)
  g.pop()
end

return scenery
