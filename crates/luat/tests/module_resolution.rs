// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Module names resolve case-exactly on every filesystem, the same in
//! development, in bundles and in packages; `require` inside comments is not
//! a dependency.

#![cfg(feature = "filesystem")]

use std::fs;
use std::path::Path;

use luat::bundle::{build, BuildOptions};
use luat::{FileSystemResolver, ResourceResolver};

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

/// `Variants.luat` (a component) next to `variants.lua` (a module). On a
/// case-insensitive filesystem `variants.luat` "exists", and since `.luat`
/// is tried before `.lua`, `require("./variants")` used to load the
/// component.
#[test]
fn wrong_case_template_does_not_shadow_a_module() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            ("Variants.luat", "<p>component</p>"),
            ("variants.lua", "return {}"),
            ("Only.luat", "<p/>"),
        ],
    );
    let resolver = FileSystemResolver::new(dir.path());
    let resolved = resolver.resolve("", "./variants").unwrap();
    assert!(resolved.path.ends_with("variants.lua"), "{}", resolved.path);
    let resolved = resolver.resolve("", "./Variants").unwrap();
    assert!(
        resolved.path.ends_with("Variants.luat"),
        "{}",
        resolved.path
    );

    assert!(resolver.resolve("", "./only").is_err());
    assert!(resolver.resolve("", "./only.luat").is_err());
    assert!(resolver.resolve("", "only").is_err());
    assert!(resolver.resolve("", "Only").is_ok());
}

#[test]
fn directory_case_must_match() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            ("Components/Card.luat", "<p/>"),
            ("lib/Util.lua", "return {}"),
        ],
    );
    let resolver = FileSystemResolver::new(dir.path()).with_lib_dir(dir.path().join("lib"));
    assert!(resolver.resolve("", "./Components/Card").is_ok());
    assert!(resolver.resolve("", "./components/Card").is_err());
    assert!(resolver.resolve("", "$lib/Util").is_ok());
    assert!(resolver.resolve("", "$lib/util").is_err());
}

#[test]
fn bundle_build_uses_exact_case_and_skips_commented_requires() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        &[
            (
                "routes/+page.server.lua",
                "-- local old = require('./gone')\n--[[ require(\"./gone_too\")\n]]\nlocal s = '-- not a comment'\nlocal v = require('./helper')\nfunction load() return { n = v.n } end",
            ),
            ("routes/Helper.luat", "<p/>"),
            ("routes/helper.lua", "return { n = 1 }"),
            (
                "routes/+page.luat",
                "<script>\n-- local Old = require('./Old')\nlocal Card = require('./Card') --[==[ require('./Older') ]==]\n</script><Card />",
            ),
            ("routes/Card.luat", "<b>card</b>"),
            ("routes/wrong/+server.lua", "local h = require('./Thing')\nfunction GET() return { body = 'x' } end"),
            ("routes/wrong/thing.lua", "return {}"),
        ],
    );
    let options = BuildOptions {
        routes_dir: dir.path().join("routes"),
        ..Default::default()
    };
    let output = build(&options, |_, _| {}).unwrap();
    let warnings = output.warnings.join("\n");
    for commented in ["gone", "gone_too", "Old'", "Older"] {
        assert!(!warnings.contains(commented), "{commented}: {warnings}");
    }
    // `./Thing` does not match `thing.lua` on any filesystem.
    assert!(warnings.contains("require('./Thing')"), "{warnings}");
    assert_eq!(output.warnings.len(), 1, "{warnings}");

    let source = output.bundle.source();
    assert!(
        source.contains("[\"./helper\"] = \"helper.lua\""),
        "require map points at helper.lua"
    );
}

#[test]
fn package_modules_need_exact_case() {
    let dir = tempfile::tempdir().unwrap();
    let pkg = ".luat/packages/@acme/ui";
    write(
        dir.path(),
        &[
            (
                &format!("{pkg}/luat.toml"),
                "[package]\nname = \"@acme/ui\"\nversion = \"1.0.0\"\n",
            ),
            (&format!("{pkg}/src/Card.luat"), "<p/>"),
            (&format!("{pkg}/src/forms/Field.lua"), "return {}"),
            ("routes/+page.luat", "<p/>"),
        ],
    );
    let packages = dir.path().join(".luat/packages");
    let found = |name: &str| luat::package_paths::resolve_package_module(&packages, name).is_ok();
    assert!(found("@acme/ui/Card"));
    assert!(!found("@acme/ui/card"));
    assert!(found("@acme/ui/forms/Field"));
    assert!(!found("@acme/ui/Forms/Field"));
}

/// `$lib/blocks/loaders/Card` next to `$lib/blocks/Card.luat`: with the lib
/// directory given relative to the working directory, the alias used to miss
/// and fall back to the bare name `Card`, loading the component instead of
/// the module.
#[test]
fn lib_alias_does_not_fall_back_to_a_same_named_file() {
    let dir = tempfile::tempdir_in(".").unwrap();
    let rel = Path::new(".").join(dir.path().file_name().unwrap());
    write(
        &rel,
        &[
            ("src/routes/+page.luat", "<p/>"),
            ("src/lib/blocks/Card.luat", "<p>component</p>"),
            ("src/lib/blocks/registry.lua", "return {}"),
            ("src/lib/blocks/loaders/Card.lua", "return {}"),
        ],
    );
    let resolver = FileSystemResolver::new(rel.join("src/routes")).with_lib_dir(rel.join("src/lib"));
    let importer = rel.join("src/lib/blocks/registry.lua");
    let resolved = resolver
        .get_resolved_path(&importer.to_string_lossy(), "$lib/blocks/loaders/Card")
        .unwrap();
    assert!(resolved.ends_with("blocks/loaders/Card.lua"), "{resolved}");
    let resolved = resolver
        .get_resolved_path(&importer.to_string_lossy(), "$lib/blocks/Card")
        .unwrap();
    assert!(resolved.ends_with("blocks/Card.luat"), "{resolved}");
    assert!(resolver
        .get_resolved_path(&importer.to_string_lossy(), "$lib/blocks/loaders/Missing")
        .is_err());
}
