-- Engine prelude: runs once in every game's Lua state, before conf.lua.
--
-- Receives host functions from Rust:
--   read_source(path) -> source | nil, err   reads a file from the mounted game
--   log(message)                              writes a line to stdout / the browser console
-- and returns `invoke(f, ...)`, which Rust uses to run every chunk and callback so that errors
-- come back with a traceback.

local read_source, log = ...

local traceback = debug.traceback
local raw_load = load

-- Only the safe parts of os and debug are exposed; io and package are never opened.
os = { time = os.time, clock = os.clock, date = os.date }
debug = { traceback = traceback }

-- Text chunks only: precompiled bytecode isn't validated. `env` is forwarded through `...`
-- because passing an explicit nil env would give the chunk no globals at all.
function load(chunk, chunkname, _mode, ...)
  return raw_load(chunk, chunkname, "t", ...)
end

function print(...)
  local parts = table.pack(...)
  for i = 1, parts.n do
    parts[i] = tostring(parts[i])
  end
  log(table.concat(parts, "\t", 1, parts.n))
end

function loadfile(path, _mode, ...)
  local source, err = read_source(path)
  if not source then
    return nil, err
  end
  return raw_load(source, "@" .. path, "t", ...)
end

-- Load errors are raised at level 0: the message already says where the problem is.
function dofile(path)
  local chunk, err = loadfile(path)
  if not chunk then
    error(err, 0)
  end
  return chunk()
end

local loaded = {}
package = { loaded = loaded }

function require(name)
  local cached = loaded[name]
  if cached ~= nil then
    return cached
  end

  local base = name:gsub("%.", "/")
  local tried = {}
  for _, path in ipairs({ base .. ".lua", base .. "/init.lua" }) do
    local source = read_source(path)
    if source then
      local chunk, err = raw_load(source, "@" .. path, "t")
      if not chunk then
        error(err, 0)
      end
      local result = chunk(name, path)
      if result == nil then
        result = loaded[name] or true
      end
      loaded[name] = result
      return result
    end
    tried[#tried + 1] = "\n\tno file '" .. path .. "'"
  end
  error("module '" .. name .. "' not found:" .. table.concat(tried), 2)
end

pg = {}

-- Adds a traceback, minus the engine's own frames (this prelude and its xpcall).
local function add_traceback(message)
  local lines = {}
  for line in traceback(tostring(message), 2):gmatch("[^\n]+") do
    if not (line:find("^%s+=?prelude:") or line:find("^%s+%[C%]: in %a+ 'xpcall'")) then
      lines[#lines + 1] = line
    end
  end
  return table.concat(lines, "\n")
end

return function(f, ...)
  return xpcall(f, add_traceback, ...)
end
