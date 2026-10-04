// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Path dependencies: `"@acme/ui" = { path = "../ui" }`.

mod support;

use std::fs;

use luat::packages::{publish, Lockfile, PackageName, Project};
use support::{project, publish_package, registry, settings, write};

/// `app` → `@acme/ui` (path) → `@acme/util` (path, relative to ui) and
/// `@acme/icons` (registry).
async fn setup() -> (tempfile::TempDir, String, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    publish_package(tmp.path(), &url, "@acme/icons", "2.0.0", &[], &[("src/Star.luat", "<i>*</i>")]).await;
    write(
        &tmp.path().join("libs/ui"),
        &[
            (
                "luat.toml",
                "[package]\nname = \"@acme/ui\"\nversion = \"0.3.0\"\n\n[dependencies]\n\"@acme/icons\" = \"^2.0\"\n\"@acme/util\" = { path = \"../util\" }\n",
            ),
            ("src/Card.luat", "<div>v1</div>"),
            ("notes.txt", "not shipped"),
        ],
    );
    write(
        &tmp.path().join("libs/util"),
        &[("luat.toml", "[package]\nname = \"@acme/util\"\nversion = \"1.0.0\"\n"), ("src/init.lua", "return {}")],
    );
    let app = project(tmp.path(), "app", &url, "\"@acme/ui\" = { path = \"../libs/ui\" }\n");
    (tmp, url, app)
}

#[tokio::test]
async fn path_dependencies_resolve_transitively_with_registry_ones() {
    let (tmp, url, app) = setup().await;
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    project.install(false).await.unwrap();

    let lock = Lockfile::load(&app).unwrap().unwrap();
    let get = |n: &str| lock.get(&PackageName::parse(n).unwrap()).unwrap().clone();
    let ui = get("@acme/ui");
    assert_eq!(ui.source.as_deref(), Some("path+../libs/ui"));
    assert_eq!((ui.version.to_string(), ui.registry, ui.checksum), ("0.3.0".to_string(), None, None));
    assert_eq!(get("@acme/util").source.as_deref(), Some("path+../libs/util"));
    assert_eq!(get("@acme/icons").registry.as_deref(), Some(url.as_str()));
    let text = fs::read_to_string(app.join("luat.lock")).unwrap();
    assert!(text.contains("source = \"path+../libs/ui\""), "{text}");

    let installed = app.join(".luat/packages/@acme/ui");
    assert_eq!(fs::read_to_string(installed.join("src/Card.luat")).unwrap(), "<div>v1</div>");
    assert!(!installed.join("notes.txt").exists(), "only packed files are copied");
    assert!(app.join(".luat/packages/@acme/util/src/init.lua").is_file());
    assert!(app.join(".luat/packages/@acme/icons/src/Star.luat").is_file());

    // Edits show up on the next install; path packages always need one.
    assert!(project.needs_install().unwrap());
    write(&tmp.path().join("libs/ui"), &[("src/Card.luat", "<div>v2</div>")]);
    project.install(true).await.unwrap();
    assert_eq!(fs::read_to_string(installed.join("src/Card.luat")).unwrap(), "<div>v2</div>");
}

#[tokio::test]
async fn path_dependency_names_must_match() {
    let (tmp, url, _app) = setup().await;
    let app = project(tmp.path(), "wrong", &url, "\"@acme/wrong\" = { path = \"../libs/ui\" }\n");
    let err = Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap_err().to_string();
    assert!(err.contains("@acme/wrong") && err.contains("which is package @acme/ui"), "{err}");
}

#[tokio::test]
async fn packages_with_path_dependencies_cannot_be_published() {
    let (tmp, _url, _app) = setup().await;
    let err = publish(&tmp.path().join("libs/ui"), true, &settings(tmp.path())).await.unwrap_err().to_string();
    assert!(err.contains("path dependencies (@acme/util)"), "{err}");
}

#[cfg(unix)]
#[tokio::test]
async fn path_packages_never_copy_symlinks() {
    let (tmp, _url, app) = setup().await;
    let secret = tmp.path().join("secret.lua");
    fs::write(&secret, "return 'secret'").unwrap();
    std::os::unix::fs::symlink(&secret, tmp.path().join("libs/ui/src/leak.lua")).unwrap();
    Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap();
    assert!(!app.join(".luat/packages/@acme/ui/src/leak.lua").exists());
}

#[tokio::test]
async fn add_path_writes_an_inline_table() {
    let (tmp, url, _app) = setup().await;
    let app = project(tmp.path(), "fresh", &url, "");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    project.add_path(&PackageName::parse("@acme/util").unwrap(), "../libs/util").await.unwrap();
    let manifest = fs::read_to_string(app.join("luat.toml")).unwrap();
    assert!(manifest.contains("\"@acme/util\" = { path = \"../libs/util\" }"), "{manifest}");
}
