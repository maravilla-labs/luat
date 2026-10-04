// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Client assets: hashed file names, the manifest, and `asset()` /
//! `%luat.head%` in a built bundle.
//!
//! The build tests run esbuild; they use `LUAT_TEST_ESBUILD` or an
//! `esbuild` on the PATH, and are skipped without one.

use std::fs;
use std::path::PathBuf;

use luat::assets::{build_assets, AssetEntry, AssetManifest, AssetOptions, IMMUTABLE_DIR};
use luat::bundle::{build, BuildOptions};

fn esbuild() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LUAT_TEST_ESBUILD") {
        return Some(path.into());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|dir| dir.join("esbuild"))
        .find(|p| p.is_file())
}

#[test]
fn entries_get_hashed_names_and_chunks() {
    let Some(esbuild) = esbuild() else {
        eprintln!("skipped: no esbuild (set LUAT_TEST_ESBUILD)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let client = dir.path().join("src/client");
    fs::create_dir_all(&client).unwrap();
    fs::write(client.join("app.js"), "import('./lazy.js').then((m) => m.run());\nimport './app.css';\n").unwrap();
    fs::write(client.join("lazy.js"), "export function run() { return 1; }\n").unwrap();
    fs::write(client.join("app.css"), "body { color: red }\n").unwrap();

    let out = dir.path().join("out");
    let options = AssetOptions {
        project_dir: dir.path().to_path_buf(),
        entries: vec!["src/client/app.js".into()],
        out_dir: out.clone(),
        esbuild,
        tailwind: None,
        production: true,
    };
    let manifest = build_assets(&options).unwrap();
    let entry = manifest.get("src/client/app.js").expect("entry in manifest");
    assert!(entry.file.starts_with(&format!("/{IMMUTABLE_DIR}/app-")), "{}", entry.file);
    assert!(out.join(entry.file.trim_start_matches('/')).is_file());
    assert_eq!(entry.css.len(), 1, "CSS imported from JS is listed");
    let chunks: Vec<_> = fs::read_dir(out.join(IMMUTABLE_DIR).join("chunks")).unwrap().collect();
    assert!(!chunks.is_empty(), "the dynamic import becomes a chunk");

    // A rebuild with different content replaces the old files.
    fs::write(client.join("lazy.js"), "export function run() { return 2; }\n").unwrap();
    let again = build_assets(&options).unwrap();
    assert!(out.join(again.get("src/client/app.js").unwrap().file.trim_start_matches('/')).is_file());
    assert!(!out.join("_luat/.staging").exists());
}

#[test]
fn a_failed_build_keeps_the_previous_output() {
    let Some(esbuild) = esbuild() else {
        eprintln!("skipped: no esbuild (set LUAT_TEST_ESBUILD)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/app.js"), "export const a = 1;\n").unwrap();
    let mut options = AssetOptions {
        project_dir: dir.path().to_path_buf(),
        entries: vec!["src/app.js".into()],
        out_dir: dir.path().join("out"),
        esbuild,
        tailwind: None,
        production: false,
    };
    let first = build_assets(&options).unwrap();
    options.entries = vec!["src/missing.js".into()];
    assert!(build_assets(&options).is_err());
    let file = first.get("src/app.js").unwrap().file.trim_start_matches('/').to_string();
    assert!(options.out_dir.join(file).is_file());
}

#[test]
fn bundles_carry_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let routes = dir.path().join("src/routes");
    fs::create_dir_all(&routes).unwrap();
    fs::write(routes.join("+page.luat"), "<script src={asset(\"src/client/app.js\")}></script>").unwrap();

    let mut manifest = AssetManifest::default();
    manifest.insert(
        "src/client/app.js".into(),
        AssetEntry { file: "/_luat/immutable/app-ABCDEFGH.js".into(), ..Default::default() },
    );
    let built = build(
        &BuildOptions {
            routes_dir: routes,
            assets: Some(manifest),
            ..Default::default()
        },
        |_, _| {},
    )
    .unwrap();
    let app = built.bundle.instantiate().unwrap();
    assert_eq!(
        app.head,
        "<script type=\"module\" src=\"/_luat/immutable/app-ABCDEFGH.js\"></script>\n"
    );
    let html = render_home(&app);
    assert!(html.contains("src=\"/_luat/immutable/app-ABCDEFGH.js\""), "{html}");
}

fn render_home(app: &luat::App) -> String {
    let route = app.router.match_url("/").unwrap();
    let request = luat::request::LuatRequest::new("/", "GET");
    let response = app.engine.respond(&route, &request).unwrap();
    match response {
        luat::response::LuatResponse::Html { body, .. } => body,
        other => panic!("unexpected response {other:?}"),
    }
}
