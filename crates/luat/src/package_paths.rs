// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Where `require("@scope/name/...")` finds an installed package's modules.
//!
//! Installed packages live in a packages directory (by convention
//! `<project>/.luat/packages`), one package root per `@scope/name`. A
//! require names the package and, optionally, a module inside its `src/`:
//!
//! | require                       | tried, in order (inside the package) |
//! |-------------------------------|--------------------------------------|
//! | `@acme/ui`                    | `src/init.lua`, `src/init.luat` |
//! | `@acme/ui/Card`               | `src/Card.luat`, `src/Card.lua`, `src/Card/init.lua` |
//! | `@acme/ui/forms/Field`        | `src/forms/Field.luat`, … (same order) |
//!
//! These rules need no networking; they are used by the filesystem resolver
//! (development) and by [`bundle::build`](crate::bundle) (production).

/// True when `name` is a valid package scope or package name part:
/// `[a-z0-9][a-z0-9._-]{0,63}`.
pub fn is_valid_name_part(part: &str) -> bool {
    let bytes = part.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
}

/// Splits a package require into the package name (`@scope/name`) and the
/// module path inside the package's `src/` (empty for the bare name).
///
/// Returns `None` when `require_name` is not a package require or is
/// malformed (bad name parts, empty, `.` or `..` segments, backslashes).
pub fn split_package_require(require_name: &str) -> Option<(&str, &str)> {
    let rest = require_name.strip_prefix('@')?;
    let mut parts = rest.splitn(3, '/');
    let scope = parts.next()?;
    let name = parts.next()?;
    if !is_valid_name_part(scope) || !is_valid_name_part(name) {
        return None;
    }
    let package_len = 1 + scope.len() + 1 + name.len();
    let package = &require_name[..package_len];
    let subpath = parts.next().unwrap_or("");
    if require_name.len() > package_len && subpath.is_empty() {
        return None; // trailing slash
    }
    let valid_subpath = subpath.is_empty()
        || subpath
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.contains(['\\', ':', '\0']));
    valid_subpath.then_some((package, subpath))
}

/// Files tried for `subpath` inside a package, relative to the package root,
/// in precedence order.
pub fn package_module_candidates(subpath: &str) -> Vec<String> {
    if subpath.is_empty() {
        return vec!["src/init.lua".to_string(), "src/init.luat".to_string()];
    }
    let mut candidates = Vec::new();
    if subpath.ends_with(".luat") || subpath.ends_with(".lua") {
        candidates.push(format!("src/{subpath}"));
    }
    candidates.extend([
        format!("src/{subpath}.luat"),
        format!("src/{subpath}.lua"),
        format!("src/{subpath}/init.lua"),
    ]);
    candidates
}

/// Resolves a package require against `packages_dir`.
///
/// Returns the canonical path of the module file, `Ok(None)` when
/// `require_name` is not a package require, and an error naming the package
/// when it is one but nothing matches (or the match escapes the package).
#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
pub fn resolve_package_module(
    packages_dir: &std::path::Path,
    require_name: &str,
) -> crate::error::Result<Option<std::path::PathBuf>> {
    use crate::error::LuatError;

    if !require_name.starts_with('@') {
        return Ok(None);
    }
    let (package, subpath) = split_package_require(require_name).ok_or_else(|| {
        LuatError::ResolutionError(format!("'{require_name}' is not a valid package module name"))
    })?;
    let package_root = packages_dir.join(package);
    if !package_root.is_dir() {
        return Err(LuatError::ResolutionError(format!(
            "package '{package}' is not installed (required as '{require_name}'); run `luat install`"
        )));
    }
    let Ok(canonical_root) = std::fs::canonicalize(&package_root) else {
        return Ok(None);
    };
    for candidate in package_module_candidates(subpath) {
        let path = package_root.join(&candidate);
        if !path.is_file() {
            continue;
        }
        let canonical = std::fs::canonicalize(&path).map_err(LuatError::IoError)?;
        if !canonical.starts_with(&canonical_root) {
            return Err(LuatError::ResolutionError(format!(
                "Security: '{require_name}' escapes package '{package}'"
            )));
        }
        return Ok(Some(canonical));
    }
    Err(LuatError::ResolutionError(format!(
        "module '{require_name}' not found in package '{package}' (tried {})",
        package_module_candidates(subpath).join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_package_requires() {
        assert_eq!(split_package_require("@acme/ui"), Some(("@acme/ui", "")));
        assert_eq!(split_package_require("@acme/ui/Card"), Some(("@acme/ui", "Card")));
        assert_eq!(split_package_require("@acme/ui/forms/Field"), Some(("@acme/ui", "forms/Field")));
        for bad in ["acme/ui", "@acme", "@Acme/ui", "@acme/ui/", "@acme/ui/../x", "@acme/ui//x", "@acme/ui/./x"] {
            assert_eq!(split_package_require(bad), None, "{bad}");
        }
    }

    #[test]
    fn candidates_follow_the_documented_order() {
        assert_eq!(package_module_candidates(""), ["src/init.lua", "src/init.luat"]);
        assert_eq!(
            package_module_candidates("Card"),
            ["src/Card.luat", "src/Card.lua", "src/Card/init.lua"]
        );
    }

    #[test]
    fn name_parts() {
        assert!(is_valid_name_part("ui"));
        assert!(is_valid_name_part("0x.y_z-1"));
        assert!(!is_valid_name_part("-ui"));
        assert!(!is_valid_name_part("UI"));
        assert!(!is_valid_name_part(&"a".repeat(65)));
        assert!(is_valid_name_part(&"a".repeat(64)));
    }
}
