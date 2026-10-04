// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Building a project into a bundle and serving requests from it.

#![cfg(feature = "filesystem")]

use std::fs;
use std::path::Path;

use luat::bundle::{build, BuildOptions, ModuleDir};
use luat::{finalize, App, Bundle, LuatRequest, LuatResponse, ShellOptions};

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

fn project() -> (tempfile::TempDir, BuildOptions) {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            ("src/app.html", "<html><title>%luat.title%</title><body>%luat.body%</body></html>"),
            ("src/routes/+layout.luat", "<main>{@html props.children}</main>"),
            ("src/routes/+page.luat", "<p>{props.greeting}</p>"),
            (
                "src/routes/+page.server.lua",
                "local util = require('$lib/util')\nfunction load() return { greeting = util.greet('bundle') } end",
            ),
            ("src/routes/+error.luat", "<h1>{props.status}</h1>"),
            ("src/routes/_private/+page.luat", "<p>never routed</p>"),
            (
                "src/routes/odd/+server.lua",
                "function GET() return { body = 'has ]=] and ]==] inside' } end",
            ),
            (
                "src/routes/jobs/+server.lua",
                "function GET() return { body = { n = require('jobs/count').n } } end",
            ),
            ("src/lib/util.lua", "return { greet = function(n) return 'hello ' .. n end }"),
            ("jobs/count.lua", "return { n = 3 }"),
            ("src/routes/hosted/+server.lua", "local h = require('myhost')\nfunction GET() return { body = 'ok' } end"),
        ],
    );
    let options = BuildOptions {
        routes_dir: dir.path().join("src/routes"),
        lib_dir: Some(dir.path().join("src/lib")),
        app_html: Some(dir.path().join("src/app.html")),
        module_dirs: vec![ModuleDir {
            prefix: "jobs".to_string(),
            dir: dir.path().join("jobs"),
        }],
        host_modules: vec!["myhost".to_string()],
    };
    (dir, options)
}

fn serve(app: &App, path: &str) -> LuatResponse {
    let request = LuatRequest::new(path, "GET");
    match app.router.match_url(path) {
        Some(route) => app.engine.respond(&route, &request).unwrap(),
        None => app.engine.respond_not_found(app.router.root_error(), &request),
    }
}

#[test]
fn built_bundle_serves_the_app() {
    let (_dir, options) = project();
    let output = build(&options, |_, _| {}).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    assert_eq!(output.route_count, 4, "_private must not be routed");

    let app = output.bundle.instantiate().unwrap();

    let response = serve(&app, "/");
    let http = finalize(response, &LuatRequest::new("/", "GET"), &app.shell, &ShellOptions::default());
    assert_eq!(
        String::from_utf8(http.body).unwrap(),
        "<html><title>Luat App</title><body><main><p>hello bundle</p></main></body></html>"
    );

    let LuatResponse::Body { body, .. } = serve(&app, "/odd") else { panic!("raw body") };
    assert_eq!(body, b"has ]=] and ]==] inside");

    let LuatResponse::Json { body, .. } = serve(&app, "/jobs") else { panic!("json") };
    assert_eq!(body["n"], 3);

    let missing = serve(&app, "/nope");
    assert_eq!(missing.status(), 404);
    assert!(matches!(&missing, LuatResponse::Html { body, .. } if body == "<h1>404</h1>"));
    assert_eq!(serve(&app, "/_private").status(), 404);
}

#[test]
fn bundle_survives_a_round_trip_through_text_and_bytecode() {
    let (_dir, options) = project();
    let source = build(&options, |_, _| {}).unwrap().bundle.source().to_string();

    let from_text = Bundle::from_source(source).unwrap();
    let bytecode = from_text.compile().unwrap();
    let app = App::from_bytecode(&bytecode).unwrap();
    app.engine.set_development_mode(true).unwrap();
    let response = serve(&app, "/");
    assert_eq!(response.status(), 200, "{response:?}");
}

#[test]
fn bundles_for_another_abi_are_rejected() {
    let err = Bundle::from_source("-- luat-bundle abi=999 luat=9.9.9\nreturn {}").unwrap_err();
    assert!(err.to_string().contains("rebuild"), "{err}");
    let err = Bundle::from_source("return {}").unwrap_err();
    assert!(err.to_string().contains("missing header"), "{err}");
}

#[test]
fn unresolved_requires_are_reported() {
    let (dir, options) = project();
    write(dir.path(), &[("src/routes/broken/+server.lua", "local x = require('./missing')")]);
    let output = build(&options, |_, _| {}).unwrap();
    assert!(
        output.warnings.iter().any(|w| w.contains("missing")),
        "{:?}",
        output.warnings
    );
}

#[test]
fn limits_apply_to_bundle_top_level_code() {
    use luat::{EngineLimits, LimitExceeded};

    let header = format!("-- luat-bundle abi={} luat={}", luat::BUNDLE_ABI, luat::bundle::LUAT_VERSION);
    let bundle = Bundle::from_source(format!("{header}\nwhile true do end")).unwrap();
    let limits = EngineLimits {
        instruction_budget: Some(100_000),
        ..Default::default()
    };

    let err = bundle.instantiate_with_limits(&limits).err().unwrap();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));

    let bytecode = bundle.compile().unwrap();
    let err = App::from_bytecode_with_limits(&bytecode, &limits).err().unwrap();
    assert_eq!(LimitExceeded::from_error(&err), Some(LimitExceeded::Instructions));
}
