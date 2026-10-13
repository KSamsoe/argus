-- The scenario harness (§9). Loaded once per test run by argus-test; every
-- scenario file then runs with `scenario`, `range`, `days`, `hours`,
-- `minutes`, `seconds` as globals. Everything `t` does is a protocol call
-- through __harness.call, so a scenario is a client like any other.

local H = __harness
local registry = {}

-- ---------------------------------------------------------------- calls

local function callm(method, params)
  local r, err = H.call(method, params or {})
  if err ~= nil then
    local msg = string.format("%s: error %s: %s", method, tostring(err.code), tostring(err.message))
    if type(err.data) == "table" then
      if err.data.hint then msg = msg .. " (" .. err.data.hint .. ")" end
      if err.data.ref then msg = msg .. " at " .. err.data.ref end
    end
    error(msg, 0)
  end
  return r
end

-- -------------------------------------------------------------- globals

function scenario(name)
  assert(type(name) == "string", "scenario needs a name")
  return function(spec)
    assert(type(spec) == "table", "scenario " .. name .. " needs a table")
    assert(type(spec.run) == "function", "scenario " .. name .. " needs run = function(t) ... end")
    -- A scenario starts from a scene, or from a replay: a player's report or
    -- a mark's replay, at its frame with its own seed (issue 25).
    assert(type(spec.scene) == "string" or type(spec.replay) == "string",
      "scenario " .. name .. " needs scene = \"name\", or replay = \"path\" (a report or a replay folder)")
    assert(spec.replay == nil or (spec.seed == nil and spec.seeds == nil),
      "scenario " .. name .. ": a replay runs with its own seed; leave seed and seeds out")
    assert(spec.max_ms == nil or (type(spec.max_ms) == "number" and spec.max_ms > 0),
      "scenario " .. name .. ": max_ms is the milliseconds a run's own code may take, more than 0")
    assert(spec.faults == nil or spec.faults == "fail" or spec.faults == "allow",
      "scenario " .. name .. ": faults is \"fail\" (the default: a system fault fails the run) or \"allow\"")
    spec.name = name
    spec.file = __current_file
    table.insert(registry, spec)
    return spec
  end
end

function range(a, b)
  return { from = a, to = b }
end

function seconds(n) return n end
function minutes(n) return n * 60 end
-- A day is the project's (`[time] day` in argus.toml, 86,400 sim seconds
-- when it says nothing); an hour is a 24th of one.
function hours(n) return n * (__day_seconds or 86400) / 24 end
function days(n) return n * (__day_seconds or 86400) end

-- ------------------------------------------------------------- helpers

local function slug(s)
  return (string.lower(s):gsub("[^%w]+", "_"):gsub("^_+", ""):gsub("_+$", ""))
end

local function seeds_of(spec, override)
  if spec.replay then return { false } end -- the replay's own seed
  if override then return override end
  if spec.seeds then
    local s = spec.seeds
    if type(s) == "table" and s.from then
      local out = {}
      for i = s.from, s.to do table.insert(out, i) end
      return out
    end
    return s
  end
  if spec.seed then return { spec.seed } end
  return { false } -- the scene's own seed policy
end

local function deep_eq(a, b)
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b end
  for k, v in pairs(a) do
    if not deep_eq(v, b[k]) then return false end
  end
  for k in pairs(b) do
    if a[k] == nil then return false end
  end
  return true
end

local function show(v)
  if type(v) == "table" then
    local parts = {}
    local n = #v
    if n > 0 then
      for i = 1, n do parts[i] = show(v[i]) end
      return "[" .. table.concat(parts, ", ") .. "]"
    end
    local keys = {}
    for k in pairs(v) do table.insert(keys, tostring(k)) end
    table.sort(keys)
    for _, k in ipairs(keys) do table.insert(parts, k .. " = " .. show(v[k])) end
    return "{" .. table.concat(parts, ", ") .. "}"
  end
  if type(v) == "string" then return string.format("%q", v) end
  if type(v) == "number" then
    if v == math.floor(v) and math.abs(v) < 1e15 then return string.format("%d", v) end
    return string.format("%.4g", v)
  end
  return tostring(v)
end

local function subset(filter, fields)
  if filter == nil then return true end
  for k, v in pairs(filter) do
    if not deep_eq(fields[k], v) then return false end
  end
  return true
end

local function percentile(values, p)
  if #values == 0 then return 0 end
  table.sort(values)
  local idx = math.ceil(#values * p)
  if idx < 1 then idx = 1 end
  if idx > #values then idx = #values end
  return values[idx]
end

-- ------------------------------------------------------------ matchers

local Num = {}
Num.__index = Num

-- Clips left to draw this run: a failing sweep of fifty seeds keeps its
-- replays, and the first few failures their clips.
local clips_left = 5

-- A failing run's replay (§9, phase 2 item 3), saved the moment it fails,
-- where `replay.open` (and later the editor) opens it: the keyframe before,
-- the input since, the world then, and a clip of the seconds up to it.
local function save_failure(ctx, why)
  if ctx.saved or not ctx.loaded or not ctx.opts.replays then return end
  ctx.saved = true
  local seed = ctx.result.seed
  local dir = ".argus/replays/" .. slug(ctx.spec.name) .. "/seed" .. tostring(seed)
  local clip = clips_left > 0
  if clip then clips_left -= 1 end
  local r, err = H.call("replay.save", {
    dir = dir, note = why, clip = clip, software = ctx.opts.software == true,
    about = { scenario = ctx.spec.name, file = ctx.spec.file, seed = seed },
  })
  ctx.result.replay = r or { error = err and tostring(err.message) or "replay.save failed" }
end

local function record(ctx, e)
  table.insert(ctx.result.expectations, e)
  if not e.ok then
    ctx.result.ok = false
    save_failure(ctx, e.message)
  end
  return e.ok
end

local function num_check(self, op, expected, test)
  local margin = self.margin or 1
  local bound = expected
  if margin ~= 1 and (op == "<" or op == "<=") then bound = expected * margin end
  local actual = self.value
  local ok = type(actual) == "number" and test(actual, bound)
  local msg
  if type(actual) ~= "number" then
    msg = string.format("%s: %s", self.desc, self.hint or ("expected a number, got " .. show(actual)))
  else
    msg = string.format("%s: %s %s %s", self.desc, show(actual), op, show(expected))
    if bound ~= expected then msg = msg .. string.format(" (x%s margin: %s)", show(margin), show(bound)) end
    msg = msg .. (ok and "" or " failed")
  end
  return record(self.ctx, { kind = self.kind, desc = self.desc, op = op, expected = expected, actual = actual, ok = ok, message = msg })
end

function Num:eq(x)
  if type(x) ~= "number" then
    local ok = deep_eq(self.value, x)
    return record(self.ctx, { kind = self.kind, desc = self.desc, op = "==", expected = x, actual = self.value, ok = ok,
      message = string.format("%s: %s == %s%s", self.desc, show(self.value), show(x), ok and "" or " failed") })
  end
  return num_check(self, "==", x, function(a, b) return a == b end)
end
function Num:ne(x) return num_check(self, "~=", x, function(a, b) return a ~= b end) end
function Num:lt(x) return num_check(self, "<", x, function(a, b) return a < b end) end
function Num:lte(x) return num_check(self, "<=", x, function(a, b) return a <= b end) end
function Num:gt(x) return num_check(self, ">", x, function(a, b) return a > b end) end
function Num:gte(x) return num_check(self, ">=", x, function(a, b) return a >= b end) end
function Num:near(x, eps)
  eps = eps or 1e-6
  return num_check(self, "~", x, function(a, b) return math.abs(a - b) <= eps end)
end
function Num:between(lo, hi)
  local a = self.value
  local ok = type(a) == "number" and a >= lo and a <= hi
  return record(self.ctx, { kind = self.kind, desc = self.desc, op = "between", expected = { lo, hi }, actual = a, ok = ok,
    message = string.format("%s: %s in [%s, %s]%s", self.desc, show(a), show(lo), show(hi), ok and "" or " failed") })
end
function Num:get() return self.value end

local function num(ctx, kind, desc, value, margin)
  return setmetatable({ ctx = ctx, kind = kind, desc = desc, value = value, margin = margin }, Num)
end

-- ------------------------------------------------------------- queries

local function query(ctx, select, where, limit)
  local params = { select = select, limit = limit or 100000 }
  if where ~= nil and next(where) ~= nil then params.where = where end
  return callm("world.query", params)
end

local function field_of(ctx, entity, path)
  local g = callm("world.get", { entity = entity })
  local comp, field = string.match(path, "^([^.]+)%.(.+)$")
  if not comp then error("field path must be Component.field, got " .. tostring(path), 0) end
  local c = g.components and g.components[comp]
  if c == nil then return nil, g end
  -- One axis of a vec2: Transform.pos.x.
  local f, axis = string.match(field, "^(.+)%.([xy])$")
  if f ~= nil and type(c[f]) == "table" then
    return c[f][axis == "x" and 1 or 2], g
  end
  return c[field], g
end

-- The daemon adds it up (world.query's sum, phase 3 item 3), so a sum over
-- 20,000 items is one small answer.
local function sum_of(ctx, field, where)
  local q = query(ctx, { "sum(" .. field .. ")" }, where)
  local total = q.rows and q.rows[1] and q.rows[1][1]
  if type(total) ~= "number" then total = 0 end
  return total, q.matched
end

-- ------------------------------------------------------------- running

local function sim_status()
  return callm("sim.status", {})
end

local function sample_one(ctx, s)
  local st = sim_status()
  local v = sum_of(ctx, s.field, s.where)
  table.insert(s.samples, { t = st.time, tick = st.tick, value = v })
end

local function sample_series(ctx)
  for _, s in ipairs(ctx.series) do sample_one(ctx, s) end
end

local function tick_interval(ctx)
  local smallest
  for _, s in ipairs(ctx.series) do
    if smallest == nil or s.every < smallest then smallest = s.every end
  end
  if smallest == nil then return nil end
  local st = sim_status()
  local hz = st.hz or 60
  if st.clock == "discrete" then return math.max(1, math.floor(smallest)) end
  return math.max(1, math.floor(smallest * hz + 0.5))
end

-- What run_until reads. A key it does not know is refused, not dropped: a
-- misspelt max_frames would run the default 100,000. speed = "max" is the only speed
-- there is (the phase 0 design named it), so it is accepted and changes nothing.
local RUN_UNTIL_KEYS = {
  event = true, where = true, max_frames = true, max_ticks = true, sim_time = true,
  frames = true, ticks = true, speed = true,
}

local function run_until(ctx, opts)
  opts = opts or {}
  for k in pairs(opts) do
    if not RUN_UNTIL_KEYS[k] then
      error("run_until has no key " .. tostring(k)
        .. ": it takes event (with where and max_frames), sim_time, or frames", 0)
    end
  end
  if opts.speed ~= nil and opts.speed ~= "max" then
    error("run_until's speed is \"max\", the only one: a scenario always runs as fast as it can", 0)
  end
  ctx.ran_any = true
  local interval = tick_interval(ctx)
  if opts.where ~= nil and opts.event == nil then
    error("run_until's where filters an event: give event too", 0)
  end
  if opts.event ~= nil and opts.where ~= nil then
    -- An event of that name whose fields match: run to each of them in
    -- turn until one does, or the ticks run out.
    local max = opts.max_frames or opts.max_ticks or 100000
    local since = callm("obs.events", { limit = 1 }).last_seq or 0
    local spent, last = 0, nil
    while spent < max do
      last = callm("sim.run_until", { event = opts.event, max_ticks = math.min(interval or max, max - spent) })
      spent += last.ticks or 0
      if interval ~= nil then sample_series(ctx) end
      if last.hit then
        local r = callm("obs.events", { name = opts.event, since = since, limit = 10000 })
        for _, row in ipairs(r.rows or {}) do
          if subset(opts.where, row.fields or {}) then
            last.ticks = spent
            last.event = row
            return last
          end
          since = row.seq
        end
        last.hit = false
      elseif (last.ticks or 0) == 0 then
        break
      end
    end
    if last then last.ticks = spent end
    return last
  end
  if opts.event ~= nil then
    local max = opts.max_frames or opts.max_ticks or 100000
    if interval == nil then
      return callm("sim.run_until", { event = opts.event, max_ticks = max })
    end
    local spent, last = 0, nil
    while spent < max do
      local step = math.min(interval, max - spent)
      last = callm("sim.run_until", { event = opts.event, max_ticks = step })
      spent += last.ticks or step
      sample_series(ctx)
      if last.hit then return last end
    end
    return last
  end
  if opts.sim_time ~= nil then
    if interval == nil then
      -- A call runs at most 100,000 ticks (27.8 of Outpost's days at 60
      -- Hz): go on until the time is reached (found in phase 3 item 3).
      local last
      repeat
        last = callm("sim.run_until", { time = opts.sim_time })
      until last.hit or (last.ticks or 0) == 0
      return last
    end
    local last
    while true do
      local st = sim_status()
      if st.time >= opts.sim_time - 1e-9 then break end
      local remaining = math.ceil((opts.sim_time - st.time) * (st.hz or 60) - 1e-6)
      local step = math.max(1, math.min(interval, remaining))
      last = callm("sim.run_until", { ticks = step })
      sample_series(ctx)
    end
    return last or sim_status()
  end
  local n = opts.frames or opts.ticks
  if n == nil then error("run_until needs event, sim_time, or frames", 0) end
  if interval == nil then
    local last, done = nil, 0
    while done < n do
      local step = math.min(100000, n - done)
      last = callm("sim.run_until", { ticks = step })
      done += step
    end
    return last
  end
  local last
  local done = 0
  while done < n do
    local step = math.min(interval, n - done)
    last = callm("sim.run_until", { ticks = step })
    done += step
    sample_series(ctx)
  end
  return last
end

local function advance(ctx, opts)
  opts = opts or {}
  ctx.ran_any = true
  local n = opts.n or 1
  local interval = tick_interval(ctx)
  if interval == nil then
    return callm("sim.advance", { by = opts.by or "turn", n = n })
  end
  local last
  local done = 0
  while done < n do
    local step = math.min(interval, n - done)
    last = callm("sim.advance", { by = opts.by or "turn", n = step })
    done += step
    sample_series(ctx)
  end
  return last
end

-- ------------------------------------------------------------ the `t`

local function events_since(ctx, name, filter)
  local rows = {}
  local since = ctx.start_seq
  while true do
    local r = callm("obs.events", { name = name, since = since, limit = 10000 })
    for _, row in ipairs(r.rows or {}) do
      if subset(filter, row.fields or {}) then table.insert(rows, row) end
    end
    if r.next == nil or #(r.rows or {}) == 0 then break end
    since = r.rows[#r.rows].seq
  end
  return rows
end

local function reachable(rooms, from, to)
  if from == nil or to == nil or from == to then return true end
  local by = {}
  for _, r in ipairs(rooms) do by[r.name] = r end
  local seen, queue = { [from] = true }, { from }
  while #queue > 0 do
    local cur = table.remove(queue, 1)
    local r = by[cur]
    for _, n in ipairs((r and r.neighbors) or {}) do
      if n == to then return true end
      if not seen[n] then
        seen[n] = true
        table.insert(queue, n)
      end
    end
  end
  return false
end

-- A handle is bound to the run that asked for it; the samples live on
-- the series, which survives a rerun (§6).
local function series_handle(ctx, s)
  local d = "series " .. s.field .. " every " .. show(s.every)
  local handle = {}
  function handle:samples() return s.samples end
  function handle:trend()
    local n = #s.samples
    if n < 2 then
      local m = num(ctx, "series", d .. " trend (per sample)", nil)
      m.hint = "needs at least two samples, got " .. n .. " (declare the series before the run it should watch)"
      return m
    end
    local sx, sy, sxx, sxy = 0, 0, 0, 0
    for i, p in ipairs(s.samples) do
      sx += i; sy += p.value; sxx += i * i; sxy += i * p.value
    end
    local slope = (n * sxy - sx * sy) / (n * sxx - sx * sx)
    return num(ctx, "series", d .. " trend (per sample)", slope)
  end
  function handle:last()
    local p = s.samples[#s.samples]
    return num(ctx, "series", d .. " last", p and p.value)
  end
  function handle:min()
    local m
    for _, p in ipairs(s.samples) do if m == nil or p.value < m then m = p.value end end
    return num(ctx, "series", d .. " min", m)
  end
  function handle:max()
    local m
    for _, p in ipairs(s.samples) do if m == nil or p.value > m then m = p.value end end
    return num(ctx, "series", d .. " max", m)
  end
  return handle
end

local function make_t(ctx, spec, seed)
  local t = { scene = spec.scene, seed = seed, messages = {} }
  t.input = {}
  function t.input:hold(action, value, frames)
    local p = { action = action }
    if value ~= nil then p.value = value end
    if type(frames) == "table" then frames = frames.frames end
    if frames ~= nil then p.frames = frames else p.hold = true end
    return callm("input.inject", p)
  end
  function t.input:tap(action, value)
    local p = { action = action, frames = 1 }
    if value ~= nil then p.value = value end
    return callm("input.inject", p)
  end
  -- Back to rest. An inject without a value is the action's active value,
  -- so the rest value is sent: false, 0, or {0, 0}, by what it holds now
  -- (found building Hollow: this used to press, not release).
  function t.input:release(action)
    local now = callm("input.state", {}).actions[action]
    local rest = false
    if type(now) == "number" then rest = 0 elseif type(now) == "table" then rest = { 0, 0 } end
    return callm("input.inject", { action = action, value = rest })
  end
  -- Text from the host (the page, a model's answer): the next tick's
  -- scripts get it as a host.message event {channel, text, slot}.
  function t.input:message(channel, text)
    return callm("input.message", { channel = channel, text = text })
  end
  function t:run_until(opts) return run_until(ctx, opts) end
  function t:advance(opts) return advance(ctx, opts) end
  function t:step(n) return run_until(ctx, { frames = n or 1 }) end
  function t:call(method, params) return callm(method, params) end
  function t:log(msg) table.insert(self.messages, tostring(msg)) end
  function t:status() return sim_status() end

  local expect = {}
  t.expect = expect
  function expect:count(where)
    local q = query(ctx, { "id" }, where, 1)
    return num(ctx, "count", "count " .. show(where or {}), q.total)
  end
  function expect:field(entity, path)
    local v = field_of(ctx, entity, path)
    return num(ctx, "field", entity .. " " .. path, v)
  end
  function expect:sum(field, where)
    local total = sum_of(ctx, field, where)
    local d = "sum " .. field
    if where then d = d .. " " .. show(where) end
    return num(ctx, "sum", d, total)
  end
  function expect:events(name, filter)
    local rows = events_since(ctx, name, filter)
    local d = "events " .. name
    if filter then d = d .. " " .. show(filter) end
    local ev = { rows = rows }
    function ev:count() return num(ctx, "events", d .. " count", #rows) end
    function ev:exists()
      local ok = #rows > 0
      return record(ctx, { kind = "events", desc = d, ok = ok,
        message = string.format("%s: %d seen%s", d, #rows, ok and "" or ", expected at least one") })
    end
    function ev:first() return rows[1] end
    -- The newest, and every one oldest first (issue 25: a scenario wanting
    -- the latest request paged through obs.events by hand).
    function ev:last() return rows[#rows] end
    function ev:all() return rows end
    return ev
  end
  function expect:perf(stage)
    local last = 600
    local ps = callm("prof.systems", { last = last })
    local desc = "perf(" .. stage .. ") p95"
    local margin = spec.perf_margin or 1.5
    local p = { }
    function p:p95_ms()
      local value
      if stage == "tick" or stage == "frame" then
        value = ps.tick_p95_ms
      else
        local names = {}
        for _, row in ipairs(ps.rows or {}) do
          if row[2] == stage then names[row[1]] = true end
        end
        local worst = callm("prof.worst", { n = last })
        local sums = {}
        for _, tk in ipairs(worst.rows or {}) do
          local s = 0
          for _, sys in ipairs(tk.systems or {}) do
            if names[sys[1]] then s += sys[2] end
          end
          table.insert(sums, s)
        end
        value = percentile(sums, 0.95)
      end
      return num(ctx, "perf", desc, value, margin)
    end
    return p
  end
  -- An image golden (section 9: "an opt-in for rendering work, with a
  -- perceptual threshold, and expected to be rare"). The frame is drawn
  -- with the software rasteriser, so it is the same picture on any machine
  -- of one system; but systems rasterise differently: WARP on Windows,
  -- lavapipe on Linux, and a Mac (which has no software rasteriser) its
  -- GPU, and Hollow's goldens differ between them at text and sprite edges
  -- by 0.4 to 0.6% of pixels (phase 5 item 11, Todd's Mac test). So each
  -- system keeps its own picture, tests/goldens/<scenario>/<name>.seed<n>.
  -- <system>.png (windows, linux, macos), held to `max`; with none of its
  -- own, an older unlabelled picture (.seed<n>.png) is held to `max` as it
  -- always was; with neither, another system's is held to `other_max`
  -- (1%), which the edges' difference fits and a changed look does not.
  -- A run with none at all records this system's; `update` writes this
  -- system's when it has none, or its own no longer matches.
  -- opts: width, height (640 x 360), threshold (how different a pixel must
  -- be, 0.1), max (the share of pixels that may differ, 0.001), other_max
  -- (the share against another system's picture, 0.01).
  function expect:image(name, opts)
    opts = opts or {}
    local base = slug(spec.name) .. "/" .. slug(name) .. ".seed" .. tostring(seed)
    local own = base .. "." .. H.system .. ".png"
    local rel, from = nil, nil
    if H.golden_exists(own) then
      rel = own
    elseif H.golden_exists(base .. ".png") then
      rel = base .. ".png"
    else
      for _, other in ipairs({ "windows", "linux", "macos" }) do
        if other ~= H.system and H.golden_exists(base .. "." .. other .. ".png") then
          rel, from = base .. "." .. other .. ".png", other
          break
        end
      end
    end
    local params = { format = "png", width = opts.width or 640, height = opts.height or 360, software = true }
    local mine = "tests/goldens/" .. own
    if rel == nil then
      local e = { kind = "image", desc = "image " .. name, golden = mine }
      params.path = mine
      callm("obs.frame", params)
      e.ok = true
      e.recorded = true
      e.message = string.format("image %s: recorded %s", name, mine)
      return record(ctx, e)
    end
    local golden = "tests/goldens/" .. rel
    local e = { kind = "image", desc = "image " .. name, golden = golden }
    params.path = ".argus/test-images/" .. own
    params.golden = golden
    params.threshold = opts.threshold or 0.1
    local r = callm("obs.frame", params)
    local c = r.compare
    local max = opts.max or 0.001
    if from ~= nil then max = math.max(max, opts.other_max or 0.01) end
    -- With update, only a picture that no longer matches is drawn again:
    -- one that still does keeps its file, byte for byte (found at the
    -- phase 5 gate: an update rewrote every golden of the scenario). This
    -- system's is drawn when it has none yet.
    if ctx.opts.update and (from ~= nil or c.fraction > max) then
      callm("obs.frame", { format = "png", width = params.width, height = params.height, software = true, path = mine })
      e.golden = mine
      e.ok = true
      e.recorded = true
      e.message = if from ~= nil
        then string.format("image %s: recorded %s (%s's differed by %.2f%%)", name, mine, from, c.fraction * 100)
        else string.format("image %s: updated %s (%.2f%% of its pixels had changed)", name, mine, c.fraction * 100)
      return record(ctx, e)
    end
    e.ok = c.fraction <= max
    e.actual = r.path
    e.diff = c.diff
    e.differing = c.differing
    e.fraction = c.fraction
    e.system = from
    local whose = if from ~= nil then string.format(", recorded on %s, %.2f%% allowed between systems", from, max * 100)
      else string.format(", %.2f%% allowed", max * 100)
    if e.ok then
      e.message = string.format("image %s: matches %s (%d of %d pixels differ%s)",
        name, golden, c.differing, c.pixels, whose)
    else
      e.message = string.format("image %s: %.2f%% of its pixels differ from %s (%s): the picture %s, where they differ %s",
        name, c.fraction * 100, golden, string.sub(whose, 3), r.path, c.diff)
    end
    return record(ctx, e)
  end
  function expect:snapshot(name, spec_)
    spec_ = spec_ or {}
    local select = { "id" }
    for _, s in ipairs(spec_.select or {}) do
      if s ~= "id" then table.insert(select, s) end
    end
    local q = query(ctx, select, spec_.where)
    local actual = { scene = spec.scene, select = select, where = spec_.where, rows = q.rows or {} }
    local rel = slug(spec.name) .. "/" .. slug(name) .. ".seed" .. tostring(seed) .. ".json"
    local text = H.golden_read(rel)
    local e = { kind = "snapshot", desc = "snapshot " .. name, golden = "tests/goldens/" .. rel }
    -- With update, only a snapshot whose rows changed is written again.
    local fresh = H.json_encode(actual)
    if text == nil or (ctx.opts.update and (string.gsub(text, "\r\n", "\n")) ~= fresh) then
      H.golden_write(rel, fresh)
      e.ok = true
      e.recorded = true
      e.message = string.format("snapshot %s: %s %s", name, text == nil and "recorded" or "updated", e.golden)
      return record(ctx, e)
    end
    local golden = H.json_decode(text)
    local diff = { added = {}, removed = {}, changed = {} }
    local by_id = {}
    for _, row in ipairs(golden.rows or {}) do by_id[row[1]] = row end
    local seen = {}
    for _, row in ipairs(actual.rows) do
      local id = row[1]
      seen[id] = true
      local g = by_id[id]
      if g == nil then
        table.insert(diff.added, id)
      else
        for i = 2, #select do
          if not deep_eq(g[i], row[i]) then
            table.insert(diff.changed, { id = id, field = select[i], golden = g[i], actual = row[i] })
          end
        end
      end
    end
    for _, row in ipairs(golden.rows or {}) do
      if not seen[row[1]] then table.insert(diff.removed, row[1]) end
    end
    local ok = #diff.added == 0 and #diff.removed == 0 and #diff.changed == 0
    e.ok = ok
    e.diff = diff
    if ok then
      e.message = string.format("snapshot %s: matches %s (%d rows)", name, e.golden, #actual.rows)
    else
      local lines = {}
      for _, c in ipairs(diff.changed) do
        if #lines < 20 then table.insert(lines, string.format("  %s %s: golden %s, now %s", c.id, c.field, show(c.golden), show(c.actual))) end
      end
      for _, id in ipairs(diff.added) do
        if #lines < 20 then table.insert(lines, "  + " .. id) end
      end
      for _, id in ipairs(diff.removed) do
        if #lines < 20 then table.insert(lines, "  - " .. id) end
      end
      e.message = string.format("snapshot %s: differs from %s (%d changed, %d added, %d removed)\n%s",
        name, e.golden, #diff.changed, #diff.added, #diff.removed, table.concat(lines, "\n"))
    end
    return record(ctx, e)
  end
  function expect:path(entity, where)
    local d = "path " .. entity .. " -> " .. show(where or {})
    local p = {}
    function p:exists()
      local targets = query(ctx, { "id" }, where, 100000)
      local ok, msg
      -- Over a navigation grid when one is under the entity (phase 3 item
      -- 2): A* to each candidate until one is reached.
      local over, why = nil, nil
      if (targets.total or 0) > 0 then
        for _, row in ipairs(targets.rows) do
          local r, err = H.call("nav.path", { from = entity, to = row[1] })
          if err == nil and r.grid ~= nil then
            over = r.grid
            if r.found then
              ok, msg = true, string.format("%s: %d steps to %s over %s", d, r.steps, tostring(row[1]), r.grid)
              break
            end
            why = r.why
          elseif err ~= nil and over == nil then
            why = err.message
          end
        end
      end
      if (targets.total or 0) == 0 then
        ok, msg = false, d .. ": no entity matches"
      elseif ok then
        -- found over the grid
      elseif over ~= nil then
        ok, msg = false, string.format("%s: no way over %s to any of %d candidate(s) (%s)", d, over, targets.total, tostring(why))
      else
        local rooms = callm("scene.rooms", {})
        if rooms.rooms == nil or #rooms.rooms == 0 then
          ok, msg = true, d .. ": " .. tostring(targets.total) .. " candidate(s), flat scene"
        else
          local g = callm("world.get", { entity = entity })
          local from = g.room
          ok = false
          for _, row in ipairs(targets.rows) do
            local tg = callm("world.get", { entity = row[1] })
            if reachable(rooms.rooms, from, tg.room) then ok = true break end
          end
          msg = d .. (ok and ": reachable through the room graph" or ": no candidate in a reachable room")
        end
      end
      return record(ctx, { kind = "path", desc = d, ok = ok, message = msg })
    end
    return p
  end
  function expect:series(field, opts)
    opts = opts or {}
    local every = opts.every or opts[1] or 1
    for _, s in ipairs(ctx.series) do
      if s.field == field and s.every == every then return series_handle(ctx, s) end
    end
    local s = { field = field, where = opts.where, every = every, samples = {} }
    table.insert(ctx.series, s)
    sample_one(ctx, s) -- the series starts where it was declared
    if ctx.ran_any then ctx.late_series = true end
    return series_handle(ctx, s)
  end
  return t
end

-- ---------------------------------------------------------- one seed

local function run_seed(spec, seed, opts, series)
  if seed == false then seed = nil end
  local t0 = H.now_ms()
  local ctx = { opts = opts, spec = spec, series = series or {}, result = { seed = seed, ok = true, expectations = {} }, ran = false }
  for _, s in ipairs(ctx.series) do s.samples = {} end
  local function where(e)
    local msg = tostring(e)
    if string.match(msg, "^tests/[^:%c]+:%d+:") then return msg end
    local tb = debug.traceback("", 2)
    local at = string.match(tb, "(tests/[^:%c]+:%d+)")
    if at then return at .. ": " .. msg end
    return msg
  end
  -- The run's own code is watched from here (issue 28): past its max_ms,
  -- or when the daemon is asked to stop, it is cut off where it is.
  H.run_begin(spec.name, seed or false, spec.max_ms)
  local ok, err = xpcall(function()
    -- A scenario's mods (phase 3 item 8), in load order; none unless it
    -- names them, whatever the scenario before it loaded.
    local p = { scene = spec.scene, mods = spec.mods or {} }
    if seed ~= nil then p.seed = seed end
    local loaded
    if spec.replay then
      -- Its scene with its seed, played to its frame, as replay.open does.
      -- After a fix the world there differs from the one it saved: that is
      -- the fix, which the scenario checks, so the digests are not.
      callm("replay.open", { path = spec.replay })
      loaded = {}
    else
      loaded = callm("scene.load", p)
    end
    if loaded.issues ~= nil then
      error(string.format("scene %s has %d issue(s); the first: %s", spec.scene, #loaded.issues, show(loaded.issues[1])), 0)
    end
    if loaded.mods ~= nil and #loaded.mods.left_out > 0 then
      local l = loaded.mods.left_out[1]
      error(string.format("mod %s was left out: %s", l.id, tostring(l.why[1])), 0)
    end
    if seed == nil then
      local st = sim_status()
      ctx.result.seed = st.seed
      seed = st.seed
    end
    -- A run's overrides (test.run's set, a sweep's candidate): into the
    -- world as it loaded, before the scenario looks at it (phase 4 item 3).
    for _, s in ipairs(opts.set or {}) do
      callm("sim.set", s)
    end
    ctx.loaded = true
    -- The run's events start where the load's end; what `on_load` systems
    -- emit (a generated floor) belongs to the run.
    if loaded.run_from ~= nil then
      ctx.start_seq = loaded.run_from - 1
    else
      local ev = callm("obs.events", { limit = 1 })
      ctx.start_seq = ev.last_seq or 0
    end
    local t = make_t(ctx, spec, seed)
    sample_series(ctx)
    ctx.t = t
    spec.run(t)
    ctx.ran = true
    ctx.result.log = t.messages
  end, where)
  H.run_end()
  if not ok then
    ctx.result.ok = false
    ctx.result.error = tostring(err)
    save_failure(ctx, ctx.result.error)
  end
  -- A game system a fault switched off fails the run, unless the scenario
  -- says it expects one (faults = "allow"): a game played on with its turn
  -- system off, and 70 scenarios passed through 14 faults (issue 12).
  if ok and ctx.loaded and spec.faults ~= "allow" then
    local r = callm("obs.events", { name = "system.fault", since = ctx.start_seq, limit = 10 })
    local first = r.rows and r.rows[1]
    if first ~= nil then
      local f = first.fields or {}
      local at = f.ref and (" (" .. tostring(f.ref) .. ")") or ""
      ctx.result.ok = false
      ctx.result.error = string.format(
        "%s faulted and was switched off at frame %s%s: %s%s",
        tostring(f.system), tostring(first.frame), at, tostring(f.message),
        #r.rows > 1 and string.format(" (and %d more faults)", #r.rows - 1) or ""
      )
      ctx.result.faults = r.rows
      save_failure(ctx, ctx.result.error)
    end
  end
  -- A seed that passes now drops its old failure's replay.
  if ctx.result.ok and opts.replays and ctx.result.seed ~= nil then
    H.replay_clear(slug(spec.name) .. "/seed" .. tostring(ctx.result.seed))
  end
  -- Where the run ended and its world's digest: the same scenario, seed,
  -- and code on two machines should end on the same one (the
  -- cross-platform determinism spike, phase 5 item 11).
  local st = H.call("sim.status", { digest = true })
  if st then ctx.result.tick = st.tick ctx.result.digest = st.digest end
  ctx.result.ms = H.now_ms() - t0
  return ctx.result, ctx
end

-- ---------------------------------------------------------- the run

local M = {}

function M.list()
  local rows = {}
  for _, spec in ipairs(registry) do
    local seeds = seeds_of(spec)
    table.insert(rows, {
      name = spec.name, file = spec.file, scene = spec.scene,
      seeds = spec.seeds and (spec.seeds.from and { from = spec.seeds.from, to = spec.seeds.to } or spec.seeds) or (spec.seed and { spec.seed }) or nil,
      runs = #seeds,
    })
  end
  return rows
end

function M.run(opts)
  opts = opts or {}
  clips_left = 5
  local report = { ok = true, scenarios = {}, total_runs = 0, passed_runs = 0, failed_runs = 0 }
  for _, spec in ipairs(registry) do
    local wanted = true
    if opts.name and opts.name ~= "" then
      wanted = spec.name == opts.name or string.find(string.lower(spec.name), string.lower(opts.name), 1, true) ~= nil
    end
    if opts.file and opts.file ~= "" and spec.file ~= opts.file then wanted = false end
    if wanted then
      local sr = { name = spec.name, file = spec.file, scene = spec.scene, ok = true, seeds = {} }
      for _, seed in ipairs(seeds_of(spec, opts.seeds)) do
        local result, ctx = run_seed(spec, seed, opts)
        -- A series declared after the run it should have watched: the run
        -- is deterministic, so run it again with the series in place (§6).
        if ctx.late_series then
          result = run_seed(spec, seed, opts, ctx.series)
          result.reran = true
        end
        table.insert(sr.seeds, result)
        report.total_runs += 1
        if result.ok then report.passed_runs += 1 else report.failed_runs += 1 sr.ok = false end
        if H.stopped() then break end
      end
      if not sr.ok then report.ok = false end
      table.insert(report.scenarios, sr)
    end
    -- The daemon was asked to stop: no more runs.
    if H.stopped() then break end
  end
  return report
end

return M
