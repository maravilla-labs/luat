# Luat packages and the registry protocol

Luat packages share components, layouts, Lua modules and assets between
projects, the way npm packages do for JavaScript. This document is the
contract between the luat tooling (client) and any registry server.

The `luat` CLI uses **https://luat.registry.maravilla.cloud** as its
default registry: a public registry anyone can install from. Any server
implementing the API below can be used instead (see `[registries]`).

## Package names

`@scope/name`, both parts matching `[a-z0-9][a-z0-9._-]{0,63}`. Every
package is scoped. Versions are [semver](https://semver.org) 2.0.

## A package: `luat.toml`

```toml
[package]
name = "@acme/ui"
version = "1.2.0"
description = "Cards, buttons and layouts"
license = "MIT"
repository = "https://example.com/acme/ui"
keywords = ["ui", "components"]
# Luat versions the package works with (semver requirement against the
# luat version that builds the consuming project).
luat = ">=0.2"
# Files to ship. Default: src/**, README*, LICENSE*, CHANGELOG*, luat.toml.
# luat.toml is always shipped. Patterns are globs relative to the package
# root; `*` does not cross `/`. Symlinks are never shipped.
include = ["src/**", "README.md"]

[dependencies]
"@acme/icons" = "^2.0"
```

The package's modules live in `src/`. A consumer requires them by package
name:

| Consumer code | Resolves to (inside the package) |
|---|---|
| `require("@acme/ui")` | `src/init.lua`, else `src/init.luat` |
| `require("@acme/ui/Card")` | `src/Card.luat`, else `src/Card.lua`, else `src/Card/init.lua` |
| `require("@acme/ui/forms/Field")` | `src/forms/Field.luat`, … (same order) |

Inside a package, `require("./x")` stays relative to the requiring file and
`require("@acme/icons/Star")` resolves the package's own dependency.

A published package's `[dependencies]` values must be plain requirement
strings (`"^2.0"`): no tables, no path dependencies, no dependency on the
package itself. `luat pack` and `luat publish` refuse anything else.
`luat` requirements are checked against the version of luat resolving the
project: versions that do not accept it are skipped.

## A project using packages

```toml
# luat.toml of the consuming project
[dependencies]
"@acme/ui" = "^1.2"

# A local directory (path dependency), relative to this luat.toml.
"@acme/forms" = { path = "../forms" }

# Optional. Without it every scope uses the default registry.
[registries]
default = "https://luat.registry.maravilla.cloud"
"@private" = "https://registry.example.com/luat"
```

- `luat add @acme/ui[@<req>]` adds the dependency (`^<latest>` when no
  requirement is given), resolves, installs and updates the lockfile;
  `luat add @acme/ui --path ../ui` adds a path dependency. `luat remove`, `luat install` (exactly what the
  lockfile says; resolves first when it is missing or stale), `luat update
  [name]`, `luat search <text>`, `luat publish`, `luat login [registry]`,
  `luat pack` (writes the tarball without publishing), `luat yank
  @acme/ui@1.2.0 [--undo]`. `luat install --frozen` fails instead of
  resolving when the lockfile is missing or out of date (for CI).
  `luat build` and `luat dev` run `luat install` first when the lockfile
  or `.luat/packages` is missing or stale.
- Installed packages are extracted to `.luat/packages/@scope/name/` (the
  package root: `luat.toml`, `src/`, …). `.luat/` is build state and is not
  committed.
- `luat.lock` pins every package of the resolved graph:

  ```toml
  version = 1

  [[package]]
  name = "@acme/ui"
  version = "1.2.0"
  registry = "https://luat.registry.maravilla.cloud"
  checksum = "sha256:3b5f…"
  dependencies = ["@acme/icons"]

  [[package]]
  name = "@acme/forms"
  version = "0.3.0"
  source = "path+../forms"
  dependencies = []
  ```

  Entries are sorted by name. Registry packages have `registry` and
  `checksum`; path dependencies have `source` (`path+` and the directory
  relative to the project) instead. The lockfile is out of date when a
  requirement of `luat.toml` (or of a path dependency) is not met by its
  entry, a package's registry changed, a path dependency's directory or
  version changed, or it holds packages no longer reachable.
- Resolution: the highest non-yanked version matching every requirement;
  one version per package name in a project (a conflict is an error that
  names both requirements). A yanked version stays installable when the
  lockfile already pins it. Locked versions are kept while they still
  satisfy every requirement; `luat update [name]` drops them (for `name`,
  or all). Every package is fetched from the registry the *project's*
  `[registries]` select for its scope.
- Path dependencies (`{ path = "..." }`): the directory must hold a
  `luat.toml` whose `[package] name` equals the key; an optional
  `version = "<req>"` in the table is checked against it. Their own
  dependencies (registry, or path relative to *their* `luat.toml`) are
  resolved transitively. Installing copies the files `luat pack` would ship
  (same `include` rules, symlinks skipped) into `.luat/packages`, again on
  every install so edits show up; `luat dev` re-copies them when their
  `.lua`/`.luat` files change. A package with path dependencies cannot be
  published.
- Builds include installed packages, so `require("@acme/ui/Card")` works
  the same in development and in a built bundle. A built bundle carries
  the packages' `src/` files and needs no `.luat` directory at runtime.
  Hosts embedding the engine set `BuildOptions::packages_dir` (bundles) or
  `FileSystemResolver::with_packages_dir` (development) to
  `<project>/.luat/packages`; the package manager itself is the `packages`
  cargo feature of the `luat` crate (`luat::packages`).
- Credentials are never written to project files: `luat login` stores
  tokens per registry URL in the user's config directory
  (`$XDG_CONFIG_HOME/luat/credentials.toml`, falling back to
  `~/.config/luat/credentials.toml`, or `%APPDATA%\luat\credentials.toml`
  on Windows; mode 0600), and the `LUAT_REGISTRY_TOKEN` environment
  variable overrides it (for CI). Tokens are sent to every request of the
  registry they belong to, so private registries may require them for
  reads too:

  ```toml
  [registries."https://registry.example.com/luat"]
  token = "…"
  ```

## Tarball

A gzip-compressed tar whose entries all live under `package/`
(`package/luat.toml`, `package/src/Card.luat`, …). Regular files and
directories only — no links, no absolute paths, no `..`. At most 10 MiB
compressed and 50 MiB / 5000 files uncompressed; the limits are binary
and inclusive (exactly 5000 files or 50 MiB are accepted). The checksum
is the SHA-256 of the tarball bytes, hex encoded, written `sha256:<hex>`.
`luat pack` writes entries sorted, with fixed metadata (mode 0644, mtime
0), so packing the same files twice gives the same checksum. Clients
re-validate every entry when extracting and never extract into place:
packages are unpacked into a temporary directory and renamed over the
previous version.

## Registry HTTP API

All responses are JSON unless noted. Errors are
`{"error": "<message>"}` with a matching status code. Reads need no
authentication on a public registry.

URL paths carry `@scope/name` literally (`/index/@acme/ui`,
`/api/v1/packages/@acme/ui/1.2.0/download`); `@` and `/` are not
percent-encoded, and encoded forms do not route. Versions are compared
by semver precedence: `1.0.0+build` is the same version as `1.0.0`.

### Index — `GET /index/@scope/name`

Every version of a package, one JSON object per line (newline-delimited),
oldest first. Clients resolve from this alone.

```json
{"name":"@acme/ui","vers":"1.2.0","deps":{"@acme/icons":"^2.0"},"cksum":"sha256:3b5f…","luat":">=0.2","yanked":false}
```

`luat` is `null` when the package states no luat requirement.

`404` when the package does not exist. Responses may be cached; send
`ETag` / honour `If-None-Match`.

### Download — `GET /api/v1/packages/@scope/name/<version>/download`

The tarball (`application/gzip`). Clients verify it against the index
checksum before extracting.

### Package page data — `GET /api/v1/packages/@scope/name`

```json
{
  "name": "@acme/ui",
  "description": "Cards, buttons and layouts",
  "latest": "1.2.0",
  "license": "MIT",
  "repository": "https://example.com/acme/ui",
  "keywords": ["ui", "components"],
  "readme": "# @acme/ui\n…",
  "versions": [
    {"vers": "1.2.0", "published_at": "2026-10-04T12:00:00Z", "yanked": false, "size": 18342}
  ],
  "dependencies": {"@acme/icons": "^2.0"},
  "files": ["luat.toml", "README.md", "src/Card.luat"]
}
```

`readme`, `dependencies` and `files` describe the latest version.
`latest` is the highest non-yanked release, else the highest non-yanked
pre-release, else the highest version. `readme` is `README.md`, else
`README`, at the package root. `GET /api/v1/packages/@scope/name/<version>`
returns the same shape plus `"vers"`, with `readme`, `dependencies`,
`files` (and the other per-version fields) from that version; `latest`
and `versions` still describe the whole package.

### Search — `GET /api/v1/packages?q=<text>&page=1&per_page=20`

```json
{"packages": [{"name": "@acme/ui", "description": "…", "latest": "1.2.0", "updated_at": "…"}], "total": 1}
```

Without `q`, all packages, most recently updated first. `per_page` ≤ 100.

### Publish — `PUT /api/v1/packages/@scope/name/<version>`

Body: the tarball (`content-type: application/gzip`), header
`Authorization: Bearer <token>`. The server reads `package/luat.toml` from
the tarball and checks that name and version match the URL, the tarball
rules above, that the version does not exist yet, and that the token may
publish to the scope.

- `201` `{"name": "@acme/ui", "vers": "1.2.0", "cksum": "sha256:…"}`
- `400` invalid package, `401` no or bad token, `403` scope not allowed
  (or token not bound to a user), `409` version exists, `413` over a
  size or file-count limit.

### Yank — `DELETE /api/v1/packages/@scope/name/<version>/yank`, `PUT …/unyank`

Authenticated like publish. A yanked version is skipped by new
resolutions; nothing is ever deleted. Both answer `200`
`{"name": "@acme/ui", "vers": "1.2.0", "yanked": true}`.

### Who am I — `GET /api/v1/me`

With a token: `{"user": "…", "scopes": ["acme"]}` (used by `luat login`
to check the token). Always requires a token: `401` without one or with
a bad one, `403` for a token that is not bound to a user (such a token
cannot publish or yank either).
