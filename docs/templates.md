# Templates: attributes and modules

## Attribute values

A dynamic attribute value (`name={expr}`, `name="{expr}"`, `{@html expr}`)
or a spread (`{...attrs}`) renders by its Lua value:

| Value | Output |
|-------|--------|
| `nil` | attribute omitted |
| `false` | attribute omitted |
| `true` | bare attribute: `disabled` |
| string, number | `name="value"`, HTML-escaped (`{@html}` values are not escaped) |

```html
<button disabled={busy} class={active and "on"} title={tip}>Save</button>
<!-- busy = true, active = false, tip = nil: -->
<button disabled>Save</button>
```

`nil` and boolean attributes behave as in Svelte. Unlike Svelte, `false`
omits every attribute, not only boolean ones: the Lua idiom
`cond and value` is `false` when `cond` is false, and should leave the
attribute out rather than render `"false"`.

Attributes whose values are literally `"true"` / `"false"` render `true`
as `name="true"`: `aria-*`, `data-*`, `draggable`, `spellcheck` and
`contenteditable`. To send `"false"` to one of them, pass the string:
`aria-expanded={open and "true" or "false"}`.

`class` also accepts a table: `class={{ active = isActive, big = true }}`
renders the keys whose values are truthy.

Spreads follow the same rules (a `nil` or `false` spread adds nothing),
and skip keys that are not valid attribute names (non-strings, or names with spaces, quotes, `<`, `>`, `/` or `=`).
The order of spread attributes is not specified.

Attributes mixing text and expressions (`title="Hello {name}"`) are one
string; Lua's `..` rules apply to the parts.

## Module names are case-exact

`require("./variants")` resolves only to a file named exactly
`variants.luat` or `variants.lua`, never to `Variants.luat`, on every
operating system. On case-insensitive filesystems (the default on macOS
and Windows) the resolver compares names with the directory entries on
disk, so development behaves like a production bundle and like Linux.
This covers the directories named in the module path too
(`./components/Card` does not find `Components/Card.luat`), `$lib/`
requires, and modules inside installed packages.

## Requires in comments

`luat build` resolves every literal `require("...")` ahead of time and
warns about those it cannot resolve. Requires inside Lua comments
(`-- require("x")`, `--[[ ... ]]`, `--[==[ ... ]==]`) are skipped.

## `$` in script strings

A `$name(...)` in Lua code inside `<script>` is reserved for the
reactivity helpers (`$state`, `$derived`). Inside a string or a comment it
is plain text, so client-side expressions keep their `$`:

```luat
<script>
local init = "$nextTick(() => $refs.input.focus())"
</script>
<div x-init={init}>...</div>
```

## JSON `null` in props

Data that reaches a template as JSON (load results, action data, context
built with `Engine::to_value`) and `json.decode` turn `null` into `nil`.
An absent field and a `null` field read alike, so `props.title or ""`
covers both. In an array a `null` leaves a hole at its position; the other
elements keep their indices.
