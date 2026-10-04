// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! The engine's pattern functions (`string.find`, `match`, `gmatch`,
//! `gsub`) must behave exactly like Lua 5.4's. Every case was run through
//! reference Lua 5.4.7 to record the expected result.

#[path = "pattern_parity/cases.rs"]
mod cases;

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, EngineLimits};

/// Prints results unambiguously: types, integer vs float, and strings with
/// every byte outside printable ASCII escaped. Uses no pattern function.
const PRELUDE: &str = r##"
local function ser(v)
  local t = type(v)
  if t == "string" then
    local out = {}
    for i = 1, #v do
      local b = string.byte(v, i)
      if b < 32 or b > 126 or b == 34 or b == 92 then
        out[i] = "\\" .. b
      else
        out[i] = string.char(b)
      end
    end
    return '"' .. table.concat(out) .. '"'
  elseif t == "number" then return math.type(v) .. ":" .. tostring(v)
  else return tostring(v) end
end
local function show(ok, ...)
  if not ok then return "ERR:" .. tostring((...)) end
  local parts = {}
  for i = 1, select("#", ...) do parts[i] = ser((select(i, ...))) end
  return "[" .. table.concat(parts, ",") .. "]"
end
local function gm(s, p, init)
  local out = {}
  for a, b, c in string.gmatch(s, p, init) do
    local item = ser(a)
    if b ~= nil then item = item .. "/" .. ser(b) end
    if c ~= nil then item = item .. "/" .. ser(c) end
    out[#out + 1] = item
  end
  return table.concat(out, " ")
end
local chars = {}
for i = 0, 255 do chars[#chars + 1] = string.char(i) end
local ALL = table.concat(chars)
"##;

fn engine() -> Engine<MemoryResourceResolver> {
    Engine::with_memory_cache(MemoryResourceResolver::new(), 8).unwrap()
}

fn run_cases(engine: &Engine<MemoryResourceResolver>) {
    let mut failures = Vec::new();
    for (case, expected) in cases::CASES {
        let code = format!("{PRELUDE}\nreturn show(pcall({case}))");
        let actual: String = match engine.lua().load(&code).set_name("=case").eval() {
            Ok(actual) => actual,
            Err(err) => format!("HOST ERROR: {err}"),
        };
        if actual != *expected {
            failures.push(format!(
                "{case}\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from Lua 5.4:\n{}",
        failures.len(),
        cases::CASES.len(),
        failures.join("\n")
    );
}

#[test]
fn patterns_match_lua_5_4() {
    run_cases(&engine());
}

#[test]
fn limits_do_not_change_results() {
    let engine = engine();
    engine
        .set_limits(&EngineLimits {
            instruction_budget: Some(500_000_000),
            deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(120)),
            memory_bytes: Some(256 * 1024 * 1024),
        })
        .unwrap();
    run_cases(&engine);
}
