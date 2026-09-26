-- pg.keyboard, pg.mouse and pg.touch: the getters, setters and argument checking. Event
-- handling is covered by the unit tests in src/input.rs.

local t = require("harness")

local suite = {}

function suite.run()
  local kb, mouse, touch = pg.keyboard, pg.mouse, pg.touch

  t.check("isDown", kb.isDown("space") == false)
  t.check("isDown many", kb.isDown("a", "lshift", "kp5", "f24", "unknown", "%", "volumeup") == false)
  t.errors("isDown bad name", "bad argument #2 to 'isDown' (invalid key constant 'Space')",
    kb.isDown, "a", "Space")
  t.errors("isDown no arguments", "bad argument #1 to 'isDown' (string expected, got no value)", kb.isDown)

  t.check("key repeat off by default", kb.hasKeyRepeat() == false)
  kb.setKeyRepeat(true)
  t.check("setKeyRepeat", kb.hasKeyRepeat() == true)
  kb.setKeyRepeat(false)
  t.check("text input on by default", kb.hasTextInput() == true)
  kb.setTextInput(false)
  t.check("setTextInput", kb.hasTextInput() == false)
  kb.setTextInput(true)

  local x, y = mouse.getPosition()
  t.check("mouse position", math.type(x) ~= nil and math.type(y) ~= nil, tostring(x) .. "," .. tostring(y))
  t.check("getX and getY", mouse.getX() == x and mouse.getY() == y)
  t.check("mouse isDown", mouse.isDown(1, 2, 3, 4, 1.5) == false)
  t.errors("mouse isDown type", "bad argument #1 to 'isDown' (number expected, got string)",
    mouse.isDown, "left")
  t.check("cursor visible by default", mouse.isVisible() == true)
  mouse.setVisible(false)
  t.check("setVisible", mouse.isVisible() == false)
  mouse.setVisible(true)
  t.check("relative mode off by default", mouse.getRelativeMode() == false)
  mouse.setRelativeMode(true)
  t.check("setRelativeMode", mouse.getRelativeMode() == true)
  mouse.setRelativeMode(false)
  t.check("setCursor", pcall(mouse.setCursor, "hand") and pcall(mouse.setCursor))
  t.errors("setCursor bad name",
    "bad argument #1 to 'setCursor' (invalid cursor type 'pointer', expected one of 'arrow'",
    mouse.setCursor, "pointer")

  local touches = touch.getTouches()
  t.check("no touches", type(touches) == "table" and #touches == 0)
  t.errors("inactive touch", "bad argument #1 to 'getPosition' (no active touch with id 3)",
    touch.getPosition, 3)

  -- Quit with the cursor hidden and captured: the engine has to restore it.
  mouse.setVisible(false)
  mouse.setRelativeMode(true)
  mouse.setCursor("wait")
  t.finish()
end

return suite
