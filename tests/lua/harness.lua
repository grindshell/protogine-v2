-- Helpers for the suites. Every check prints one line, "ok   <name>" or "FAIL <name>", and a
-- suite ends by printing "done: <n> passed, <n> failed" (or "skip: <reason>") and quitting.
-- tests/lua.rs fails on any FAIL line, a missing summary, or a non-zero exit.

local harness = { passed = 0, failed = 0 }

function harness.check(name, ok, detail)
  if ok then
    harness.passed = harness.passed + 1
  else
    harness.failed = harness.failed + 1
  end
  local line = (ok and "ok   " or "FAIL ") .. name
  if detail ~= nil then
    line = line .. "  -- " .. tostring(detail)
  end
  print(line)
end

-- Checks that `f(...)` raises an error whose message contains `expected` (plain text).
function harness.errors(name, expected, f, ...)
  local ok, err = pcall(f, ...)
  local found = not ok and tostring(err):find(expected, 1, true) ~= nil
  harness.check(name, found, ok and "no error" or err)
end

function harness.near(a, b, tolerance)
  return math.abs(a - b) <= (tolerance or 1e-3)
end

-- Prints the summary. Use `finish` instead unless the suite quits some other way.
function harness.summary()
  print(("done: %d passed, %d failed"):format(harness.passed, harness.failed))
end

function harness.finish()
  harness.summary()
  pg.event.quit()
end

-- Ends the suite without failing, for tests the machine can't run.
function harness.skip(reason)
  print("skip: " .. reason)
  pg.event.quit()
end

return harness
