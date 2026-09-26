-- pg.audio: loading, Source state, the source pool, and playback over time. The master volume
-- is 0 throughout, so nothing is audible. Skipped when there's no audio device.

local t = require("harness")

local suite = {}

local audio = pg.audio
local blip, tone, looped
local started
local phase = 1

function suite.run()
  audio.setVolume(0)

  t.errors("missing type", "bad argument #2 to 'newSource' (string expected, got no value)",
    audio.newSource, "assets/blip.wav")
  t.errors("bad type", "bad argument #2 to 'newSource' (invalid source type 'queue', expected one of 'static', 'stream')",
    audio.newSource, "assets/blip.wav", "queue")
  t.errors("missing file", "could not load audio 'assets/nope.ogg': file not found: 'assets/nope.ogg'",
    audio.newSource, "assets/nope.ogg", "static")
  t.errors("not audio", "could not load audio 'assets/bad.ogg'", audio.newSource, "assets/bad.ogg", "static")

  -- Formats and durations (1 second at 22050 Hz, except the 0.25 second blip)
  for _, file in ipairs({ "tone.ogg", "tone.mp3", "tone.flac" }) do
    for _, kind in ipairs({ "static", "stream" }) do
      local ok, source = pcall(audio.newSource, "assets/" .. file, kind)
      t.check(("decode %s as %s"):format(file, kind), ok and t.near(source:getDuration(), 1, 0.1),
        ok and source:getDuration() or source)
    end
  end
  blip = audio.newSource("assets/blip.wav", "static")
  tone = audio.newSource("assets/tone.ogg", "static")
  looped = audio.newSource("assets/tone.ogg", "stream")
  t.check("types", tone:type() == "Source" and tone:getType() == "static" and looped:getType() == "stream")
  t.check("duration", t.near(blip:getDuration(), 0.25) and t.near(tone:getDuration(), 1), tone:getDuration())
  t.check("duration in samples", tone:getDuration("samples") == 22050, tone:getDuration("samples"))
  t.errors("bad unit", "invalid time unit 'ms', expected one of 'seconds', 'samples'", tone.getDuration, tone, "ms")

  -- Settings
  t.check("defaults", tone:getVolume() == 1 and tone:getPitch() == 1 and not tone:isLooping()
    and not tone:isPlaying() and tone:tell() == 0)
  tone:setVolume(2)
  t.check("volume clamps", tone:getVolume() == 1)
  tone:setVolume(0.25)
  t.check("setVolume", tone:getVolume() == 0.25)
  t.errors("pitch must be positive", "pitch must be a positive, finite number", tone.setPitch, tone, 0)
  tone:setPitch(1.5)
  t.check("setPitch", tone:getPitch() == 1.5)
  tone:setPitch(1)
  tone:setLooping(true)
  t.check("setLooping", tone:isLooping())
  tone:seek(0.5)
  t.check("seek while stopped", t.near(tone:tell(), 0.5), tone:tell())
  t.check("tell in samples", tone:tell("samples") == 11025, tone:tell("samples"))
  tone:seek(0)
  tone:seek(11025, "samples")
  t.check("seek in samples", t.near(tone:tell(), 0.5), tone:tell())
  local copy = tone:clone()
  t.check("clone copies settings", copy:getVolume() == 0.25 and copy:isLooping() and copy:tell() == 0)
  t.check("clone is a new source", copy ~= tone and tone == tone)

  -- Playback state
  if not blip:play() then
    t.skip("no audio device")
    return
  end
  blip:stop()
  t.check("play", tone:play() == true and tone:isPlaying())
  t.check("play while playing", tone:play() == true)
  t.check("one active source", audio.getActiveSourceCount() == 1, audio.getActiveSourceCount())
  tone:pause()
  t.check("pause", not tone:isPlaying() and audio.getActiveSourceCount() == 0)
  t.check("resume", tone:play() and tone:isPlaying())
  t.check("play several", audio.play(looped, { copy }) == true)
  t.check("three active sources", audio.getActiveSourceCount() == 3, audio.getActiveSourceCount())
  t.errors("play a non-source", "bad argument #2 to 'play' (Source expected, got number)", audio.play, tone, 5)
  t.errors("play a bad list", "bad argument #1 to 'play' (table must contain only Sources)",
    audio.play, { tone, "x" })

  local paused = audio.pause()
  local found = false
  for _, source in ipairs(paused) do
    found = found or source == tone
  end
  t.check("pause all", #paused == 3 and found and not tone:isPlaying() and not looped:isPlaying(), #paused)
  audio.play(paused)
  t.check("play the paused list", tone:isPlaying() and looped:isPlaying() and copy:isPlaying())
  audio.stop(looped)
  t.check("stop one", not looped:isPlaying() and looped:tell() == 0)
  audio.setVolume(0.5)
  t.check("master volume", audio.getVolume() == 0.5)
  audio.setVolume(0)

  -- A dropped source keeps playing, and stop() still reaches it.
  audio.newSource("assets/tone.ogg", "static"):play()
  collectgarbage()
  t.check("dropped source stays active", audio.getActiveSourceCount() == 3, audio.getActiveSourceCount())
  audio.stop()
  t.check("stop all", audio.getActiveSourceCount() == 0 and not tone:isPlaying() and not copy:isPlaying())

  blip:play()
  started = pg.timer.getTime()
end

-- Checks that need audio to play for a while, timed by the clock rather than dt (the first dt
-- includes pg.load).
function pg.update()
  if not started then
    return
  end
  local elapsed = pg.timer.getTime() - started
  if phase == 1 and elapsed > 0.4 then
    t.check("a sound finishes on its own", not blip:isPlaying() and blip:tell() == 0, blip:tell())
    tone:setLooping(false)
    tone:seek(0.6)
    tone:play()
    phase = 2
  elseif phase == 2 and elapsed > 0.55 then
    t.check("position advances", tone:tell() > 0.6 and tone:tell() < 1, tone:tell())
    phase = 3
  elseif phase == 3 and elapsed > 1.1 then
    t.check("a non-looping source finishes", not tone:isPlaying() and audio.getActiveSourceCount() == 0)
    looped:setLooping(true)
    looped:seek(0.9)
    looped:play()
    phase = 4
  elseif phase == 4 and elapsed > 1.5 then
    t.check("a looping source wraps around", looped:isPlaying() and looped:tell() < 0.9, looped:tell())
    -- Quit with it still playing: the engine has to silence it.
    t.finish()
    phase = 5
  end
end

return suite
