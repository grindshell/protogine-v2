# Lua API design

**Status:** first design pass. It covers the core lifecycle, graphics, input, audio, math and the filesystem. System comes in a later pass.

- **Implemented:** everything in this document: games and files, the lifecycle, `pg.graphics`, `pg.window`, `pg.timer`, `pg.event`, `pg.keyboard`, `pg.mouse`, `pg.touch`, `pg.audio`, `pg.math` and `pg.filesystem`. `games/demo` exercises them.

```lua
local player = { x = 100, y = 100, speed = 200 }
local sprite

function pg.load()
  sprite = pg.graphics.newImage("player.png")
end

function pg.update(dt)
  if pg.keyboard.isDown("left", "a") then player.x = player.x - player.speed * dt end
  if pg.keyboard.isDown("right", "d") then player.x = player.x + player.speed * dt end
end

function pg.draw()
  pg.graphics.draw(sprite, player.x, player.y)
  pg.graphics.print("hello", 10, 10)
end

function pg.keypressed(key)
  if key == "escape" then pg.event.quit() end
end
```

## Principles

- **Love2D-shaped.** The callbacks and modules mirror [Love2D](https://love2d.org/wiki/Main_Page), and names follow Love2D wherever the semantics match. The API diverges where macroquad, kira, or the web force it; every divergence is listed under [Divergences from Love2D](#divergences-from-love2d).
- **One global, `pg`.** Modules live under it (`pg.graphics`). Callbacks are functions the game assigns on it (`function pg.update(dt) end`). The engine looks callbacks up at each call, so reassigning one takes effect immediately.
- **Naming:** functions use camelCase (`setColor`, `newImage`). Callbacks are lowercase (`keypressed`). Enums are strings (`"fill"`, `"space"`).
- **Units:**
  - Colors are floats from 0 to 1.
  - Angles are in radians.
  - Coordinates are DPI-scaled pixels, with the origin at the top left and y pointing down. macroquad already reports sizes this way.
- **Objects** are userdata with `:` methods (`image:getWidth()`), freed by the GC. `obj:type()` returns the type name, such as `"Image"`.
- **Synchronous everywhere.** The game's files and its saves are loaded before any Lua runs, so nothing blocks or yields on any platform (see [Games and files](#games-and-files)).
- **Misuse raises Lua errors** in standard form, for example `bad argument #2 to 'rectangle' (number expected, got nil)`. Rust code never panics on bad script input.

## Games and files

A game is a directory with a required `main.lua`, an optional `conf.lua`, and its assets. It can also be packed as a `.zip` of that directory, like a `.love` file.

- **Native:** run `protogine-v2 [game] [args...]`, where `game` is a game directory or `.zip`. The engine reads a directory directly and mounts a zip in memory. With no `game`, it shows the [no-game screen](#no-game-screen).
- **Web:** `cargo xtask web --game <dir>` zips the game directory into `game.zip` next to the wasm. The engine fetches it before running any Lua and mounts it in memory. Without `--game`, the build ships no archive and shows the no-game screen.

The game's files are read-only. Each game also has a writable **save directory**, named by its identity: `t.identity` in `conf.lua`, or else the name of the game's directory or archive (without `.zip`). Reads look in the save directory first, then the game, so a saved file hides a game file at the same path. See [pg.filesystem](#pgfilesystem).

File rules:

- **Paths** are relative to the game root, use `/`, and are case-sensitive on every platform. The engine enforces case on Windows too, so a game that works natively also works on the web.
- **`require("a.b")`** loads `a/b.lua`, then falls back to `a/b/init.lua`, from the save directory or the game. C modules aren't supported.
- **Standard library:** `base`, `string`, `table`, `math`, `utf8` and `coroutine` are available.
  - `print` goes to stdout natively and to the browser console on the web.
  - `dofile` and `loadfile` read from the save directory or the game.
  - From `os`, only `os.time`, `os.clock` and `os.date` are available.
  - There's no `io` library, and `debug` only offers `debug.traceback`.

### No-game screen

This is modeled on Love2D's no-game screen. It's the engine's own screen, not Lua, and it shows:

- that no game is loaded
- the engine version
- how to start a game: pass a game path on the command line, or drop a game directory or `.zip` onto the window

Dropping a game loads and starts it.

## Lifecycle

### conf.lua

`conf.lua` runs first, before the window opens. Only `pg.conf` is read from it; the rest of `pg` isn't available yet. The defaults are:

```lua
function pg.conf(t)
  t.window.title = "protogine"
  t.window.width = 800
  t.window.height = 600
  t.window.resizable = false
  t.window.fullscreen = false
  t.window.highdpi = false
  t.window.msaa = 0         -- MSAA sample count
  t.window.vsync = true
  t.maxdelta = 10           -- seconds; dt is capped at this
  t.identity = nil          -- the save directory's name; nil uses the game's name
  t.appendidentity = false  -- whether reads look in the game before the save directory
end
```

On the web, the page's canvas determines the size and title. There, `title`, `width`, `height`, `resizable` and `fullscreen` are ignored.

### Callbacks

| Callback | Called |
| --- | --- |
| `pg.load(args)` | Once, after `main.lua` runs. `args` holds the command-line arguments after the game path (empty on the web). |
| `pg.update(dt)` | Every frame. `dt` is the seconds since the previous frame, capped at `t.maxdelta` (10 by default). The cap keeps a long stall, such as a backgrounded browser tab, from producing one huge step. |
| `pg.draw()` | Every frame, after `update`. The screen has been cleared to the background color, and the transform reset to the origin. |
| `pg.resize(w, h)` | When the drawable area changes size. |
| `pg.visible(visible)` | When the window loses focus or is minimized (`false`), and when it gets focus back (`true`). miniquad reports both the same way. Losing focus first releases every held key, mouse button and touch, with the matching callbacks, so nothing stays stuck down. |
| `pg.quit()` | When a quit is requested: a window close, or `pg.event.quit()`. Return `true` to cancel. Browsers can't intercept tab closes, so on the web only `pg.event.quit()` triggers it. |
| Input callbacks | See [pg.keyboard](#pgkeyboard), [pg.mouse](#pgmouse) and [pg.touch](#pgtouch). |

### Frame order

1. Call `pg.resize` if the size changed, then dispatch this frame's input and focus events to their callbacks, in the order they happened.
2. Call `pg.update(dt)`.
3. Clear to the background color, reset the transform stack, and call `pg.draw()`.
4. Present.

### Errors

An uncaught error in any callback, `main.lua` or `conf.lua` stops the game's callbacks. The engine then shows an error screen with the message and traceback, and logs the same text to stderr or the browser console.

On the error screen, Ctrl+C (Cmd+C on macOS) copies the message and traceback to the clipboard. Games can't override the error screen yet.

## pg.graphics

### State

| Function | Notes |
| --- | --- |
| `setColor(r, g, b, a)` / `setColor({r, g, b, a})` | `a` defaults to 1. Tints images and text. Persists across frames. |
| `getColor()` | Returns `r, g, b, a`. |
| `setBackgroundColor(r, g, b, a)` / `getBackgroundColor()` | The color the frame is cleared to before `pg.draw`. |
| `setLineWidth(width)` / `getLineWidth()` | Applies to `"line"` shapes and `line`. Default 1. |
| `setPointSize(size)` / `getPointSize()` | Applies to `points`. Default 1. |
| `setFont(font)` / `getFont()` | The font used by `print` and `printf`. |
| `setDefaultFilter(filter)` / `getDefaultFilter()` | Either `"linear"` (the default) or `"nearest"`. Applies to images and fonts created afterwards. |
| `getWidth()`, `getHeight()`, `getDimensions()` | The drawable area. |
| `clear(r, g, b, a)` | Clears immediately. |

### Shapes

`mode` is `"fill"` or `"line"`.

| Function | Notes |
| --- | --- |
| `rectangle(mode, x, y, w, h)` | |
| `circle(mode, x, y, radius)` | |
| `ellipse(mode, x, y, rx, ry)` | |
| `polygon(mode, x1, y1, x2, y2, x3, y3, ...)` / `polygon(mode, vertices)` | Filled polygons must be convex. |
| `line(x1, y1, x2, y2, ...)` / `line(points)` | A polyline. |
| `points(x1, y1, ...)` / `points(points)` | |

### Images

| Function | Notes |
| --- | --- |
| `newImage(path)` | Returns an `Image`. Supports PNG and TGA. |
| `newQuad(x, y, w, h)` | Returns a `Quad`, a pixel rectangle within an image, used for sprite sheets. |
| `draw(drawable, x, y, r, sx, sy, ox, oy, kx, ky)` | Defaults: `x, y, r = 0`, `sx = 1`, `sy = sx`, `ox, oy, kx, ky = 0`. `ox, oy` set the origin for rotation and scaling. A negative scale flips. `kx, ky` shear. |
| `draw(drawable, transform)` | Places the drawable with a [`Transform`](#transforms-1) instead. |
| `draw(image, quad, x, y, ...)` / `draw(image, quad, transform)` | Draws just the part of `image` under `quad`. |

- `Image` methods: `getWidth()`, `getHeight()`, `getDimensions()`, `setFilter(filter)`, `getFilter()`.
- `Quad` methods: `getViewport()` returns `x, y, w, h`. `setViewport(x, y, w, h)` changes it.

### Text

| Function | Notes |
| --- | --- |
| `newFont(path, size)` | Returns a `Font` loaded from a TTF file. |
| `newFont(size)` | The built-in font at `size`. The default font is the built-in font at size 16. |
| `print(text, x, y, r, sx, sy, ox, oy, kx, ky)` / `print(text, transform)` | Honors `\n`. Converts numbers with `tostring`. |
| `printf(text, x, y, limit, align, r, sx, sy, ox, oy, kx, ky)` / `printf(text, transform, limit, align)` | Wraps at `limit` pixels. `align` is `"left"` (the default), `"center"` or `"right"`. |

`Font` methods: `getWidth(text)`, `getHeight()`, `setFilter(filter)`, `getFilter()`.

Every `Font` made from the built-in font shares one glyph atlas, so `setFilter` on one of them filters them all.

### Transforms

| Function | Notes |
| --- | --- |
| `push()`, `pop()` | Save and restore the transform. The stack holds at most 64 entries; overflowing it is an error. |
| `origin()` | Resets to the identity. |
| `translate(dx, dy)`, `rotate(angle)`, `scale(sx, sy)` | `sy` defaults to `sx`. |
| `shear(kx, ky)` | Shears x by `kx` times y, and y by `ky` times x. |
| `applyTransform(transform)` | Applies a [`Transform`](#transforms-1) on top of the current transform. |
| `replaceTransform(transform)` | Makes a `Transform` the current transform. |
| `transformPoint(x, y)`, `inverseTransformPoint(x, y)` | Convert between local and screen coordinates, for example for mouse picking. |

### Later passes

These are planned for later:

- canvases (render targets)
- shaders
- blend modes
- scissor and stencil
- sprite batches and meshes
- particles
- arcs and rounded rectangles
- colored text
- screenshots

## pg.window

| Function | Notes |
| --- | --- |
| `setFullscreen(fullscreen)` / `getFullscreen()` | |
| `setMode(w, h)` | Requests a new window size. The OS may pick a different one, and `pg.resize` reports the result. Native only. |
| `getDPIScale()` | Physical pixels per unit. |

## pg.timer

| Function | Notes |
| --- | --- |
| `getDelta()` | The same value as `dt`, including the cap. |
| `getFPS()` | |
| `getTime()` | Seconds since the engine started. High resolution. |

## pg.event

| Function | Notes |
| --- | --- |
| `quit()` | Requests a quit, which goes through `pg.quit`. If the quit isn't canceled, the native build exits. The web build unloads the game and leaves the canvas blank. |

## pg.keyboard

| Function | Notes |
| --- | --- |
| `isDown(key, ...)` | `true` if any of the given keys is held. A name that isn't a Love2D key constant is an error. |
| `setKeyRepeat(enable)` / `hasKeyRepeat()` | Off by default. While off, held keys don't fire repeat `keypressed` events. |
| `setTextInput(enable)` / `hasTextInput()` | On by default. Controls `textinput` events, and the on-screen keyboard on mobile. |

Callbacks:

- `pg.keypressed(key, scancode, isrepeat)`
- `pg.keyreleased(key, scancode)`
- `pg.textinput(text)`: `text` is one UTF-8 character. Control characters, such as backspace and return, don't produce it.

**Key names** follow Love2D's [KeyConstant](https://love2d.org/wiki/KeyConstant), for example `"a"`, `"1"`, `"space"`, `"return"`, `"escape"`, `"left"`, `"lshift"`, `"f1"`, `"kp5"` and `"-"`. Keys with no Love2D name report `"unknown"`. Constants for keys miniquad never reports, such as `"!"` or `"volumeup"`, are accepted by `isDown` but never down.

On Windows, macOS and the web, miniquad identifies keys by their physical position, so a key's name is what that key says on a US layout, whatever the player's layout is. That's how Love2D's scancodes work, so `scancode` is always the same as `key`. Use `textinput` for the characters the player actually typed. On Linux, names follow the layout.

All input state (`isDown` for keys and buttons, positions, touches) is updated for the whole frame's events before any input callback runs, as in Love2D.

## pg.mouse

| Function | Notes |
| --- | --- |
| `getPosition()`, `getX()`, `getY()` | |
| `isDown(button, ...)` | Buttons are numbered like Love2D: `1` left, `2` right, `3` middle. Other numbers are never down. |
| `setVisible(visible)` / `isVisible()` | |
| `setRelativeMode(enable)` / `getRelativeMode()` | Hides and captures the cursor. Its position stays put, and `mousemoved` reports only `dx, dy`. On the web this is pointer lock, which the browser grants only after the player clicks the page. |
| `setCursor(name)` | Takes a system cursor name: `"arrow"`, `"ibeam"`, `"wait"`, `"waitarrow"`, `"crosshair"`, `"hand"`, `"sizeall"`, `"sizewe"`, `"sizens"`, `"sizenesw"`, `"sizenwse"` or `"no"`. `setCursor()` restores the arrow. |

Callbacks:

- `pg.mousepressed(x, y, button, istouch, presses)`
- `pg.mousereleased(x, y, button, istouch, presses)`
- `pg.mousemoved(x, y, dx, dy, istouch)`
- `pg.wheelmoved(x, y)`: about 1 per wheel notch, with positive `y` away from the player and positive `x` to the right. Touchpads give fractions.

The engine counts `presses` itself, like SDL: presses of the same button within 0.5 seconds and 32 pixels of each other add up, so `2` means a double click. The primary touch (the first finger down while no other finger drives the mouse) also moves the cursor and holds button 1, with `istouch = true`. Its mouse events come just before its touch events.

When the game stops, by an error or a quit, the engine shows the cursor again and leaves relative mode.

## pg.touch

| Function | Notes |
| --- | --- |
| `getTouches()` | Returns a list of active touch ids (integers), oldest first. |
| `getPosition(id)` | Returns `x, y`. An id that isn't active is an error. |

Callbacks: `pg.touchpressed(id, x, y, dx, dy, pressure)`, `pg.touchmoved(...)` and `pg.touchreleased(...)`, which take the same arguments. `pressure` is always 1.

## pg.audio

| Function | Notes |
| --- | --- |
| `newSource(path, type)` | Returns a `Source`. `type` is `"static"` or `"stream"`. Reads OGG Vorbis, MP3, WAV and FLAC. |
| `play(source, ...)` | Plays each source. Lists of Sources work too. Returns `true` if all of them started. |
| `pause()` / `pause(source, ...)` | With no arguments, pauses every playing source and returns them as a list, which `play` can resume. |
| `stop()` / `stop(source, ...)` | With no arguments, stops every source. |
| `setVolume(volume)` / `getVolume()` | The master volume, from 0 to 1. |
| `getActiveSourceCount()` | The number of playing sources. |

`Source` methods:

| Method | Notes |
| --- | --- |
| `play()` | Plays from the current position, or resumes a paused source. Playing a source that's already playing does nothing. Returns `false` if the sound couldn't start: there's no audio device, or 128 sounds are already playing. |
| `pause()`, `stop()`, `isPlaying()` | `stop` rewinds. |
| `setVolume(volume)` / `getVolume()` | Linear, from 0 to 1. Default 1. |
| `setPitch(pitch)` / `getPitch()` | The playback speed, so `2` is twice as fast and an octave up. Must be positive. Default 1. |
| `setLooping(loop)` / `isLooping()` | Default `false`. |
| `seek(position, unit)` / `tell(unit)` | `unit` is `"seconds"` (the default) or `"samples"`. |
| `getDuration(unit)` | |
| `getType()` | `"static"` or `"stream"`. |
| `clone()` | A new, stopped Source with the same sound and settings. The decoded audio is shared, so this is cheap. Clone a sound to play overlapping copies of it. |

- A playing source keeps playing after the game drops it, until it finishes. A looping one plays until `pg.audio.stop()`.
- When the game stops, by an error or a quit, all sound stops.
- **Web:** browsers keep audio suspended until the player clicks, taps or presses a key. The engine resumes it on the first gesture. Sounds played before then start at that point.

## pg.math

### Random numbers

| Function | Notes |
| --- | --- |
| `random()` | A number from 0 up to, but not including, 1. |
| `random(max)`, `random(min, max)` | An integer from 1 (or `min`) to `max`, inclusive. An empty range is an error. |
| `randomNormal(stddev, mean)` | A normally distributed number. `stddev` defaults to 1 and `mean` to 0. |
| `setRandomSeed(seed)` / `setRandomSeed(low, high)` | `seed` is a 64-bit integer, or its low and high 32 bits. |
| `getRandomSeed()` | Returns `low, high`. |
| `setRandomState(state)` / `getRandomState()` | The generator's whole state, as a string. Restore a saved state to repeat the numbers from that point. |
| `newRandomGenerator()` / `newRandomGenerator(seed)` / `newRandomGenerator(low, high)` | Returns a `RandomGenerator`, a generator of its own. Without a seed, it starts from the same fixed seed every run. |

`pg.math` has its own generator, seeded from the clock when the game starts. `RandomGenerator` has the same functions as methods, named without "Random": `random`, `randomNormal`, `setSeed`, `getSeed`, `setState` and `getState`.

The generator is Love2D's (xorshift64*), so a seed gives the same numbers as it does in Love2D. Lua's `math.random` is a separate generator.

### Noise

| Function | Notes |
| --- | --- |
| `noise(x)`, `noise(x, y)`, `noise(x, y, z)`, `noise(x, y, z, w)` | Smooth noise from 0 to 1, always the same for the same arguments. It's simplex noise in 1 and 2 dimensions and Perlin noise in 3 and 4, as in Love2D. |

Noise changes over distances of about 1, and in 1, 3 and 4 dimensions it's 0.5 at whole-number coordinates. Scale coordinates down to get smooth variation, for example `noise(x / 100)`.

### Color

| Function | Notes |
| --- | --- |
| `gammaToLinear(r, g, b)` / `linearToGamma(r, g, b)` | Convert between sRGB and linear RGB. They also take one component, or a table. Components are clamped to 0–1. A fourth component, alpha, is only clamped, since alpha is always linear. They return as many components as they're given. |
| `colorToBytes(r, g, b, a)` | Converts components from 0–1 to integers from 0 to 255. It also takes a table, and `a` is optional. |
| `colorFromBytes(r, g, b, a)` | The reverse of `colorToBytes`. |

### Polygons

Both functions take vertices the way `pg.graphics.polygon` does: `x1, y1, x2, y2, ...` or a table of them.

| Function | Notes |
| --- | --- |
| `isConvex(vertices)` | Whether every corner turns the same way. `false` for fewer than three vertices. |
| `triangulate(vertices)` | Splits a polygon, convex or not, into triangles, and returns a list of `{x1, y1, x2, y2, x3, y3}` tables. To fill a concave polygon, draw each triangle with `pg.graphics.polygon("fill", triangle)`. It may fail on a polygon whose edges cross. |

### Bézier curves

| Function | Notes |
| --- | --- |
| `newBezierCurve(vertices)` | Returns a `BezierCurve` with the given control points, which it takes the way `pg.graphics.polygon` takes vertices. |

`BezierCurve` methods:

| Method | Notes |
| --- | --- |
| `evaluate(t)` | Returns the point at `t`, from 0 to 1. |
| `render(depth)` | Returns points along the curve as a flat list, `{x1, y1, x2, y2, ...}`, ready for `pg.graphics.line`. Each level of `depth` doubles the number of segments. The default is 5, and the maximum is 16. |
| `renderSegment(start, end, depth)` | The rendered points from `start` to `end`, each from 0 to 1. |
| `getSegment(t1, t2)` | Returns a new curve that follows this one from `t1` to `t2`. |
| `getDerivative()` | Returns the derivative as a new curve, one degree lower. |
| `getDegree()`, `getControlPointCount()` | The degree is one less than the number of control points. |
| `getControlPoint(i)`, `setControlPoint(i, x, y)`, `removeControlPoint(i)` | Indices start at 1. Negative ones count back from the end, so `-1` is the last point. Other indices wrap around, as in Love2D. |
| `insertControlPoint(x, y, i)` | Inserts a point before the one at `i`. `i` can be one past the end, to append. The default, `-1`, inserts before the last point, as in Love2D. |
| `translate(dx, dy)`, `rotate(angle, ox, oy)`, `scale(s, ox, oy)` | Move the control points. `ox, oy` is the center of rotation or scaling, 0, 0 by default. |

`evaluate`, `render`, `renderSegment` and `getSegment` need at least two control points.

### Transforms

| Function | Notes |
| --- | --- |
| `newTransform()` / `newTransform(x, y, r, sx, sy, ox, oy, kx, ky)` | Returns a `Transform`: the identity, or a placement that takes the same arguments and defaults as `pg.graphics.draw`. |

A `Transform` holds a transformation, which `pg.graphics.applyTransform`, `replaceTransform`, `draw`, `print` and `printf` accept. Its methods:

| Method | Notes |
| --- | --- |
| `translate(dx, dy)`, `rotate(angle)`, `scale(sx, sy)`, `shear(kx, ky)` | Like the `pg.graphics` functions of the same names. |
| `setTransformation(x, y, r, sx, sy, ox, oy, kx, ky)` | Replaces the transformation with a placement. |
| `reset()` | Replaces the transformation with the identity. |
| `apply(other)` | Applies `other` on top, the way `pg.graphics.applyTransform` does. |
| `transformPoint(x, y)`, `inverseTransformPoint(x, y)` | |
| `inverse()`, `clone()` | Return new Transforms. |
| `getMatrix()` | Returns the 16 elements of the 4x4 matrix, row by row. |
| `setMatrix(layout, e1, ..., e16)` | `layout` is `"row"` or `"column"`, and can be left out for `"row"`. The elements can also be given as one table of 16, or as a table of four tables of four. |
| `isAffine2DTransform()` | Whether the matrix only translates, rotates, scales and shears in 2D. |

The methods that change a Transform return it, so calls chain: `t:translate(10, 0):rotate(1)`. `a * b` returns a new Transform that applies `b`, then `a`.

## pg.filesystem

Reading functions see the save directory and the game together, as described in [Games and files](#games-and-files). Writing functions only change the save directory.

| Function | Notes |
| --- | --- |
| `read(name, size)` | Returns the file's contents as a string, and its size in bytes. `size` limits how many bytes are read. On failure, returns `nil` and an error message. `read("string", name, size)` works too. |
| `lines(name)` | An iterator over the file's lines, without their line endings (`\n` or `\r\n`). A file that can't be read is an error. |
| `load(name)` | Loads a Lua file without running it, like `loadfile`. Returns the chunk, or `nil` and an error message. |
| `getInfo(path, filtertype, info)` | Returns a table with `type` (`"file"` or `"directory"`), `size` (files only), and `modtime` (seconds since 1970, when known), or `nil` if nothing is at `path`. With `filtertype`, it also returns `nil` if the type doesn't match. With an `info` table, it fills that in and returns it instead of making a new one. |
| `getDirectoryItems(dir)` | The names in a directory, sorted. `""` is the root. |
| `getRealDirectory(path)` | Where `path` comes from: `getSaveDirectory()` or `getSource()`. `nil` if it doesn't exist. |
| `write(name, data, size)` | Writes a string to a file, replacing it, and creates its directories. `size` limits how many bytes are written. Returns `true`, or `false` and an error message. |
| `append(name, data, size)` | Like `write`, but adds to the end of the file. |
| `createDirectory(name)` | Creates a directory and its parents. Returns `true`, or `false` and an error message. |
| `remove(name)` | Removes a file or an empty directory. Returns `true`, or `false` and an error message. |
| `getIdentity()` / `setIdentity(name, appendidentity)` | The save directory's name, which must be a valid file name. `appendidentity` is like `t.appendidentity`. |
| `getSaveDirectory()` | Where the save directory is. |
| `getSource()` | The game's directory or archive, as a full path natively and `"game.zip"` on the web. |

Strings can hold any bytes, so binary files work.

Save directories are:

- **Windows:** `%APPDATA%\protogine\<identity>`
- **macOS:** `~/Library/Application Support/protogine/<identity>`
- **Linux:** `$XDG_DATA_HOME/protogine/<identity>`, or `~/.local/share/protogine/<identity>`
- **Web:** the browser's IndexedDB storage for the page's site. `getSaveDirectory()` returns `"indexeddb:protogine/<identity>"`. The engine loads all saved files before the game starts. A write updates them right away, and reaches IndexedDB a moment later, so closing the tab at once can lose the last write. Browsers can clear this storage, for example in private windows.

## Divergences from Love2D

- **Lua 5.5 (luars), not LuaJIT.** There's no `ffi`, no `bit` (use the native bitwise operators), no `setfenv`/`getfenv`, and no `loadstring`. `unpack` becomes `table.unpack`. Love2D libraries that rely on any of these need porting.
  - Integers and floats are distinct. `pg.*` functions return whole numbers as integers, so sizes print as `32`, not `32.0`.
  - Syntax errors come from luars' parser and can read differently from C Lua's, for example `expected 'TkRightParen'`.
- **The engine owns the main loop and the error screen.** There's no `pg.run` or `pg.errorhandler`.
- **Files:**
  - There's no `io` library.
  - Paths are case-sensitive on every platform. On a case-insensitive disk (Windows and macOS, by default), a save file can't exist twice in two cases, though it can on the web.
  - Save directories are under `protogine` instead of `LOVE`.
  - `write` and `append` create missing directories. `createDirectory` and `remove` also return an error message when they fail.
  - `getInfo` never reports `"symlink"` or `"other"`, and has no `modtime` for files in a `.zip`.
  - There are no `File` or `FileData` objects (`newFile`, `newFileData`, `read("data", ...)`), no `mount` or `unmount`, and no `setRequirePath`. There's no `pg.filedropped` or `pg.directorydropped` callback.
  - Love2D 11's deprecated `exists`, `isFile`, `isDirectory`, `getSize` and `getLastModified` are left out; use `getInfo`. So are `isFused`, `getUserDirectory`, `getAppdataDirectory`, `getWorkingDirectory` and `getSourceBaseDirectory`.
- **Window:** the title can only be set in `conf.lua`, because miniquad has no runtime title API.
- **Input:**
  - Key names come from physical key positions (except on Linux), so `key` doesn't follow the player's layout and always equals `scancode`. There's no `isScancodeDown`, `getKeyFromScancode` or `getScancodeFromKey`.
  - Only three mouse buttons are supported.
  - There's no `setGrabbed`, because macroquad can only capture the cursor in a way that behaves like relative mode. Use `setRelativeMode`.
  - `pg.visible` also fires on focus changes, and there's no separate `pg.focus`.
  - There's no joystick or gamepad support.
- **Audio:**
  - Every Source is decoded into memory when it's created, `"stream"` ones included. kira can't stream on the web, and its native streaming hangs at the end of OGG Vorbis files after a seek (kira 0.12.4), which breaks looping music. Decoded audio takes about 21 MB per minute.
  - There's no spatial audio (`setPosition`, the listener), no effects or filters, no queueable sources, and no `SoundData` or `Decoder` objects. `newSource` only takes a path. Tracker formats (`.xm`, `.mod`, `.it`) aren't supported.
  - The Sources that `pg.audio.pause()` returns are `==` to the originals but are different objects, so they don't work as keys into tables keyed by the originals.
- **Math:**
  - `random(max)` with `max` below 1, and `random(min, max)` with `min` above `max`, raise `interval is empty`, as Lua's `math.random` does. Love2D returns a number anyway.
  - `noise` is computed in double precision, so its values differ slightly from Love2D's single-precision ones, most of all far from the origin.
  - `isConvex` handles straight corners anywhere. Love2D calls any polygon convex if the corner at its last vertex is straight.
  - `BezierCurve:render` and `renderSegment` accept a depth of at most 16. `renderSegment` requires `start` and `end` between 0 and 1, and swaps them if `start` is larger.
  - There's no `compress` or `decompress`, which Love2D 11 deprecated.
- **Smaller API differences:**
  - Quads are pixel rectangles with no reference dimensions.
  - `setFilter` takes one filter mode, not separate min and mag filters, because macroquad has only one.
  - `setCursor` takes a name instead of a `Cursor` object, and there's no `getCursor`.
  - Touch ids are integers, and `pressure` is always 1.
- **`dt` is capped** at `t.maxdelta` (10 seconds by default).
