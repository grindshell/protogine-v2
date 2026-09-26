-- pg.system: the platform, the clipboard, power and URLs. tests/lua.rs passes the name getOS
-- should give, then "clipboard" where changing the clipboard bothers no one (on CI).

local t = require("harness")

local suite = {}

local function clipboard(s)
  s.setClipboardText("protogine ✓")
  t.check("clipboard round trip", s.getClipboardText() == "protogine ✓", s.getClipboardText())
  s.setClipboardText("before\0after")
  t.check("clipboard text stops at NUL", s.getClipboardText() == "before", s.getClipboardText())
  s.setClipboardText(42)
  t.check("setClipboardText converts numbers", s.getClipboardText() == "42", s.getClipboardText())
end

function suite.run(args)
  local s = pg.system

  t.check("getOS", s.getOS() == args[2], s.getOS())
  local count = s.getProcessorCount()
  t.check("getProcessorCount", math.type(count) == "integer" and count >= 1, count)
  t.check("hasBackgroundMusic", s.hasBackgroundMusic() == false)

  local state, percent, seconds = s.getPowerInfo()
  local states = { unknown = true, battery = true, nobattery = true, charging = true, charged = true }
  t.check("getPowerInfo state", states[state] ~= nil, state)
  t.check("getPowerInfo percent",
    percent == nil or (math.type(percent) == "integer" and percent >= 0 and percent <= 100), percent)
  t.check("getPowerInfo seconds",
    seconds == nil or (math.type(seconds) == "integer" and seconds >= 0), seconds)
  if state == "nobattery" or state == "unknown" then
    t.check("no battery, no percent", percent == nil and seconds == nil)
  end

  t.check("getClipboardText", type(s.getClipboardText()) == "string")
  if args[3] == "clipboard" then
    clipboard(s)
  end
  t.errors("setClipboardText without text",
    "bad argument #1 to 'setClipboardText' (string expected, got no value)", s.setClipboardText)

  -- Only http, https and mailto URLs open, so these fail without starting anything.
  for _, url in ipairs({ "file:///etc/passwd", "javascript:alert(1)", "C:\\Windows\\notepad.exe",
      "https://", "steam://run/1", "" }) do
    t.check(("openURL refuses %q"):format(url), s.openURL(url) == false)
  end
  t.errors("openURL without a URL", "bad argument #1 to 'openURL' (string expected, got no value)",
    s.openURL)

  s.vibrate()
  s.vibrate(0.1)
  t.errors("vibrate with a string", "bad argument #1 to 'vibrate' (number expected, got string)",
    s.vibrate, "long")

  t.finish()
end

return suite
