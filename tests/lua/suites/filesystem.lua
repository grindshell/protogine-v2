-- pg.filesystem: reading the game, writing the save directory, and layering the two. The test
-- runs this twice with a fresh save directory; the second run ("again") checks what the first
-- one saved.

local t = require("harness")

local suite = {}

local function contains(list, value)
  for _, item in ipairs(list) do
    if item == value then
      return true
    end
  end
  return false
end

local function reading(fs)
  t.check("default identity", fs.getIdentity() == "lua", fs.getIdentity())
  t.check("getSource", fs.getSource():match("lua$") ~= nil, fs.getSource())
  t.check("getSaveDirectory", fs.getSaveDirectory():match("lua$") ~= nil, fs.getSaveDirectory())

  local contents, size = fs.read("harness.lua")
  t.check("read", contents:find("local harness", 1, true) ~= nil and size == #contents)
  local prefix, prefix_size = fs.read("harness.lua", 5)
  t.check("read a prefix", prefix == "-- He" and prefix_size == 5, prefix)
  t.check("read into a string", fs.read("string", "harness.lua", 2) == "--")
  local missing, err = fs.read("nope.txt")
  t.check("read a missing file", missing == nil and err == "file not found: 'nope.txt'", err)
  missing, err = fs.read("../x")
  t.check("read outside the game", missing == nil and err == "invalid path: '../x'", err)

  local info = fs.getInfo("harness.lua")
  t.check("getInfo file", info.type == "file" and info.size == size and math.type(info.modtime) == "integer")
  t.check("getInfo directory", fs.getInfo("suites").type == "directory" and fs.getInfo("suites").size == nil)
  t.check("getInfo root", fs.getInfo("").type == "directory")
  t.check("getInfo missing", fs.getInfo("nope") == nil)
  t.check("getInfo filter", fs.getInfo("suites", "file") == nil and fs.getInfo("suites", "directory") ~= nil)
  local given = {}
  t.check("getInfo fills a table", fs.getInfo("harness.lua", given) == given and given.type == "file")
  t.check("getInfo filter and table", fs.getInfo("suites", "directory", given) == given and given.size == nil)

  local items = fs.getDirectoryItems("suites")
  t.check("getDirectoryItems", contains(items, "filesystem.lua") and contains(fs.getDirectoryItems("/"), "main.lua"))
  t.check("getDirectoryItems of a missing directory", #fs.getDirectoryItems("nope") == 0)
  t.check("getRealDirectory", fs.getRealDirectory("main.lua") == fs.getSource()
    and fs.getRealDirectory("nope") == nil)

  t.errors("bad container", "bad argument #1 to 'read' (invalid container type 'data', expected one of 'string')",
    fs.read, "data", "harness.lua")
  t.errors("bad file type",
    "bad argument #2 to 'getInfo' (invalid file type 'socket', expected one of 'file', 'directory', 'symlink', 'other')",
    fs.getInfo, "x", "socket")
  t.errors("lines of a missing file", "file not found: 'nope.txt'", fs.lines, "nope.txt")
end

local function writing(fs)
  t.check("write", fs.write("notes.txt", "hello\nworld\r\n") == true)
  t.check("read back", fs.read("notes.txt") == "hello\nworld\r\n")
  local lines = {}
  for line in fs.lines("notes.txt") do
    lines[#lines + 1] = line
  end
  t.check("lines", #lines == 2 and lines[1] == "hello" and lines[2] == "world", table.concat(lines, "|"))
  t.check("append", fs.append("notes.txt", "!") and fs.getInfo("notes.txt").size == 14)
  t.check("binary data", fs.write("bin.dat", "\0\255\1") and fs.read("bin.dat") == "\0\255\1")
  t.check("write a prefix", fs.write("short.txt", "abcdef", 3) and fs.read("short.txt") == "abc")
  t.check("write a number", fs.write("number.txt", 42) and fs.read("number.txt") == "42")

  t.check("write creates directories", fs.write("slots/1/save.lua", "return { level = 3 }")
    and fs.getInfo("slots/1").type == "directory")
  t.check("load", fs.load("slots/1/save.lua")().level == 3)
  t.check("require from the save directory", require("slots.1.save").level == 3)
  t.check("saves are listed", contains(fs.getDirectoryItems(""), "slots") and contains(fs.getDirectoryItems(""), "main.lua"))

  -- The save directory comes first, unless it's appended.
  t.check("the game's copy", fs.read("fixtures/shadowed.txt") == "from the game\n")
  fs.write("fixtures/shadowed.txt", "from the save")
  t.check("the save shadows the game", fs.read("fixtures/shadowed.txt") == "from the save"
    and fs.getRealDirectory("fixtures/shadowed.txt") == fs.getSaveDirectory())
  fs.setIdentity("lua", true)
  t.check("appended identity", fs.read("fixtures/shadowed.txt") == "from the game\n")
  fs.setIdentity("lua")
  t.check("merged listing", contains(fs.getDirectoryItems("fixtures"), "answer.lua")
    and contains(fs.getDirectoryItems("fixtures"), "shadowed.txt"))

  t.check("createDirectory", fs.createDirectory("empty/dir") and fs.getInfo("empty/dir").type == "directory")
  t.check("remove", fs.remove("empty/dir") and fs.remove("empty") and fs.getInfo("empty") == nil)
  local ok, err = fs.remove("slots")
  t.check("remove a full directory", not ok and err == "could not remove 'slots': the directory isn't empty", err)
  ok, err = fs.remove("nope")
  t.check("remove a missing file", not ok and err == "could not remove 'nope': it doesn't exist", err)
  ok, err = fs.remove("main.lua")
  t.check("the game is read-only", not ok and err == "could not remove 'main.lua': it doesn't exist", err)
  ok, err = fs.write("slots", "x")
  t.check("write over a directory", not ok and err == "could not write 'slots': it's a directory", err)
  ok, err = fs.write("../escape", "x")
  t.check("write outside the save directory", not ok and err == "invalid path: '../escape'", err)

  fs.setIdentity("lua-other")
  t.check("another identity", fs.getInfo("notes.txt") == nil and fs.getSaveDirectory():match("lua%-other$") ~= nil)
  fs.write("other.txt", "x")
  fs.setIdentity("lua")
  t.check("identities are separate", fs.getInfo("other.txt") == nil)
  t.errors("bad identity", "bad argument #1 to 'setIdentity' (invalid identity 'a/b')", fs.setIdentity, "a/b")
end

local function persisted(fs)
  t.check("files persist", fs.read("notes.txt") == "hello\nworld\r\n!")
  t.check("modules persist", require("slots.1.save").level == 3)
  t.check("shadows persist", fs.read("fixtures/shadowed.txt") == "from the save")
  fs.setIdentity("lua-other")
  t.check("other identities persist", fs.read("other.txt") == "x")
end

function suite.run(args)
  local fs = pg.filesystem
  if args[2] == "again" then
    persisted(fs)
  else
    reading(fs)
    writing(fs)
  end
  t.finish()
end

return suite
