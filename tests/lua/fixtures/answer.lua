-- Counts its loads, so the lifecycle suite can check that require caches modules.
answer_loads = (answer_loads or 0) + 1
return { answer = 42 }
