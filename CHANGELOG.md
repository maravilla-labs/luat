# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `ctx.setHeader(name, value)` and `ctx.appendHeader(name, value)` set
  response headers from `load` functions, API handlers and actions.
- `ctx.setStatus(code)` sets a page's status from `load`; a `status`
  returned from `load` without `redirect` now does the same.
- `ctx.path`, `ctx.search` and `ctx.href`; `LuatRequest::with_raw_query`.
- `partitioned = true` cookie option.

### Changed
- Dynamic attributes whose value is `nil` or `false` are omitted; `true`
  renders a bare attribute. Spreads follow the same rules.
- Module names resolve case-exactly on case-insensitive filesystems.
- `setCookie` rejects `sameSite = "None"` and `partitioned` without
  `secure = true`.
- `luat build` ignores `require` calls inside Lua comments.

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
