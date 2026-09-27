-- pg.graphics: state, argument checking, images, quads, canvases, blend modes, fonts and
-- transforms.

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
  t.errors("draw non-drawable", "bad argument #1 to 'draw' (Drawable expected, got userdata)", g.draw, quad)
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
  g.origin()
  g.shear(1, 0)
  px, py = g.transformPoint(0, 10)
  t.check("shear", px == 10 and py == 10, px .. "," .. py)

  -- Transform objects
  local tf = pg.math.newTransform(10, 20)
  g.origin()
  g.applyTransform(tf)
  g.applyTransform(tf)
  px, py = g.transformPoint(0, 0)
  t.check("applyTransform", px == 20 and py == 40, px .. "," .. py)
  g.replaceTransform(pg.math.newTransform(1, 2))
  px, py = g.transformPoint(0, 0)
  t.check("replaceTransform", px == 1 and py == 2)
  g.pop()
  t.errors("applyTransform type", "bad argument #1 to 'applyTransform' (Transform expected, got number)",
    g.applyTransform, 5)
  t.check("draw with a Transform", pcall(g.draw, image, tf) and pcall(g.draw, image, quad, tf))
  t.check("print with a Transform", pcall(g.print, "hi", tf) and pcall(g.printf, "hi", tf, 50, "right"))
  t.errors("printf align after a Transform",
    "bad argument #4 to 'printf' (invalid align mode 'up', expected one of 'left', 'center', 'right')",
    g.printf, "x", tf, 10, "up")
  t.check("draw with shear", pcall(g.draw, image, 0, 0, 0, 1, 1, 0, 0, 0.5, 0.5))

  -- Canvases
  local canvas = g.newCanvas()
  t.check("canvas type", canvas:type() == "Canvas")
  local cw, ch = canvas:getDimensions()
  t.check("canvas defaults to the window's size", cw == 320 and ch == 240, cw .. "x" .. ch)
  t.check("canvas defaults", canvas:getDPIScale() == 1 and canvas:getMSAA() == 0
    and canvas:getFilter() == "linear")
  canvas = g.newCanvas(64, 32, { dpiscale = 2 })
  t.check("canvas size", canvas:getWidth() == 64 and canvas:getHeight() == 32)
  local pw, ph = canvas:getPixelDimensions()
  t.check("canvas pixel size", pw == 128 and ph == 64 and canvas:getPixelWidth() == 128
    and canvas:getPixelHeight() == 64, pw .. "x" .. ph)
  t.check("canvas dpiscale", canvas:getDPIScale() == 2)
  canvas:setFilter("nearest")
  t.check("canvas setFilter", canvas:getFilter() == "nearest")
  t.errors("canvas bad filter",
    "bad argument #1 to 'setFilter' (invalid filter mode 'blurry', expected one of 'linear', 'nearest')",
    canvas.setFilter, canvas, "blurry")
  g.setDefaultFilter("nearest")
  t.check("canvas default filter", g.newCanvas(8, 8):getFilter() == "nearest")
  g.setDefaultFilter("linear")
  local msaa = g.newCanvas(8, 8, { msaa = 4 }):getMSAA()
  t.check("canvas msaa", msaa == 0 or (msaa > 1 and msaa <= 4), msaa)
  t.errors("canvas width", "bad argument #1 to 'newCanvas' (width must be positive)", g.newCanvas, 0, 10)
  t.errors("canvas height", "bad argument #2 to 'newCanvas' (height must be positive)", g.newCanvas, 10, -1)
  t.errors("canvas fractional size", "bad argument #1 to 'newCanvas' (number has no integer representation)",
    g.newCanvas, 1.5, 10)
  t.errors("canvas setting",
    "bad argument #3 to 'newCanvas' (invalid canvas setting 'format', expected one of 'msaa', 'dpiscale')",
    g.newCanvas, 8, 8, { format = "normal" })
  t.errors("canvas msaa setting", "bad argument #3 to 'newCanvas' (msaa must be a whole number from 0 to 64)",
    g.newCanvas, 8, 8, { msaa = -1 })
  t.errors("canvas dpiscale setting", "bad argument #3 to 'newCanvas' (dpiscale must be a positive number)",
    g.newCanvas, 8, 8, { dpiscale = 0 })
  t.errors("canvas settings type", "bad argument #3 to 'newCanvas' (table expected, got string)",
    g.newCanvas, 8, 8, "msaa")
  t.errors("canvas too big", "could not create canvas: 1000000x1 pixels is larger than this GPU's limit",
    g.newCanvas, 1000000, 1)

  t.check("the screen is the default target", g.getCanvas() == nil)
  g.setCanvas(canvas)
  t.check("setCanvas", g.getCanvas() == canvas and g.getCanvas() ~= g.newCanvas(8, 8))
  t.check("getWidth reports the window", g.getWidth() == 320)
  t.check("draw into a canvas", pcall(function()
    g.clear()
    g.clear(1, 0, 0)
    g.rectangle("fill", 0, 0, 10, 10)
    g.draw(image, 0, 0)
    g.print("hi", 0, 0)
  end))
  t.errors("draw a canvas into itself", "cannot draw a Canvas into itself", g.draw, canvas, 0, 0)
  g.setCanvas()
  t.check("setCanvas()", g.getCanvas() == nil)
  t.check("draw a canvas", pcall(g.draw, canvas, 10, 10, 0, 2) and pcall(g.draw, canvas, quad, 0, 0)
    and pcall(g.draw, canvas, tf))
  t.errors("setCanvas type", "bad argument #1 to 'setCanvas' (Canvas expected, got number)", g.setCanvas, 5)

  local inside
  canvas:renderTo(function(a, b)
    inside = { g.getCanvas() == canvas, a, b }
  end, 1, "two")
  t.check("renderTo", inside[1] and inside[2] == 1 and inside[3] == "two" and g.getCanvas() == nil)
  local other = g.newCanvas(8, 8)
  g.setCanvas(other)
  local ok, err = pcall(canvas.renderTo, canvas, function() error("inside", 0) end)
  t.check("renderTo restores the target after an error", not ok and err == "inside" and g.getCanvas() == other,
    tostring(err))
  g.setCanvas()
  t.errors("renderTo needs a function", "bad argument #1 to 'renderTo' (function expected, got no value)",
    canvas.renderTo, canvas)

  -- Blend modes
  local mode, alphamode = g.getBlendMode()
  t.check("default blend mode", mode == "alpha" and alphamode == "alphamultiply")
  g.setBlendMode("add")
  mode, alphamode = g.getBlendMode()
  t.check("setBlendMode", mode == "add" and alphamode == "alphamultiply")
  g.setBlendMode("multiply", "premultiplied")
  mode, alphamode = g.getBlendMode()
  t.check("setBlendMode premultiplied", mode == "multiply" and alphamode == "premultiplied")
  t.errors("multiply needs premultiplied", "the 'multiply' blend mode must be used with premultiplied alpha",
    g.setBlendMode, "multiply")
  t.check("failed setBlendMode keeps the mode", g.getBlendMode() == "multiply")
  t.errors("bad blend mode",
    "bad argument #1 to 'setBlendMode' (invalid blend mode 'lighten', expected one of 'alpha', 'add', "
      .. "'subtract', 'multiply', 'screen', 'replace')",
    g.setBlendMode, "lighten")
  t.errors("bad alpha mode",
    "bad argument #2 to 'setBlendMode' (invalid blend alpha mode 'straight', expected one of "
      .. "'alphamultiply', 'premultiplied')",
    g.setBlendMode, "alpha", "straight")
  t.check("draw with every blend mode", pcall(function()
    for _, m in ipairs({ "alpha", "add", "subtract", "multiply", "screen", "replace" }) do
      g.setBlendMode(m, "premultiplied")
      g.rectangle("fill", 0, 0, 1, 1)
      if m ~= "multiply" then
        g.setBlendMode(m)
        g.draw(canvas, 0, 0)
      end
    end
  end))
  g.setBlendMode("alpha")

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
