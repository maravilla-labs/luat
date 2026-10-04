# Client assets

Everything the browser loads (JavaScript, TypeScript, CSS) is built from a
list of entries into `_luat/immutable/`, with the content hash in every
file name. A changed file is a new URL, so these files can be cached for
good, and a deploy never leaves browsers on old scripts or styles.

```toml
[frontend]
enabled = ["tailwind", "esbuild"]
entries = ["src/client/app.js", "src/client/app.css"]
```

- **JavaScript/TypeScript entries** are bundled by esbuild as ES modules.
  npm imports (`import htmx from "htmx.org"`) are bundled in, a dynamic
  `import("./chart.js")` becomes a chunk of its own, and fonts or images
  imported from code are copied with hashed names.
- **CSS entries** go through Tailwind when it is enabled, otherwise through
  esbuild (which bundles `@import`s).
- **CSS imported from JavaScript** is built next to its entry and listed with it.

`luat build` writes the files to `<out>/_luat/immutable/` and embeds the
manifest (entry → URL) into the bundle. `luat dev` rebuilds on every change
under `src/` (templates included, since Tailwind scans them) into
`.luat/dev/`.

## In templates

`asset(entry)` returns an entry's URL; an unknown entry is an error:

```luat
<script type="module" src={asset("src/client/app.js")}></script>
<link rel="stylesheet" href={asset("src/client/app.css")}>
```

With an `app.html` shell, `%luat.head%` already holds the tags for every
entry: stylesheets, `modulepreload` for chunks an entry imports, and a
module script per JavaScript entry. Hosts that embed luat read them from
`App::head` and pass them as `ShellOptions::head`.

## Tools

esbuild and Tailwind are taken from the project's `node_modules/.bin` when
it has them (so the versions in `package.json` apply), otherwise
`esbuild_version` / `tailwind_version` are downloaded.

## Serving and caching

`luat serve` answers `/_luat/immutable/*` with
`Cache-Control: public, max-age=31536000, immutable`. Hosts should do the
same; everything else (`public/`, `static/`) keeps a short cache.

Files that need a fixed URL (favicon, `robots.txt`, downloads, fonts linked
from outside) stay in `public/` and are served as they are.

## Embedding

`luat::assets::build_assets(&AssetOptions { .. })` builds the entries and
returns the `AssetManifest`; pass it as `BuildOptions::assets` to embed it
in the bundle. `luat::assets::find_project_tool` finds a project's own
esbuild/Tailwind.

Projects without `entries` keep the fixed-path builds (`*_entrypoint` /
`*_output`).
