// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! End to end against an in-process registry: publish, add, lockfile,
//! frozen installs, yanking, conflicts, checksums, hostile tarballs and a
//! built bundle using installed packages.

mod support;

use std::fs;

use luat::bundle::{build, BuildOptions};
use luat::packages::{login, search, yank, Lockfile, PackageError, PackageName, Project, Settings};
use luat::{LuatRequest, LuatResponse};
use support::{auth_settings, project, publish_package, raw_tarball, registry, settings, write};

async fn publish_acme(root: &std::path::Path, url: &str) {
    publish_package(root, url, "@acme/icons", "2.0.0", &[], &[("src/Star.luat", "<i>*</i>")]).await;
    publish_package(
        root,
        url,
        "@acme/ui",
        "1.2.0",
        &[("@acme/icons", "^2.0")],
        &[
            ("src/init.lua", "local h = require('./helper')\nreturn { greet = h.greet }"),
            ("src/helper.lua", "return { greet = function(n) return 'hi ' .. n end }"),
            (
                "src/Card.luat",
                "<script>\nlocal Star = require(\"@acme/icons/Star\")\n</script>\n<div class=\"card\"><Star />{props.title}</div>",
            ),
        ],
    )
    .await;
}

fn version_of(dir: &std::path::Path, name: &str) -> String {
    let lock = Lockfile::load(dir).unwrap().unwrap();
    lock.get(&PackageName::parse(name).unwrap()).unwrap().version.to_string()
}

#[tokio::test]
async fn publish_add_install_and_build() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    publish_acme(tmp.path(), &url).await;

    let app = project(tmp.path(), "app", &url, "");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    assert!(!project.needs_install().unwrap());
    let report = project.add("@acme/ui").await.unwrap();
    assert_eq!(report.installed.len(), 2);

    let manifest = fs::read_to_string(app.join("luat.toml")).unwrap();
    assert!(manifest.contains("\"@acme/ui\" = \"^1.2.0\""), "{manifest}");
    let lock_text = fs::read_to_string(app.join("luat.lock")).unwrap();
    let lock = Lockfile::parse(&lock_text).unwrap();
    assert_eq!(lock.packages.len(), 2);
    let ui = lock.get(&PackageName::parse("@acme/ui").unwrap()).unwrap();
    assert_eq!(ui.registry.as_deref(), Some(url.as_str()));
    assert!(ui.checksum.as_deref().unwrap().starts_with("sha256:"));
    assert_eq!(ui.dependencies, [PackageName::parse("@acme/icons").unwrap()]);
    assert!(lock_text.starts_with("version = 1\n"), "{lock_text}");
    assert!(app.join(".luat/packages/@acme/ui/src/Card.luat").is_file());
    assert!(app.join(".luat/packages/@acme/icons/luat.toml").is_file());
    assert!(!project.needs_install().unwrap());

    // A frozen install from the lockfile alone reproduces the same tree.
    fs::remove_dir_all(app.join(".luat")).unwrap();
    assert!(project.needs_install().unwrap());
    project.install(true).await.unwrap();
    assert_eq!(fs::read_to_string(app.join("luat.lock")).unwrap(), lock_text);
    assert!(app.join(".luat/packages/@acme/icons/src/Star.luat").is_file());

    // A bundle built with the packages renders without .luat at runtime.
    write(
        &app,
        &[(
            "src/routes/+page.luat",
            "<script>\nlocal Card = require(\"@acme/ui/Card\")\nlocal ui = require(\"@acme/ui\")\n</script>\n<Card title={ui.greet(\"bundle\")} />",
        )],
    );
    let options = BuildOptions {
        routes_dir: app.join("src/routes"),
        packages_dir: Some(project.packages_dir()),
        ..Default::default()
    };
    let output = build(&options, |_, _| {}).unwrap();
    assert!(output.warnings.is_empty(), "{:?}", output.warnings);
    fs::remove_dir_all(app.join(".luat")).unwrap();
    let app = output.bundle.instantiate().unwrap();
    let route = app.router.match_url("/").unwrap();
    app.engine.set_development_mode(true).unwrap();
    let LuatResponse::Html { body, .. } = app.engine.respond(&route, &LuatRequest::new("/", "GET")).unwrap() else {
        panic!("html")
    };
    assert_eq!(body.trim(), "<div class=\"card\"><i>*</i>hi bundle</div>");
}

#[tokio::test]
async fn frozen_install_needs_a_current_lockfile() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    publish_acme(tmp.path(), &url).await;
    let app = project(tmp.path(), "app", &url, "\"@acme/ui\" = \"^1\"\n");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    let err = project.install(true).await.unwrap_err();
    assert!(matches!(err, PackageError::Frozen(_)), "{err}");
    project.install(false).await.unwrap();
    project.install(true).await.unwrap();
}

#[tokio::test]
async fn yanked_versions_are_skipped_unless_locked() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    let s = auth_settings(tmp.path(), &url);
    for v in ["2.0.0", "2.1.0"] {
        publish_package(tmp.path(), &url, "@acme/icons", v, &[], &[("src/init.lua", "return {}")]).await;
    }
    let a = project(tmp.path(), "a", &url, "");
    let pa = Project::new(&a).with_settings(s.clone());
    pa.add("@acme/icons@^2.0").await.unwrap();
    assert_eq!(version_of(&a, "@acme/icons"), "2.1.0");

    yank(&url, &PackageName::parse("@acme/icons").unwrap(), &semver::Version::new(2, 1, 0), false, &s)
        .await
        .unwrap();

    // Locked: still installable, even frozen from scratch.
    fs::remove_dir_all(a.join(".luat")).unwrap();
    pa.install(true).await.unwrap();
    assert_eq!(version_of(&a, "@acme/icons"), "2.1.0");

    // New resolutions skip it.
    let b = project(tmp.path(), "b", &url, "");
    Project::new(&b).with_settings(s.clone()).add("@acme/icons").await.unwrap();
    assert_eq!(version_of(&b, "@acme/icons"), "2.0.0");
    pa.update(None).await.unwrap();
    assert_eq!(version_of(&a, "@acme/icons"), "2.0.0");
}

#[tokio::test]
async fn version_conflicts_name_both_requirements() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    publish_package(tmp.path(), &url, "@acme/icons", "1.0.0", &[], &[]).await;
    publish_acme(tmp.path(), &url).await;
    let app = project(tmp.path(), "app", &url, "\"@acme/icons\" = \"^1.0\"\n\"@acme/ui\" = \"^1.2\"\n");
    let err = Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap_err().to_string();
    assert!(err.contains("^1.0 (required by luat.toml)"), "{err}");
    assert!(err.contains("^2.0 (required by @acme/ui@1.2.0)"), "{err}");
    assert!(!app.join("luat.lock").exists());
}

#[tokio::test]
async fn checksum_mismatches_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, store) = registry::start().await;
    publish_acme(tmp.path(), &url).await;
    store.lock().unwrap().tampered.insert("@acme/icons@2.0.0".into());
    let app = project(tmp.path(), "app", &url, "\"@acme/ui\" = \"^1\"\n");
    let err = Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap_err();
    assert!(matches!(err, PackageError::ChecksumMismatch { .. }), "{err}");
    assert!(!app.join(".luat/packages/@acme/icons").exists());
}

#[tokio::test]
async fn hostile_tarballs_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, store) = registry::start().await;
    let cases = [
        ("@evil/dotdot", raw_tarball("package/../../escaped.lua", tar::EntryType::Regular, None)),
        ("@evil/abs", raw_tarball("/tmp/luat-absolute.lua", tar::EntryType::Regular, None)),
        ("@evil/link", raw_tarball("package/src/link.lua", tar::EntryType::Symlink, Some("/etc/passwd"))),
        ("@evil/hard", raw_tarball("package/src/hard.lua", tar::EntryType::Link, Some("package/luat.toml"))),
        ("@evil/outside", raw_tarball("other/x.lua", tar::EntryType::Regular, None)),
    ];
    for (name, bytes) in cases {
        registry::inject(&store, name, "1.0.0", bytes);
        let app = project(tmp.path(), &name.replace(['@', '/'], "_"), &url, &format!("\"{name}\" = \"^1\"\n"));
        let err = Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap_err();
        assert!(matches!(err, PackageError::Tarball(_)), "{name}: {err}");
        assert!(!app.join(".luat/packages").join(name).exists(), "{name}");
        assert!(!tmp.path().join("escaped.lua").exists() && !app.join(".luat/escaped.lua").exists());
    }
}

#[tokio::test]
async fn login_search_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let (url, _store) = registry::start().await;
    publish_acme(tmp.path(), &url).await;
    let creds = tmp.path().join("creds/credentials.toml");
    let s = Settings {
        credentials_path: Some(creds.clone()),
        ..Default::default()
    };
    let err = login(&url, "wrong", &s).await.unwrap_err().to_string();
    assert!(err.contains("rejected the token"), "{err}");
    let err = login(&url, "unbound", &s).await.unwrap_err().to_string();
    assert!(err.contains("not bound to a user"), "{err}");
    assert!(!creds.exists());
    let me = login(&url, registry::TOKEN, &s).await.unwrap();
    assert_eq!(me.user, "tester");
    assert!(fs::read_to_string(&creds).unwrap().contains(registry::TOKEN));

    let found = search(&url, "ui", 1, 20, &s).await.unwrap();
    assert_eq!(found.packages.len(), 1);
    assert_eq!(found.packages[0].name, "@acme/ui");

    let app = project(tmp.path(), "app", &url, "");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    project.add("@acme/ui@^1.2").await.unwrap();
    let report = project.remove(&PackageName::parse("@acme/ui").unwrap()).await.unwrap();
    assert_eq!(report.removed.len(), 2);
    assert!(!fs::read_to_string(app.join("luat.toml")).unwrap().contains("@acme/ui"));
    assert!(Lockfile::load(&app).unwrap().unwrap().packages.is_empty());
}
