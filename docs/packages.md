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

## A project using packages

```toml
# luat.toml of the consuming project
[dependencies]
"@acme/ui" = "^1.2"

# Optional. Without it every scope uses the default registry.
[registries]
default = "https://luat.registry.maravilla.cloud"
"@private" = "https://registry.example.com/luat"
```

- `luat add @acme/ui[@<req>]` adds the dependency, resolves, installs and
  updates the lockfile. `luat remove`, `luat install` (exactly what the
  lockfile says; resolves first when it is missing or stale), `luat update
  [name]`, `luat search <text>`, `luat publish`, `luat login [registry]`,
  `luat pack` (writes the tarball without publishing).
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
  ```

- Resolution: the highest non-yanked version matching every requirement;
  one version per package name in a project (a conflict is an error that
  names both requirements). A yanked version stays installable when the
  lockfile already pins it.
- Builds include installed packages, so `require("@acme/ui/Card")` works
  the same in development and in a built bundle.
- Credentials are never written to project files: `luat login` stores
  tokens per registry URL in the user's config directory
  (`$XDG_CONFIG_HOME/luat/credentials.toml`, mode 0600), and the
  `LUAT_REGISTRY_TOKEN` environment variable overrides it (for CI).

## Tarball

A gzip-compressed tar whose entries all live under `package/`
(`package/luat.toml`, `package/src/Card.luat`, …). Regular files and
directories only — no links, no absolute paths, no `..`. At most 10 MB
compressed and 50 MB / 5000 files uncompressed. The checksum is the
SHA-256 of the tarball bytes, hex encoded, written `sha256:<hex>`.

## Registry HTTP API

All responses are JSON unless noted. Errors are
`{"error": "<message>"}` with a matching status code. Reads need no
authentication on a public registry.

### Index — `GET /index/@scope/name`

Every version of a package, one JSON object per line (newline-delimited),
oldest first. Clients resolve from this alone.

```json
{"name":"@acme/ui","vers":"1.2.0","deps":{"@acme/icons":"^2.0"},"cksum":"sha256:3b5f…","luat":">=0.2","yanked":false}
```

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
`GET /api/v1/packages/@scope/name/<version>` returns the same shape for
one version.

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
- `400` invalid package, `401` no or bad token, `403` scope not allowed,
  `409` version exists, `413` too large.

### Yank — `DELETE /api/v1/packages/@scope/name/<version>/yank`, `PUT …/unyank`

Authenticated like publish. A yanked version is skipped by new
resolutions; nothing is ever deleted.

### Who am I — `GET /api/v1/me`

With a token: `{"user": "…", "scopes": ["acme"]}` (used by `luat login`
to check the token).
