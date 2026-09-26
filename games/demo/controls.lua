-- Input and audio: a player moved with the keyboard, click markers that blip, typed text,
-- looping music, and an event log.

local controls = {}

local player = { x = 600, y = 500, speed = 240 }
local markers = {}
local typed = ""
local log = {}
local blip, music

local function record(text)
  table.insert(log, 1, text)
  log[6] = nil
end

function controls.load()
  blip = pg.audio.newSource("blip.wav", "static")
  music = pg.audio.newSource("music.ogg", "stream")
  music:setLooping(true)
  music:setVolume(0.5)
end

function controls.update(dt)
  local kb = pg.keyboard
  local dx, dy = 0, 0
  if kb.isDown("left", "a") then dx = dx - 1 end
  if kb.isDown("right", "d") then dx = dx + 1 end
  if kb.isDown("up", "w") then dy = dy - 1 end
  if kb.isDown("down", "s") then dy = dy + 1 end
  player.x = player.x + dx * player.speed * dt
  player.y = player.y + dy * player.speed * dt

  for i = #markers, 1, -1 do
    local marker = markers[i]
    marker.age = marker.age + dt
    if marker.age > 1 then
      table.remove(markers, i)
    end
  end
end

function controls.draw(x, y)
  local g = pg.graphics

  g.setColor(0.4, 0.9, 0.6)
  g.rectangle("fill", player.x - 10, player.y - 10, 20, 20)

  for _, marker in ipairs(markers) do
    g.setColor(1, 0.8, 0.3, 1 - marker.age)
    g.circle("line", marker.x, marker.y, 8 + marker.age * 30)
    if marker.presses > 1 then
      g.print("x" .. marker.presses, marker.x + 10, marker.y - 24)
    end
  end

  local mx, my = pg.mouse.getPosition()
  if pg.mouse.isDown(1, 2, 3) then
    g.setColor(1, 1, 1)
    g.circle("fill", mx, my, 4)
  end

  g.setColor(0.7, 0.7, 0.8)
  g.print("Arrows/WASD move, click anywhere, type. Tab toggles key repeat, Enter toggles music.", x, y)
  g.print(("mouse %d, %d   touches %d   key repeat %s   music %s %.1f s"):format(
    math.floor(mx), math.floor(my), #pg.touch.getTouches(),
    tostring(pg.keyboard.hasKeyRepeat()), music:isPlaying() and "playing" or "paused",
    music:tell()), x, y + 20)
  g.setColor(1, 1, 1)
  g.print("> " .. typed, x, y + 44)
  g.setColor(0.6, 0.6, 0.7)
  g.print(table.concat(log, "\n"), x, y + 68)
end

function pg.keypressed(key, _scancode, isrepeat)
  record(("keypressed %s%s"):format(key, isrepeat and " (repeat)" or ""))
  if key == "backspace" then
    local last = utf8.offset(typed, -1)
    if last then
      typed = typed:sub(1, last - 1)
    end
  elseif key == "tab" then
    pg.keyboard.setKeyRepeat(not pg.keyboard.hasKeyRepeat())
  elseif key == "return" and not isrepeat then
    if music:isPlaying() then
      music:pause()
    else
      music:play()
    end
  end
end

function pg.keyreleased(key)
  record("keyreleased " .. key)
end

function pg.textinput(text)
  typed = typed .. text
end

function pg.mousepressed(x, y, button, istouch, presses)
  record(("mousepressed %d at %d, %d%s, presses %d"):format(
    button, math.floor(x), math.floor(y), istouch and " (touch)" or "", presses))
  markers[#markers + 1] = { x = x, y = y, presses = presses, age = 0 }

  -- A clone per click lets blips overlap; multi-clicks go up in pitch.
  local sound = blip:clone()
  sound:setPitch(2 ^ ((presses - 1) / 4))
  sound:play()
end

function pg.mousereleased(_x, _y, button)
  record("mousereleased " .. button)
end

function pg.wheelmoved(x, y)
  record(("wheelmoved %g, %g"):format(x, y))
end

function pg.touchpressed(id, x, y)
  record(("touchpressed %d at %d, %d"):format(id, math.floor(x), math.floor(y)))
end

function pg.touchreleased(id)
  record("touchreleased " .. id)
end

function pg.visible(visible)
  record("visible " .. tostring(visible))
end

return controls
