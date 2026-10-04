# Server code: the request context

`load` functions (`+page.server.lua`, `+layout.server.lua`), API handlers
(`+server.lua`) and form actions (`actions` in `+page.server.lua`) receive
one argument, `ctx`, describing the request and collecting what goes into
the response.

## Request

| Field | Value |
|-------|-------|
| `ctx.method` | `"GET"`, `"POST"`, … |
| `ctx.path` | The request path: `/blog/hello` |
| `ctx.search` | The query string with its `?` (`?page=2&sort=new`), or `""` when there is none. Raw, as the client sent it, like `URL.search` |
| `ctx.href` | `ctx.path .. ctx.search`: the request target, relative to the site's origin (`/blog/hello?page=2`) |
| `ctx.url` | In `load` and API handlers: the path (same as `ctx.path`). In actions: path and query (same as `ctx.href`). Kept for compatibility; prefer `ctx.path` / `ctx.href` |
| `ctx.params` | Route parameters: `ctx.params.slug` for `[slug]` |
| `ctx.query` | Decoded query parameters: `ctx.query.page` |
| `ctx.headers` | Request headers |
| `ctx.cookies` | Request cookies, decoded |
| `ctx.body` / `ctx.form` / `ctx.json` | The parsed body (form or JSON) |

The engine does not know the public origin (scheme and host) of the site,
so there is no absolute URL; build one from configuration when you need it
(canonical links, emails).

Hosts that build `LuatRequest` themselves pass the raw query string with
`LuatRequest::with_raw_query`; without it, `ctx.search` is rebuilt from the
decoded parameters (sorted by name).

## Response headers

```lua
function load(ctx)
  ctx.setHeader("Cache-Control", "public, max-age=300")
  ctx.setHeader("Content-Security-Policy", "frame-ancestors 'self' https://partner.example")
  ctx.setHeader("X-Robots-Tag", "noindex")
  ctx.appendHeader("Vary", "Cookie")
  ctx.appendHeader("Vary", "Accept-Language")
  return { post = post }
end
```

- `ctx.setHeader(name, value)` sets a header, replacing values set earlier
  through `ctx` for the same name (names ignore case). Layout loads run
  before the page's, root first, so a page can override what its layout
  set.
- `ctx.appendHeader(name, value)` adds another value, for headers that may
  repeat (`Vary`, `Link`, …).
- Both work in `load` functions, API handlers and actions. In API handlers
  and actions, a header the handler also returns in `headers` wins over
  the value set through `ctx`.
- Names must be HTTP tokens. Values may not contain control characters
  (CR and LF included), so headers cannot be injected. Violations raise an
  error.
- Not allowed: `Set-Cookie` (use `ctx.setCookie`), `Location` (return
  `redirect`), the framing headers `Content-Length`, `Transfer-Encoding`,
  `Connection`, `Keep-Alive`, `Upgrade`, `Trailer`, and the engine's own
  `x-luat-*` headers.
- `Content-Type` may be set; pages default to `text/html; charset=utf-8`.
- Headers are added to the response when the request succeeds. When it
  fails (`ctx.error` or an error in a handler or template), the error
  response is built without them, so a `Cache-Control` meant for the page
  cannot make an error cacheable. Cookies are sent either way.

## Response status

```lua
function load(ctx)
  local post = find_post(ctx.params.slug)
  if post.deleted then
    ctx.setStatus(410)      -- render the page, with status 410
  end
  return { post = post }
end
```

- `ctx.setStatus(code)` sets the status of the rendered page: 200-299 or
  400-599. Use `redirect` for 3xx. It is available in `load` functions
  only; API handlers and actions return `{ status = ... }`.
- A `status` returned from `load` without `redirect` does the same
  (`return { status = 404, ... }`). With `redirect` it is the redirect
  status, 302 by default (`return { redirect = "/login", status = 303 }`).
- `ctx.error(status, message)` stops the request and renders the nearest
  `+error.luat` instead.

## Cookies

```lua
ctx.setCookie("session", token, {
  maxAge = 3600,        -- seconds
  path = "/",           -- default "/"
  domain = nil,
  secure = true,        -- default false
  httpOnly = true,      -- default true
  sameSite = "Lax",     -- "Lax" (default), "Strict", "None", or false to omit
  partitioned = false,  -- adds Partitioned (CHIPS)
})
ctx.deleteCookie("session")
```

- `sameSite = "None"` and `partitioned = true` both require
  `secure = true`, as browsers do; otherwise `setCookie` raises an error.
- A cookie for a page embedded in another site's frame needs
  `{ secure = true, sameSite = "None", partitioned = true }`.
- Values are percent-encoded where RFC 6265 does not allow the byte, and
  decoded again in `ctx.cookies`.
