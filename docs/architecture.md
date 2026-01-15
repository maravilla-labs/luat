# Architecture

This document describes the high-level architecture of Luat. The core idea: the engine is resolver-driven, so the same routing and action logic works across dev, production bundles, and WASM.

## Core Engine

- **Resolver-first design**: `ResourceResolver` is the extension point. Filesystem, memory, or bundle resolvers all plug in without changing engine behavior.
- **Routing**: File-based routing modeled after SvelteKit.
  - Pages: `+page.luat`
  - Page server: `+page.server.lua`
  - Layouts: `+layout.luat`, `+layout.server.lua`
  - API routes: `+server.lua`
  - Errors: `+error.luat`
  - Dynamic segments: `[param]`, `[[optional]]`, `[...rest]`
- **Actions**: Defined in `+page.server.lua` as an `actions` table.
  - Requests are actions when method is not `GET`, or when query includes `?/actionName`.
  - Handler resolution order: method-specific handlers under `actions.<name>.<method>`, then `actions.<name>`, then `actions.default.<method>`, then `actions.default`.
  - Example: `POST /todos?/add` triggers `actions.add.POST` or `actions.add` or `actions.default.POST` or `actions.default`.

- **Action Templates (Fragments)**: Optional `*.luat` templates that render after an action executes.
  - **Location**: Place action templates in a `(fragments)` subfolder of the route directory.
  - **Naming**: `METHOD-actionname.luat` (case-insensitive) or `actionname.luat`
  - **Resolution order**: method-prefixed first (e.g., `POST-delete.luat`), then without prefix (`delete.luat`)
  - **Directory structure**:
    ```
    src/routes/todos/
    ├── +page.luat
    ├── +page.server.lua
    └── (fragments)/
        ├── add.luat           # renders after "add" action
        ├── POST-add.luat      # renders after POST "add" action (takes precedence)
        ├── delete.luat
        └── toggle.luat
    ```
  - **Triggering**: Actions are triggered via:
    - Non-GET requests: `POST /todos` with form field `action=add` or query `?/add`
    - Any method with query: `GET /todos?/refresh` or `POST /todos?/delete`
  - **Response**: When an action template renders, the engine returns HTML with `x-luat-fragment` header so adapters skip wrapping it in `app.html`. This enables HTMX-style partial updates.
  - **Props**: Action templates receive the action result as `props` (e.g., `{props.message}`, `{props.error}`).

## Bundling and Production

- **Bundle metadata**:
  - `__routes`: route patterns, page/server/api files, layouts, layout servers, action templates.
  - `__server_sources`: server-side Lua sources.
  - `__require_map`: optional pre-resolved require map (non-literal requires are warnings).
- **Require resolution**:
  - Same rules in dev and production, including `$lib/` and `lib/` aliases.
  - Production `require` uses bundled module loaders and `__server_sources`.

## Adapters

- **CLI dev server**: Translates HTTP requests into `LuatRequest`, calls `engine.respond_async`, then wraps HTML in `app.html`.
- **CLI production server**: Loads the bundle, registers modules, and delegates all routing/actions to the engine.
- **WASM**: Uses memory resolution and the same engine API.
