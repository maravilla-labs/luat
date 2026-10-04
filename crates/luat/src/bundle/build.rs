// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Building a bundle from a project on disk.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::{emit, walk, Bundle, BundleHeader};
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::lua_comments::blank_comments;
use crate::parser::parse_template;
use crate::resolver::{FileSystemResolver, ResourceResolver};
use crate::router::Router;
use crate::sourcemap::BundleSourceMap;

/// An extra directory of modules to include, reachable as
/// `require("<prefix>/<path>")`.
#[derive(Debug, Clone)]
pub struct ModuleDir {
    /// Module key prefix, e.g. `"jobs"`.
    pub prefix: String,
    /// Directory holding the modules.
    pub dir: PathBuf,
}

/// What to build.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Directory with `+page.luat`, `+server.lua`, … files.
    pub routes_dir: PathBuf,
    /// Library directory, reachable as `$lib/...` / `lib/...`.
    pub lib_dir: Option<PathBuf>,
    /// The app shell (`src/app.html`), embedded into the bundle.
    pub app_html: Option<PathBuf>,
    /// Further module directories (handlers, jobs, …) to include.
    pub module_dirs: Vec<ModuleDir>,
    /// Modules the host registers at runtime (`Engine::register_module`).
    /// Requires of these names are left to the host instead of being
    /// resolved (and warned about) at build time.
    pub host_modules: Vec<String>,
    /// Directory of installed packages (by convention
    /// `<project>/.luat/packages`, holding `@scope/name/` package roots).
    ///
    /// When set, every installed package's `src/` is compiled into the
    /// bundle under the module key `@scope/name/<path inside src>`, and
    /// `require("@scope/name")` / `require("@scope/name/Card")` resolve as
    /// described in [`crate::package_paths`], so the bundle needs no
    /// packages directory at runtime.
    pub packages_dir: Option<PathBuf>,
    /// Client assets built for this bundle (see [`crate::assets`]). They are
    /// embedded, so templates can call `asset("src/client/app.js")` and the
    /// app shell gets their tags in `%luat.head%`.
    #[cfg(not(target_arch = "wasm32"))]
    pub assets: Option<crate::assets::AssetManifest>,
}

/// The result of a build.
#[derive(Debug)]
pub struct BuildOutput {
    /// The bundle.
    pub bundle: Bundle,
    /// Maps bundle line numbers back to source files, for error messages.
    pub source_map: BundleSourceMap,
    /// Number of routes discovered.
    pub route_count: usize,
    /// Number of compiled templates.
    pub template_count: usize,
    /// Number of server and library Lua sources included.
    pub server_source_count: usize,
    /// Problems that did not stop the build (unresolved requires, …).
    pub warnings: Vec<String>,
}

/// A source file collected for the bundle.
struct SourceFile {
    /// Module key inside the bundle, e.g. `blog/+page.luat`, `lib/db.lua`.
    key: String,
    /// Canonical absolute path.
    abs: PathBuf,
    content: String,
}

impl SourceFile {
    fn is_template(&self) -> bool {
        self.key.ends_with(".luat")
    }
}

/// Builds a bundle from the project described by `options`.
///
/// `progress` is called with `(done, total)` while templates compile.
pub fn build(options: &BuildOptions, progress: impl FnMut(usize, usize)) -> Result<BuildOutput> {
    let mut resolver = FileSystemResolver::new(&options.routes_dir);
    if let Some(lib_dir) = &options.lib_dir {
        resolver = resolver.with_lib_dir(lib_dir);
    }
    if let Some(packages_dir) = &options.packages_dir {
        resolver = resolver.with_packages_dir(packages_dir);
    }
    let engine = Engine::with_memory_cache(resolver, 100)?;

    let mut warnings = Vec::new();
    let mut files = collect(&options.routes_dir, "", &mut warnings)?;
    if let Some(lib_dir) = options.lib_dir.as_deref().filter(|d| d.exists()) {
        files.extend(collect(lib_dir, "lib", &mut warnings)?);
    }
    for module_dir in &options.module_dirs {
        if !walk::is_plain_prefix(&module_dir.prefix) {
            return Err(LuatError::InvalidTemplate(format!(
                "module directory prefix '{}' must be plain path segments",
                module_dir.prefix
            )));
        }
        files.extend(collect(&module_dir.dir, &module_dir.prefix, &mut warnings)?);
    }
    if let Some(packages_dir) = &options.packages_dir {
        for (package, src) in walk::installed_packages(packages_dir, &mut warnings)? {
            files.extend(collect(&src, &package, &mut warnings)?);
        }
    }

    let require_map = require_map(&files, engine.resolver(), &options.host_modules, &mut warnings);

    let (templates, server_sources): (Vec<_>, Vec<_>) = files.into_iter().partition(SourceFile::is_template);
    let template_count = templates.len();
    let server_sources: Vec<(String, String)> =
        server_sources.into_iter().map(|f| (f.key, f.content)).collect();

    let routes: Vec<String> = walk::walk(&options.routes_dir, &mut Vec::new())?
        .into_iter()
        .map(|f| f.rel)
        .collect();
    let route_count = Router::from_paths(routes.iter()).routes().len();

    let app_html = match &options.app_html {
        Some(path) if path.exists() => Some(fs::read_to_string(path)?),
        _ => None,
    };

    let template_sources = templates.into_iter().map(|f| (f.key, f.content)).collect();
    let (mut source, mut source_map) = engine.bundle_sources(template_sources, progress)?;

    let data = [
        emit::require_map(&require_map),
        emit::server_sources(&server_sources),
        emit::route_files(&routes),
        emit::app_html(app_html.as_deref()),
        emit::assets(options),
    ]
    .join("\n");
    // Data goes before the final `return __modules`, after all module
    // code, so module line offsets stay valid.
    match source.rfind("return __modules") {
        Some(pos) => source.insert_str(pos, &format!("{data}\n")),
        None => source.push_str(&format!("\n{data}")),
    }

    let header = BundleHeader::current().to_line();
    source = format!("{header}\n{source}");
    source_map.adjust_offsets(1);

    let bundle = Bundle::from_source(source)?;
    // Fail the build, not the first request, if the bundle doesn't load.
    engine
        .compile_bundle(bundle.source())
        .map_err(|e| LuatError::InvalidTemplate(source_map.translate_error(&e.to_string())))?;

    Ok(BuildOutput {
        bundle,
        source_map,
        route_count,
        template_count,
        server_source_count: server_sources.len(),
        warnings,
    })
}

/// Collects `.luat` and `.lua` files under `dir`, keyed by `prefix/relative`.
/// Symlinks and anything resolving outside `dir` are skipped with a warning.
fn collect(dir: &Path, prefix: &str, warnings: &mut Vec<String>) -> Result<Vec<SourceFile>> {
    walk::walk(dir, warnings)?
        .into_iter()
        .filter(|f| f.rel.ends_with(".luat") || f.rel.ends_with(".lua"))
        .map(|f| {
            Ok(SourceFile {
                content: walk::read_contained(dir, &f)?,
                key: if prefix.is_empty() { f.rel } else { format!("{prefix}/{}", f.rel) },
                abs: f.abs,
            })
        })
        .collect()
}

/// Resolves every literal `require("...")` at build time, so bundles do not
/// depend on the filesystem layout at runtime.
fn require_map(
    files: &[SourceFile],
    resolver: &dyn ResourceResolver,
    host_modules: &[String],
    warnings: &mut Vec<String>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let key_by_path: HashMap<&Path, &str> = files.iter().map(|f| (f.abs.as_path(), f.key.as_str())).collect();
    let literal = Regex::new(r#"require\s*\(\s*["']([^"']+)["']\s*\)"#).expect("valid regex");
    let any_call = Regex::new(r#"require\s*\("#).expect("valid regex");

    let mut map: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for file in files {
        let (code, requires) = match requires_of(file, &literal) {
            Ok(found) => found,
            Err(e) => {
                warnings.push(format!("{}: {}", file.key, e));
                continue;
            }
        };
        if any_call.find_iter(&code).count() != literal.captures_iter(&code).count() {
            warnings.push(format!(
                "{}: non-literal require() calls are resolved at runtime only",
                file.key
            ));
        }

        let importer = file.abs.to_string_lossy();
        for name in requires.into_iter().filter(|n| !host_modules.contains(n)) {
            let resolved = resolver
                .get_resolved_path(&importer, &name)
                .map_err(|e| e.to_string())
                .and_then(|p| fs::canonicalize(&p).map_err(|e| format!("{p}: {e}")));
            match resolved {
                Ok(abs) => match key_by_path.get(abs.as_path()) {
                    Some(key) => {
                        map.entry(file.key.clone())
                            .or_default()
                            .insert(name, key.to_string());
                    }
                    None => warnings.push(format!(
                        "{}: require('{}') resolves to {}, which is not part of the bundle",
                        file.key,
                        name,
                        abs.display()
                    )),
                },
                // Names that match a bundle module key directly (e.g. a
                // module directory prefix) are resolved by the bundle at
                // runtime.
                Err(_) if bundle_key_exists(&name, &key_by_path) => {}
                Err(e) => warnings.push(format!("{}: unresolved require('{}'): {}", file.key, name, e)),
            }
        }
    }
    map
}

/// True when `name` (with `$lib/` expanded, with or without extension)
/// is the key of a module in the bundle.
fn bundle_key_exists(name: &str, key_by_path: &HashMap<&Path, &str>) -> bool {
    let name = name.strip_prefix("$lib/").map(|rest| format!("lib/{rest}")).unwrap_or_else(|| name.to_string());
    let candidates = [name.clone(), format!("{name}.lua"), format!("{name}.luat")];
    key_by_path.values().any(|key| candidates.iter().any(|c| c == key))
}

/// Returns the Lua code of `file` (script blocks for templates) with its
/// comments blanked, and the names it requires. Requires inside comments
/// are not dependencies.
fn requires_of(file: &SourceFile, literal: &Regex) -> std::result::Result<(String, Vec<String>), String> {
    if !file.is_template() {
        let code = blank_comments(&file.content);
        let names = literal.captures_iter(&code).map(|c| c[1].to_string()).collect();
        return Ok((code, names));
    }
    let ast = parse_template(&file.content).map_err(|e| e.to_string())?;
    let code = [ast.module_script.as_ref(), ast.regular_script.as_ref()]
        .into_iter()
        .flatten()
        .map(|s| s.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok((blank_comments(&code), ast.imports))
}
