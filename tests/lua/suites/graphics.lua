-- pg.graphics: state, argument checking, images, quads, fonts and transforms.

local t = require("harness")

local suite = {}

function suite.run()
  local g = pg.graphics

  -- State
  g.setColor(0.25, 0.5, 0.75)
  local r, gr, b, a = g.getColor()
  t.check("getColor", r == 0.25 and gr == 0.5 and b == 0.75 and a == 1, table.concat({ r, gr, b, a }, ","))
  g.setColor({ 1, 0, 0, 0.5 })
  r, gr, b, a = g.getColor()
  t.check("setColor table", r == 1 and gr == 0 and a == 0.5)
  t.errors("setColor type", "bad argument #1 to 'setColor' (number expected, got string)", g.setColor, "red")
  g.setBackgroundColor(0.1, 0.2, 0.3)
  r, gr, b = g.getBackgroundColor()
  t.check("background color", t.near(r, 0.1) and t.near(gr, 0.2) and t.near(b, 0.3))
  g.setLineWidth(3)
  t.check("line width", g.getLineWidth() == 3)
  g.setPointSize(4)
  t.check("point size", g.getPointSize() == 4)
  local w, h = g.getDimensions()
  t.check("dimensions", w == 320 and h == 240 and g.getWidth() == w and g.getHeight() == h, w .. "x" .. h)
  t.check("sizes are integers", math.type(w) == "integer", math.type(w))

  -- Shapes
  t.errors("missing argument", "bad argument #4 to 'rectangle' (number expected, got no value)",
    g.rectangle, "fill", 1, 2)
  t.errors("bad draw mode",
    "bad argument #1 to 'rectangle' (invalid draw mode 'fil', expected one of 'fill', 'line')",
    g.rectangle, "fil", 1, 2, 3, 4)
  t.errors("odd coordinates", "polygon: number of vertex components must be a multiple of two",
    g.polygon, "fill", 1, 2, 3)
  t.errors("non-number vertex", "bad argument #2 to 'polygon' (table must contain only numbers)",
    g.polygon, "fill", { 1, 2, "x" })
  t.check("shapes draw", pcall(function()
    g.rectangle("line", 1, 2, 3, 4)
    g.circle("fill", 10, 10, 5)
    g.ellipse("line", 10, 10, 5, 3)
    g.polygon("fill", { 0, 0, 10, 0, 5, 5 })
    g.line(0, 0, 10, 10, 20, 0)
    g.points({ 1, 1, 2, 2 })
  end))

  -- Images and quads
  local image = g.newImage("assets/sprites.png")
  t.check("image size", image:getWidth() == 32 and image:getHeight() == 16,
    image:getWidth() .. "x" .. image:getHeight())
  local iw, ih = image:getDimensions()
  t.check("image getDimensions", iw == 32 and ih == 16)
  t.check("image type", image:type() == "Image")
  t.check("image default filter", image:getFilter() == "linear")
  image:setFilter("nearest")
  t.check("image setFilter", image:getFilter() == "nearest")
  t.errors("bad filter", "invalid filter mode 'blurry', expected one of 'linear', 'nearest'",
    image.setFilter, image, "blurry")
  g.setDefaultFilter("nearest")
  t.check("default filter", g.getDefaultFilter() == "nearest"
    and g.newImage("assets/sprites.png"):getFilter() == "nearest")
  g.setDefaultFilter("linear")
  t.errors("missing image", "could not load image 'assets/nope.png': file not found: 'assets/nope.png'",
    g.newImage, "assets/nope.png")
  t.errors("case-sensitive paths", "(found 'assets/sprites.png'; paths are case-sensitive)",
    g.newImage, "assets/Sprites.png")
  local quad = g.newQuad(1, 2, 3, 4)
  local qx, qy, qw, qh = quad:getViewport()
  t.check("quad viewport", qx == 1 and qy == 2 and qw == 3 and qh == 4)
  quad:setViewport(16, 0, 16, 16)
  qx, qy, qw, qh = quad:getViewport()
  t.check("quad setViewport", qx == 16 and qw == 16)
  t.check("quad type", quad:type() == "Quad")
  t.errors("draw non-image", "bad argument #1 to 'draw' (Image expected", g.draw, quad)
  t.check("draw", pcall(g.draw, image, 10, 10, 0.5, 2, 2, 8, 8))
  t.check("draw quad", pcall(g.draw, image, quad, 10, 10))

  -- Text
  local font = g.getFont()
  t.check("default font", font:type() == "Font" and font:getHeight() >= 16, font:getHeight())
  t.check("font width", font:getWidth("abc") > 0 and font:getWidth("abc") < font:getWidth("abcdef"))
  t.check("multi-line width", font:getWidth("abc\na") == font:getWidth("abc"))
  local big = g.newFont(32)
  t.check("newFont(size)", big:getHeight() >= 32, big:getHeight())
  g.setFont(big)
  t.check("setFont", g.getFont():getHeight() == big:getHeight())
  g.setFont(font)
  t.check("print a number", pcall(g.print, 42, 0, 0))
  t.check("printf", pcall(g.printf, "hello world", 0, 0, 50, "center"))
  t.errors("bad align",
    "bad argument #5 to 'printf' (invalid align mode 'justify', expected one of 'left', 'center', 'right')",
    g.printf, "x", 0, 0, 10, "justify")

  -- Transforms
  g.push()
  g.translate(10, 20)
  g.scale(2)
  local px, py = g.transformPoint(1, 1)
  t.check("transformPoint", px == 12 and py == 22, px .. "," .. py)
  local ix, iy = g.inverseTransformPoint(12, 22)
  t.check("inverseTransformPoint", t.near(ix, 1) and t.near(iy, 1), ix .. "," .. iy)
  g.origin()
  px, py = g.transformPoint(1, 1)
  t.check("origin", px == 1 and py == 1)
  g.rotate(math.pi / 2)
  px, py = g.transformPoint(1, 0)
  t.check("rotate", t.near(px, 0) and t.near(py, 1), px .. "," .. py)
  g.pop()
  t.errors("pop underflow", "minimum stack depth reached (more pops than pushes?)", g.pop)
  for _ = 1, 64 do
    g.push()
  end
  t.errors("push overflow", "maximum stack depth reached (more pushes than pops?)", g.push)
  for _ = 1, 64 do
    g.pop()
  end

  t.finish()
end

return suite
