// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! A bundle build never pulls in files from outside the directories it was
//! given: symlinks are skipped and package directories must be valid names.

#![cfg(all(feature = "filesystem", unix))]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use luat::bundle::{build, BuildOptions, ModuleDir};

const SECRET: &str = "return 'TOP SECRET'";

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

/// A project plus a directory outside it holding a "secret" module.
fn setup() -> (tempfile::TempDir, tempfile::TempDir, BuildOptions) {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(outside.path(), &[("secret.lua", SECRET), ("src/secret.lua", SECRET)]);
    write(
        project.path(),
        &[
            ("src/routes/+page.luat", "<p>ok</p>"),
            ("src/lib/util.lua", "return {}"),
            (".luat/packages/@acme/ui/src/init.lua", "return {}"),
        ],
    );
    let options = BuildOptions {
        routes_dir: project.path().join("src/routes"),
        lib_dir: Some(project.path().join("src/lib")),
        packages_dir: Some(project.path().join(".luat/packages")),
        ..Default::default()
    };
    (project, outside, options)
}

fn assert_excluded(options: &BuildOptions, needle: &str) {
    let output = build(options, |_, _| {}).unwrap();
    assert!(!output.bundle.source().contains("TOP SECRET"), "outside file was bundled");
    assert!(
        output.warnings.iter().any(|w| w.contains(needle)),
        "expected a warning mentioning {needle:?}: {:?}",
        output.warnings
    );
}

#[test]
fn symlinked_files_in_lib_are_skipped() {
    let (project, outside, options) = setup();
    symlink(outside.path().join("secret.lua"), project.path().join("src/lib/leak.lua")).unwrap();
    assert_excluded(&options, "leak.lua");
}

#[test]
fn symlinked_directories_in_routes_are_skipped() {
    let (project, outside, options) = setup();
    symlink(outside.path(), project.path().join("src/routes/leak")).unwrap();
    assert_excluded(&options, "leak");
}

#[test]
fn symlinked_packages_are_skipped() {
    let (project, outside, options) = setup();
    symlink(outside.path(), project.path().join(".luat/packages/@acme/evil")).unwrap();
    assert_excluded(&options, "@acme/evil");
}

#[test]
fn symlinked_package_src_is_skipped() {
    let (project, outside, options) = setup();
    let pkg = project.path().join(".luat/packages/@acme/other");
    fs::create_dir_all(&pkg).unwrap();
    symlink(outside.path().join("src"), pkg.join("src")).unwrap();
    assert_excluded(&options, "@acme/other/src");
}

#[test]
fn symlinked_scope_directories_are_skipped() {
    let (project, outside, options) = setup();
    let scope = tempfile::tempdir_in(outside.path()).unwrap();
    write(scope.path(), &[("pkg/src/init.lua", SECRET)]);
    symlink(scope.path(), project.path().join(".luat/packages/@evil")).unwrap();
    assert_excluded(&options, "@evil");
}

#[test]
fn invalid_package_directory_names_are_skipped() {
    let (project, _outside, options) = setup();
    write(
        project.path(),
        &[
            (".luat/packages/@acme/..evil/src/init.lua", SECRET),
            (".luat/packages/@a+b/ui/src/init.lua", SECRET),
            (".luat/packages/acme/ui/src/init.lua", SECRET),
        ],
    );
    let output = build(&options, |_, _| {}).unwrap();
    assert!(!output.bundle.source().contains("TOP SECRET"));
    for needle in ["..evil", "@a+b"] {
        assert!(output.warnings.iter().any(|w| w.contains(needle)), "{needle}: {:?}", output.warnings);
    }
    assert!(output.warnings.iter().any(|w| w.contains("packages/acme:")), "{:?}", output.warnings);
}

#[test]
fn module_dir_prefixes_must_be_plain() {
    let (project, _outside, mut options) = setup();
    for prefix in ["../x", "/abs", "a/../b", ""] {
        options.module_dirs = vec![ModuleDir {
            prefix: prefix.to_string(),
            dir: project.path().join("src/lib"),
        }];
        assert!(build(&options, |_, _| {}).is_err(), "prefix {prefix:?} accepted");
    }
}

#[test]
fn development_resolver_rejects_symlinked_packages() {
    use luat::ResourceResolver;
    let (project, outside, options) = setup();
    write(outside.path(), &[("pkg/src/init.lua", SECRET)]);
    symlink(outside.path().join("pkg"), project.path().join(".luat/packages/@acme/evil")).unwrap();
    let resolver = luat::FileSystemResolver::new(&options.routes_dir)
        .with_packages_dir(project.path().join(".luat/packages"));
    let err = resolver.resolve("", "@acme/evil").unwrap_err();
    assert!(err.to_string().contains("Security"), "{err}");
    assert!(resolver.resolve("", "@acme/ui").is_ok());
}
