// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! A cloned project is untrusted: its luat.toml and luat.lock must not leak
//! tokens, redirect downloads or make the installer write outside
//! `.luat/packages`.

mod support;

use std::fs;

use luat::packages::{login, publish, PackageError, PackageManifest, Project};
use support::{auth_settings, project, publish_package, registry, settings, write};

async fn published() -> (tempfile::TempDir, String, registry::Shared) {
    let tmp = tempfile::tempdir().unwrap();
    let (url, store) = registry::start().await;
    publish_package(tmp.path(), &url, "@acme/icons", "2.0.0", &[], &[("src/Star.luat", "<i>*</i>")]).await;
    store.lock().unwrap().authorized_requests.clear();
    (tmp, url, store)
}

#[tokio::test]
async fn installs_never_send_credentials() {
    let (tmp, url, store) = published().await;
    let app = project(tmp.path(), "app", &url, "");
    // Even with a token bound to exactly this registry.
    let project = Project::new(&app).with_settings(auth_settings(tmp.path(), &url));
    project.add("@acme/icons").await.unwrap();
    fs::remove_dir_all(app.join(".luat")).unwrap();
    project.install(false).await.unwrap();
    project.update(None).await.unwrap();
    assert_eq!(store.lock().unwrap().authorized_requests, Vec::<String>::new());
}

#[tokio::test]
async fn tokens_only_go_to_the_registry_they_belong_to() {
    let (tmp, url, store) = published().await;
    let (other_url, other) = registry::start().await;
    // A package whose luat.toml points its scope at another registry.
    let dir = tmp.path().join("pkg");
    write(
        &dir,
        &[(
            "luat.toml",
            &format!("[package]\nname = \"@acme/x\"\nversion = \"1.0.0\"\n\n[registries]\ndefault = \"{other_url}\"\n"),
        )],
    );
    let err = publish(&dir, false, &auth_settings(tmp.path(), &url)).await.unwrap_err();
    assert!(matches!(err, PackageError::NoToken(_)), "{err}");
    assert!(other.lock().unwrap().authorized_requests.is_empty());
    assert!(store.lock().unwrap().authorized_requests.is_empty());
}

#[tokio::test]
async fn authenticated_requests_do_not_follow_redirects() {
    let (tmp, url, store) = published().await;
    let (other_url, other) = registry::start().await;
    store.lock().unwrap().redirect_me = Some(other_url);
    let err = login(&url, registry::TOKEN, &settings(tmp.path())).await.unwrap_err().to_string();
    assert!(err.contains("redirect"), "{err}");
    assert!(other.lock().unwrap().authorized_requests.is_empty());
    assert!(!tmp.path().join("credentials.toml").exists());
}

#[test]
fn plain_http_registries_are_refused_except_localhost() {
    let parse = |url: &str| PackageManifest::parse(&format!("[registries]\ndefault = \"{url}\"\n"));
    assert!(parse("http://registry.example.com").is_err());
    assert!(parse("http://127.0.0.1:8080").is_ok());
    assert!(parse("https://registry.example.com").is_ok());
}

#[tokio::test]
async fn lockfiles_cannot_redirect_a_scope_to_another_registry() {
    let (tmp, url, _store) = published().await;
    let (other_url, other) = registry::start().await;
    let app = project(tmp.path(), "app", &url, "");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    project.add("@acme/icons").await.unwrap();
    let lock = fs::read_to_string(app.join("luat.lock")).unwrap();
    fs::write(app.join("luat.lock"), lock.replace(&url, &other_url)).unwrap();
    fs::remove_dir_all(app.join(".luat")).unwrap();
    for frozen in [true, false] {
        let err = project.install(frozen).await.unwrap_err().to_string();
        assert!(err.contains("pins @acme/icons to registry"), "{err}");
    }
    assert!(other.lock().unwrap().packages.is_empty());
    // `luat update` re-resolves from luat.toml and repairs it.
    project.update(None).await.unwrap();
    assert!(fs::read_to_string(app.join("luat.lock")).unwrap().contains(&url));
}

#[tokio::test]
async fn lockfiles_cannot_add_path_sources() {
    let (tmp, url, _store) = published().await;
    let app = project(tmp.path(), "app", &url, "");
    write(
        &app,
        &[("luat.lock", "version = 1\n\n[[package]]\nname = \"@acme/evil\"\nversion = \"1.0.0\"\nsource = \"path+../../..\"\ndependencies = []\n")],
    );
    let err = Project::new(&app).with_settings(settings(tmp.path())).install(false).await.unwrap_err().to_string();
    assert!(err.contains("does not declare"), "{err}");
}

#[tokio::test]
async fn lockfile_checksums_must_match_the_index() {
    let (tmp, url, store) = published().await;
    let app = project(tmp.path(), "app", &url, "");
    let project = Project::new(&app).with_settings(settings(tmp.path()));
    project.add("@acme/icons").await.unwrap();
    fs::remove_dir_all(app.join(".luat")).unwrap();
    // The registry now lists another checksum than the lockfile pins.
    store.lock().unwrap().packages.get_mut("@acme/icons").unwrap()[0].0.cksum = format!("sha256:{}", "0".repeat(64));
    let err = project.install(true).await.unwrap_err();
    assert!(matches!(&err, PackageError::ChecksumMismatch { version, .. } if version.contains("index")), "{err}");
    assert!(!app.join(".luat/packages/@acme/icons").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn installs_never_write_through_symlinks() {
    use std::os::unix::fs::symlink;
    let (tmp, url, _store) = published().await;
    let outside = tempfile::tempdir().unwrap();
    let cases: [(&str, &dyn Fn(&std::path::Path)); 4] = [
        (".luat", &|app| symlink(outside.path(), app.join(".luat")).unwrap()),
        (".luat/packages", &|app| {
            fs::create_dir_all(app.join(".luat")).unwrap();
            symlink(outside.path(), app.join(".luat/packages")).unwrap();
        }),
        ("scope", &|app| {
            fs::create_dir_all(app.join(".luat/packages")).unwrap();
            symlink(outside.path(), app.join(".luat/packages/@acme")).unwrap();
        }),
        ("package", &|app| {
            fs::create_dir_all(app.join(".luat/packages/@acme")).unwrap();
            symlink(outside.path(), app.join(".luat/packages/@acme/icons")).unwrap();
        }),
    ];
    for (i, (what, plant)) in cases.iter().enumerate() {
        let app = project(tmp.path(), &format!("app{i}"), &url, "\"@acme/icons\" = \"^2\"\n");
        plant(&app);
        let project = Project::new(&app).with_settings(settings(tmp.path()));
        assert!(project.needs_install().unwrap(), "{what}");
        let err = project.install(false).await.unwrap_err().to_string();
        assert!(err.contains("symlink"), "{what}: {err}");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0, "{what}: wrote outside");
    }
}
