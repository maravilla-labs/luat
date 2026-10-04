// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `require("@scope/name/...")` against installed packages, in development
//! (filesystem resolver) and in a built bundle.

#![cfg(feature = "filesystem")]

use std::fs;
use std::path::Path;

use luat::bundle::{build, BuildOptions};
use luat::{App, Engine, FileSystemResolver, LuatRequest, LuatResponse, NoOpCache, Router};

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

const PAGE: &str = r#"<script>
local Card = require("@acme/ui/Card")
local ui = require("@acme/ui")
local badge = require("@acme/ui/Badge")
local field = require("@acme/ui/forms/Field")
</script>
<Card title={ui.greet(props.name)} /><b>{badge.label}</b>|{field.kind}"#;

/// A project with `@acme/ui` (depending on `@acme/icons`) installed.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let ui = ".luat/packages/@acme/ui";
    let icons = ".luat/packages/@acme/icons";
    write(
        dir.path(),
        &[
            ("src/routes/+page.luat", PAGE),
            ("src/routes/+page.server.lua", "function load() return { name = 'pkg' } end"),
            (&format!("{ui}/luat.toml"), "[package]\nname = \"@acme/ui\"\nversion = \"1.0.0\"\n"),
            // init.lua wins over init.luat for the bare name.
            (&format!("{ui}/src/init.lua"), "local h = require('./helper')\nreturn { greet = h.greet }"),
            (&format!("{ui}/src/init.luat"), "<p>wrong init</p>"),
            (&format!("{ui}/src/helper.lua"), "return { greet = function(n) return 'hi ' .. n end }"),
            // Card.luat wins over Card.lua.
            (
                &format!("{ui}/src/Card.luat"),
                "<script>\nlocal Star = require(\"@acme/icons/Star\")\n</script>\n<div class=\"card\"><Star />{props.title}</div>",
            ),
            (&format!("{ui}/src/Card.lua"), "error('Card.lua must not win')"),
            // A directory module.
            (&format!("{ui}/src/Badge/init.lua"), "return { label = 'badge' }"),
            (&format!("{ui}/src/forms/Field.lua"), "return { kind = 'field' }"),
            (&format!("{icons}/src/Star.luat"), "<i>*</i>"),
        ],
    );
    dir
}

const EXPECTED: &str = "<div class=\"card\"><i>*</i>hi pkg</div><b>badge</b>|field";

fn body(response: LuatResponse) -> String {
    match response {
        LuatResponse::Html { body, .. } => body,
        other => panic!("expected html, got {other:?}"),
    }
}

#[test]
fn packages_resolve_in_development() {
    let dir = project();
    let routes = dir.path().join("src/routes");
    let router = Router::discover(&routes).unwrap();
    let resolver = FileSystemResolver::new(&routes).with_packages_dir(dir.path().join(".luat/packages"));
    let engine = Engine::new(resolver, Box::new(NoOpCache::new())).unwrap();
    engine.set_development_mode(true).unwrap();
    let request = LuatRequest::new("/", "GET");
    let route = router.match_url("/").unwrap();
    assert_eq!(body(engine.respond(&route, &request).unwrap()).trim(), EXPECTED);
}

#[test]
fn built_bundle_contains_packages() {
    let dir = project();
    let options = BuildOptions {
        routes_dir: dir.path().join("src/routes"),
        packages_dir: Some(dir.path().join(".luat/packages")),
        ..Default::default()
    };
    let output = build(&options, |_, _| {}).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    let source = output.bundle.source().to_string();
    // The bundle must not depend on the packages directory at runtime.
    drop(dir);

    let app = luat::Bundle::from_source(source).unwrap().instantiate().unwrap();
    assert_eq!(render(&app), EXPECTED);
}

#[test]
fn missing_packages_are_reported_by_name() {
    let dir = project();
    write(dir.path(), &[("src/routes/x/+server.lua", "local m = require('@acme/nope/Thing')")]);
    let options = BuildOptions {
        routes_dir: dir.path().join("src/routes"),
        packages_dir: Some(dir.path().join(".luat/packages")),
        ..Default::default()
    };
    let output = build(&options, |_, _| {}).unwrap();
    assert!(
        output.warnings.iter().any(|w| w.contains("'@acme/nope' is not installed")),
        "{:?}",
        output.warnings
    );
}

fn render(app: &App) -> String {
    let request = LuatRequest::new("/", "GET");
    let route = app.router.match_url("/").unwrap();
    app.engine.set_development_mode(true).unwrap();
    body(app.engine.respond(&route, &request).unwrap()).trim().to_string()
}
