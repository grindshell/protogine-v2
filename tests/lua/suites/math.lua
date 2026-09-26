-- pg.math: random numbers, noise, colors, polygons, Bézier curves and transforms.

local t = require("harness")

local suite = {}

-- The area of a polygon given as a flat list of coordinates.
local function area(coords)
  local sum = 0
  for i = 1, #coords, 2 do
    local j = i + 2 > #coords and 1 or i + 2
    sum = sum + coords[i] * coords[j + 1] - coords[j] * coords[i + 1]
  end
  return math.abs(sum) / 2
end

local function random_numbers(m)
  local rng = m.newRandomGenerator()
  t.check("RandomGenerator type", rng:type() == "RandomGenerator")
  local low, high = rng:getSeed()
  t.check("default seed", low == 0xCBBF7A44 and high == 0x0139408D, low .. ", " .. high)

  local in_range = true
  for _ = 1, 1000 do
    local r, d, e = rng:random(), rng:random(6), rng:random(-3, 3)
    in_range = in_range and r >= 0 and r < 1 and d >= 1 and d <= 6 and e >= -3 and e <= 3
      and math.type(d) == "integer" and math.type(e) == "integer"
  end
  t.check("random ranges", in_range)

  local a, b = m.newRandomGenerator(42), m.newRandomGenerator(42)
  t.check("same seed, same numbers", a:random() == b:random() and a:random(100) == b:random(100))
  local state = a:getState()
  t.check("state format", state:match("^0x%x+$") ~= nil and #state == 18, state)
  local expected = a:random()
  b:setState(state)
  t.check("setState", b:random() == expected)
  b:setSeed(7, 1)
  local lo, hi = b:getSeed()
  t.check("setSeed(low, high)", lo == 7 and hi == 1, lo .. ", " .. hi)
  t.check("split seeds", m.newRandomGenerator(7, 1):random() == m.newRandomGenerator(7 + (1 << 32)):random())
  b:setSeed(5)
  lo, hi = b:getSeed()
  t.check("setSeed(seed)", lo == 5 and hi == 0)

  m.setRandomSeed(123)
  local first = m.random()
  m.setRandomSeed(123)
  t.check("setRandomSeed", m.random() == first)
  t.check("the global generator is a RandomGenerator", m.newRandomGenerator(123):random() == first)
  lo, hi = m.getRandomSeed()
  t.check("getRandomSeed", lo == 123 and hi == 0)
  local global_state = m.getRandomState()
  local roll = m.random(1000)
  m.setRandomState(global_state)
  t.check("setRandomState", m.random(1000) == roll)

  m.setRandomSeed(1)
  local sum = 0
  for _ = 1, 2000 do
    sum = sum + m.randomNormal(2, 10)
  end
  t.check("randomNormal", t.near(sum / 2000, 10, 0.3), sum / 2000)

  t.errors("empty interval", "bad argument #2 to 'random' (interval is empty)", m.random, 5, 1)
  t.errors("empty interval below 1", "bad argument #1 to 'random' (interval is empty)", m.random, 0)
  t.errors("method arguments skip self", "bad argument #1 to 'random' (number expected, got string)",
    rng.random, rng, "x")
  t.errors("bad self", "calling 'random' on bad self (RandomGenerator expected, got number)", rng.random, 5)
  t.errors("bad state", "bad argument #1 to 'setState' (invalid random state 'nope')", rng.setState, rng, "nope")
  t.errors("bad seed", "bad argument #1 to 'setRandomSeed' (invalid random seed)", m.setRandomSeed, math.huge)
end

local function noise(m)
  t.check("noise on the lattice", m.noise(3) == 0.5 and m.noise(0, 0) == 0.5 and m.noise(1, 2, 3) == 0.5
    and m.noise(1, 2, 3, 4) == 0.5)
  local in_range = true
  for i = 1, 500 do
    local x = i * 0.173
    for _, n in ipairs({ m.noise(x), m.noise(x, -x), m.noise(x, 1, x), m.noise(x, x, 2, -x) }) do
      in_range = in_range and n >= 0 and n <= 1
    end
  end
  t.check("noise range", in_range)
  t.check("noise is repeatable", m.noise(0.3, 0.7) == m.noise(0.3, 0.7))
  t.check("dimensions differ", m.noise(0.3) ~= m.noise(0.3, 0.2))
  t.errors("noise needs a number", "bad argument #1 to 'noise' (number expected, got no value)", m.noise)
end

local function colors(m)
  local r, g, b = m.gammaToLinear(0.5, 1, 0)
  t.check("gammaToLinear", t.near(r, 0.214041, 1e-6) and g == 1 and b == 0, r)
  t.check("linearToGamma", t.near(m.linearToGamma(r), 0.5, 1e-9))
  t.check("alpha stays linear", select(4, m.gammaToLinear({ 0.5, 0.5, 0.5, 0.5 })) == 0.5)
  t.check("gamma clamps", m.gammaToLinear(2) == 1 and select("#", m.gammaToLinear(2)) == 1)
  local r8, g8, b8, a8 = m.colorToBytes(1, 0.5, 0, 2)
  t.check("colorToBytes", r8 == 255 and g8 == 128 and b8 == 0 and a8 == 255 and math.type(r8) == "integer")
  t.check("colorToBytes without alpha", select("#", m.colorToBytes({ 0, 0, 0 })) == 3)
  local rf, gf = m.colorFromBytes({ 255, 51, 0 })
  t.check("colorFromBytes", rf == 1 and t.near(gf, 0.2, 1e-9))
  t.errors("colorToBytes needs three", "bad argument #3 to 'colorToBytes' (number expected, got no value)",
    m.colorToBytes, 1, 1)
  t.errors("gamma needs a number", "bad argument #1 to 'gammaToLinear' (number expected, got no value)",
    m.gammaToLinear)
end

local function polygons(m)
  local square = { 0, 0, 10, 0, 10, 10, 0, 10 }
  local ell = { 0, 0, 20, 0, 20, 10, 10, 10, 10, 20, 0, 20 }
  t.check("isConvex", m.isConvex(square) and m.isConvex(table.unpack(square)) and not m.isConvex(ell))
  local triangles = m.triangulate(ell)
  local total = 0
  for _, triangle in ipairs(triangles) do
    total = total + area(triangle)
  end
  t.check("triangulate", #triangles == 4 and #triangles[1] == 6 and total == 300, #triangles .. " " .. total)
  t.errors("triangulate too few", "triangulate: need at least three vertices", m.triangulate, 0, 0, 1, 1)
  t.errors("triangulate a tangle", "triangulate: cannot triangulate the polygon",
    m.triangulate, 30, 20, 0, 30, 30, 10, 10, 30, 30, 30)
end

local function curves(m)
  local curve = m.newBezierCurve({ 0, 0, 0, 10, 10, 10, 10, 0 })
  t.check("BezierCurve type", curve:type() == "BezierCurve")
  t.check("degree", curve:getDegree() == 3 and curve:getControlPointCount() == 4)
  local x, y = curve:evaluate(0.5)
  t.check("evaluate", x == 5 and y == 7.5, x .. ", " .. y)
  x, y = curve:getControlPoint(-1)
  t.check("negative indices", x == 10 and y == 0)
  local points = curve:render()
  t.check("render", #points == 194 and points[1] == 0 and points[194] == 0, #points)
  t.check("renderSegment", #curve:renderSegment(0, 0.5, 3) == 26)
  local sx, sy = curve:getSegment(0.25, 0.75):evaluate(0.5)
  t.check("getSegment", t.near(sx, 5, 1e-9) and t.near(sy, 7.5, 1e-9))
  local derivative = curve:getDerivative()
  x, y = derivative:getControlPoint(1)
  t.check("getDerivative", derivative:getDegree() == 2 and x == 0 and y == 30)

  curve:insertControlPoint(20, 20)
  x, y = curve:getControlPoint(-2)
  t.check("insertControlPoint goes before the last", curve:getControlPointCount() == 5 and x == 20 and y == 20)
  curve:removeControlPoint(-2)
  curve:setControlPoint(1, 1, 1)
  curve:translate(1, 2)
  x, y = curve:getControlPoint(1)
  t.check("edit points", curve:getControlPointCount() == 4 and x == 2 and y == 3)
  curve:scale(2, 2, 3)
  x, y = curve:getControlPoint(1)
  t.check("scale about a point", x == 2 and y == 3)
  curve:rotate(math.pi, 2, 3)
  x, y = curve:getControlPoint(-1)
  t.check("rotate about a point", t.near(x, -16) and t.near(y, 5), x .. ", " .. y)

  t.errors("evaluate range", "evaluate: the curve parameter must be between 0 and 1", curve.evaluate, curve, 2)
  t.errors("render depth", "bad argument #1 to 'render' (depth must be at most 16)", curve.render, curve, 17)
  t.errors("integer index", "bad argument #1 to 'getControlPoint' (number has no integer representation)",
    curve.getControlPoint, curve, 1.5)
  local empty = m.newBezierCurve()
  t.errors("empty curve", "getControlPoint: the curve has no control points", empty.getControlPoint, empty, 1)
  t.errors("too few points", "render: the curve needs at least two control points", empty.render, empty)
end

local function transforms(m)
  local tr = m.newTransform()
  t.check("Transform type", tr:type() == "Transform")
  t.check("chaining", tr:translate(10, 20):scale(2) == tr)
  local x, y = tr:transformPoint(1, 1)
  t.check("transformPoint", x == 12 and y == 22 and math.type(x) == "integer", x .. ", " .. y)
  x, y = tr:inverseTransformPoint(12, 22)
  t.check("inverseTransformPoint", t.near(x, 1) and t.near(y, 1))
  x, y = tr:inverse():transformPoint(12, 22)
  t.check("inverse", t.near(x, 1) and t.near(y, 1))
  local copy = tr:clone()
  tr:reset():rotate(math.pi / 2)
  x, y = tr:transformPoint(1, 0)
  t.check("reset and rotate", t.near(x, 0) and t.near(y, 1), x .. ", " .. y)
  x = copy:transformPoint(1, 1)
  t.check("clone is independent", x == 12)

  x, y = m.newTransform(100, 50, 0, 2, 2, 5, 5):transformPoint(5, 5)
  t.check("newTransform with an origin", x == 100 and y == 50)
  x, y = m.newTransform(0, 0, 0, 1, 1, 0, 0, 1, 0):transformPoint(0, 10)
  t.check("newTransform with shear", x == 10 and y == 10)
  x, y = m.newTransform():setTransformation(3, 4):transformPoint(0, 0)
  t.check("setTransformation", x == 3 and y == 4)
  x, y = m.newTransform():shear(1, 0):transformPoint(0, 10)
  t.check("shear", x == 10 and y == 10)

  local e = { m.newTransform(3, 4):getMatrix() }
  t.check("getMatrix is row-major", #e == 16 and e[4] == 3 and e[8] == 4 and e[16] == 1)
  local set = m.newTransform()
  x, y = set:setMatrix(1, 0, 0, 7, 0, 1, 0, 8, 0, 0, 1, 0, 0, 0, 0, 1):transformPoint(0, 0)
  t.check("setMatrix numbers", x == 7 and y == 8)
  x, y = set:setMatrix("column", { 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 0, 1 }):transformPoint(0, 0)
  t.check("setMatrix column-major table", x == 5 and y == 6)
  x, y = set:setMatrix({ { 1, 0, 0, 2 }, { 0, 1, 0, 3 }, { 0, 0, 1, 0 }, { 0, 0, 0, 1 } }):transformPoint(0, 0)
  t.check("setMatrix nested tables", x == 2 and y == 3)
  t.check("isAffine2DTransform", set:isAffine2DTransform())
  set:setMatrix(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.5, 0, 0, 1)
  t.check("projections aren't affine", not set:isAffine2DTransform())

  local move, grow = m.newTransform(10, 0), m.newTransform(0, 0, 0, 2)
  x, y = (move * grow):transformPoint(1, 1)
  t.check("multiply", x == 12 and y == 2)
  x, y = move:clone():apply(grow):transformPoint(1, 1)
  t.check("apply", x == 12 and y == 2)
  t.check("multiply by a number", not pcall(function() return move * 2 end))

  t.errors("bad layout",
    "bad argument #1 to 'setMatrix' (invalid matrix layout 'diagonal', expected one of 'row', 'column')",
    set.setMatrix, set, "diagonal", {})
  t.errors("too few elements", "bad argument #16 to 'setMatrix' (number expected, got no value)",
    set.setMatrix, set, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)
  t.errors("short table", "bad argument #1 to 'setMatrix' (matrix table must hold 16 numbers)",
    set.setMatrix, set, { 1, 2, 3 })
  t.errors("apply a non-Transform", "bad argument #1 to 'apply' (Transform expected, got number)",
    set.apply, set, 5)
end

function suite.run()
  local m = pg.math
  random_numbers(m)
  noise(m)
  colors(m)
  polygons(m)
  curves(m)
  transforms(m)
  t.finish()
end

return suite
