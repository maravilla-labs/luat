# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-10-05

### Upgrading from 0.1
- Rebuild bundles: bundles carry an ABI version, and 0.2 does not load
  bundles built by 0.1.
- `require` only resolves modules inside the bundle, installed packages and
  modules the host registers; the Lua search path is no longer consulted.
- Server code runs as a coroutine. Hosts that call into the engine use
  `respond_async` (or the blocking `respond`).
- Dynamic attributes with `nil` or `false` are now omitted, and `true`
  renders a bare attribute.

### Added
- **Client assets with content-hashed names** (`[frontend] entries`):
  esbuild bundles JS/TS entries (npm imports included, dynamic imports as
  chunks), CSS entries go through Tailwind. Output lands in
  `_luat/immutable/`, `asset("src/client/app.js")` returns an entry's URL,
  `%luat.head%` gets the tags, and `luat serve` caches these files for good.
  The build lives in the library (`luat::assets`). (#2)
- **Packages**: `luat add/remove/install/update/search/pack/publish/login/yank`,
  `require("@scope/name/...")` from installed packages, path dependencies,
  a registry client behind the `packages` feature.
- **Async host functions**: hosts register async functions with
  `Engine::register_module`, and server code awaits them.
- **Execution limits**: memory, instruction budget and deadline per request
  (`EngineLimits`), settable before a bundle's code runs; Lua pattern
  matching is charged to them.
- **Response model**: cookies, HTTP errors, raw API bodies, the app shell in
  the core; `ctx.setHeader`, `ctx.appendHeader` and `ctx.setStatus`;
  `ctx.path`, `ctx.search`, `ctx.href`; `partitioned = true` cookies.
- Bundles are built and loaded from the library, with one router for every
  host; hosts declare the modules they provide at runtime.

### Changed
- Module names resolve case-exactly on case-insensitive filesystems.
- `setCookie` rejects `sameSite = "None"` and `partitioned` without
  `secure = true`.
- `luat build` ignores `require` calls inside Lua comments and never bundles
  files from outside the given directories.
- Whitespace between sibling nodes is kept.
- JSON `null` in load results, action data and `json.decode` becomes `nil`
  (it was a truthy userdata that slipped past `x or default`).
- `luat dev` watches JS, TS and CSS under `src/` as well.

### Fixed
- A `$lib/` require resolves as written; it no longer falls back to a
  same-named file next to the importer.
- Spreading a `nil` value adds nothing instead of failing the render.
- `$name(...)` inside a string or comment in `<script>` is left alone
  (Alpine's `$nextTick` in attribute strings).

### Security
- Closed sandbox escapes: the internal loader, `require` fallbacks and state
  shared between requests.
- Per-request state stays out of shared slots, so concurrent async requests
  cannot see each other's data.
- Fixed limit bypasses (a budget race and fail-open checks).
- Cloned projects are treated as untrusted when installing packages.

## [0.1.0] - 2025-01-12

### Added
- Initial release of luat
- Core templating engine with Svelte-like syntax
- Support for `{#if}`, `{#each}` control flow blocks
- Component system with props and children
- Script blocks (module and regular)
- Memory and filesystem resolvers
- Template caching (memory and filesystem)
- Template bundling for production
- CLI with commands: `init`, `dev`, `build`, `watch`
- Development server with live reload
- File watcher with debouncing
- npm wrapper package (`@maravilla-labs/luat`)
- Homebrew formula
- Shell installer script

[Unreleased]: https://github.com/maravilla-labs/luat/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/maravilla-labs/luat/releases/tag/v0.1.0
