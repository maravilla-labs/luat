// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Build command for compiling LUAT templates into a production bundle.

use crate::config::Config;
use crate::router::Router as LuatRouter;
use crate::toolchain::{build::BuildOrchestrator, prepare_build_tools};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use luat::{Engine, FileSystemResolver, ResourceResolver, parse_template};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Runs the build command to compile templates into a production bundle.
pub async fn run(source: bool, output: &str) -> anyhow::Result<()> {
    let config = Config::load()?;
    let templates_dir = &config.dev.templates_dir;
    let working_dir = std::env::current_dir()?;

    // Run frontend build if any tools are enabled
    let enabled_tools = config.frontend.get_enabled_tools();
    if !enabled_tools.is_empty() {
        println!("Building frontend assets...");

        // Download/ensure tools are available
        let tool_paths = prepare_build_tools(&config.frontend, false).await?;

        // Create build orchestrator for production build
        let mut orchestrator =
            BuildOrchestrator::new(config.frontend.clone(), working_dir.clone(), true, false);

        // Register tools with orchestrator
        for (tool, path) in &tool_paths {
            orchestrator.register_tool(*tool, path.clone());
        }

        // Run production build
        orchestrator.build_all().await?;
        println!();
    }

    // Use routes_dir for SvelteKit-style routing, templates_dir for simplified mode
    let source_dir = if config.routing.simplified {
        templates_dir.clone()
    } else {
        config.routing.routes_dir.clone()
    };

    println!(
        "{} {}",
        style("Building templates from:").cyan(),
        source_dir
    );

    // Create output directory
    fs::create_dir_all(output)?;

    // Create engine
    let routes_root = working_dir.join(&source_dir);
    let lib_root = working_dir.join(&config.routing.lib_dir);
    let resolver = FileSystemResolver::new(&routes_root).with_lib_dir(&lib_root);
    let mut engine = Engine::with_memory_cache(resolver, 100)?;
    // Set root path for readable error messages (show relative paths)
    engine.set_root_path(&working_dir);

    // Discover routes for SvelteKit-style routing
    let routes_dir = working_dir.join(&source_dir);
    let router = if !config.routing.simplified && routes_dir.exists() {
        Some(LuatRouter::discover(&routes_dir)?)
    } else {
        None
    };

    // Collect template files (.luat) - will be compiled
    let mut sources = Vec::new();
    // Collect server files (.lua) - will be stored as raw source
    let mut server_sources: Vec<(String, String)> = Vec::new();
    let mut source_paths: Vec<(String, String, bool)> = Vec::new(); // (module_key, abs_path, is_template)
    let mut path_map: HashMap<String, String> = HashMap::new(); // canonical path -> module_key

    // Collect all .luat template files
    let pattern = format!("{}/**/*.luat", source_dir);
    for path in (glob::glob(&pattern)?).flatten() {
        let relative = path.strip_prefix(&source_dir)?;
        let content = fs::read_to_string(&path)?;
        let key = relative.to_string_lossy().to_string();
        let abs = fs::canonicalize(&path)?;
        path_map.insert(abs.to_string_lossy().to_string(), key.clone());
        source_paths.push((key.clone(), abs.to_string_lossy().to_string(), true));
        sources.push((key, content));
    }

    // Collect all .lua files - server files go to server_sources, lib files to sources
    let lua_pattern = format!("{}/**/*.lua", source_dir);
    for path in glob::glob(&lua_pattern)?.flatten() {
        let relative = path.strip_prefix(&source_dir)?;
        let content = fs::read_to_string(&path)?;
        let rel_str = relative.to_string_lossy().to_string();
        // Server files are stored as raw source (not compiled)
        let abs = fs::canonicalize(&path)?;
        path_map.insert(abs.to_string_lossy().to_string(), rel_str.clone());
        source_paths.push((rel_str.clone(), abs.to_string_lossy().to_string(), false));
        server_sources.push((rel_str, content));
    }

    // Also collect lib directory files
    let lib_dir = Path::new(&config.routing.lib_dir);
    if lib_dir.exists() {
        // Collect .lua files from lib - these go to server_sources (raw Lua, not templates)
        let lib_lua_pattern = format!("{}/**/*.lua", lib_dir.display());
        for path in glob::glob(&lib_lua_pattern)?.flatten() {
            let relative = path.strip_prefix(lib_dir)?;
            let content = fs::read_to_string(&path)?;
            // Store lib .lua files as server sources (executed as raw Lua)
            let key = format!("lib/{}", relative.to_string_lossy());
            let abs = fs::canonicalize(&path)?;
            path_map.insert(abs.to_string_lossy().to_string(), key.clone());
            source_paths.push((key.clone(), abs.to_string_lossy().to_string(), false));
            server_sources.push((key, content));
        }
        // Collect .luat files from lib - these are templates
        let lib_luat_pattern = format!("{}/**/*.luat", lib_dir.display());
        for path in (glob::glob(&lib_luat_pattern)?).flatten() {
            let relative = path.strip_prefix(lib_dir)?;
            let content = fs::read_to_string(&path)?;
            let key = format!("lib/{}", relative.to_string_lossy());
            let abs = fs::canonicalize(&path)?;
            path_map.insert(abs.to_string_lossy().to_string(), key.clone());
            source_paths.push((key.clone(), abs.to_string_lossy().to_string(), true));
            sources.push((key, content));
        }
    }

    if sources.is_empty() {
        println!("No templates found in {}", templates_dir);
        return Ok(());
    }

    let require_map = build_require_map(&source_paths, &path_map, &engine);

    println!(
        "{} {} source file(s)",
        style("Found").green(),
        sources.len()
    );

    // Bundle sources with progress bar
    let total_steps = sources.len() * 2; // bundle_sources reports 2 steps per source
    let pb = ProgressBar::new(total_steps as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} Compiling [{bar:30.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("━━╺"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let start_compile = Instant::now();
    let pb_clone = pb.clone();
    let (mut bundle, source_map) = engine.bundle_sources(sources, move |current, _total| {
        pb_clone.set_position(current as u64);
    })?;
    pb.finish_and_clear();
    let compile_time = start_compile.elapsed();

    // Generate routes metadata and prepend to bundle
    // Need mutable source_map to adjust offsets after bundle modifications
    let mut source_map = source_map;

    if let Some(ref router) = router {
        let routes_lua = generate_routes_lua(router, &routes_dir);
        // Count lines being prepended (routes_lua + 2 newlines)
        let lines_added = routes_lua.lines().count() + 2;
        bundle = format!("{}\n\n{}", routes_lua, bundle);
        // Adjust source map offsets since we prepended content
        source_map.adjust_offsets(lines_added as isize);
        println!(
            "{} {} route(s)",
            style("Discovered").green(),
            router.routes().len()
        );
    }

    // Add server sources as raw Lua strings (for execution at runtime)
    // Must be inserted BEFORE "return __modules" at the end of the bundle
    // Note: These are inserted at the END (before return), so they don't affect module line offsets
    if !server_sources.is_empty() || !require_map.is_empty() {
        if !require_map.is_empty() {
            let require_lua = generate_require_map_lua(&require_map);
            if let Some(pos) = bundle.rfind("return __modules") {
                bundle.insert_str(pos, &format!("{}\n\n", require_lua));
            } else {
                bundle.push_str(&format!("\n\n{}", require_lua));
            }
        }
        let server_lua = generate_server_sources_lua(&server_sources);
        // Find the "return __modules" at the end and insert before it
        if let Some(pos) = bundle.rfind("return __modules") {
            bundle.insert_str(pos, &format!("{}\n\n", server_lua));
        } else {
            // Fallback: just append (shouldn't happen with proper bundle)
            bundle.push_str(&format!("\n\n{}", server_lua));
        }
        println!(
            "{} {} server file(s)",
            style("Included").green(),
            server_sources.len()
        );
    }

    // Write output
    let output_path = Path::new(output);
    if source {
        let output_file = output_path.join("bundle.lua");
        fs::write(&output_file, &bundle)?;
        println!(
            "{} {}",
            style("Written source bundle to:").cyan(),
            output_file.display()
        );
    } else {
        // Compile bundle, translating errors to show original source locations
        let binary_bundle = match engine.compile_bundle(&bundle) {
            Ok(b) => b,
            Err(e) => {
                // Translate bundle line numbers to original source files
                let translated_error = source_map.translate_error(&e.to_string());
                return Err(anyhow::anyhow!("{}", translated_error));
            }
        };
        let output_file = output_path.join("bundle.bin");
        fs::write(&output_file, &binary_bundle)?;
        println!(
            "{} {}",
            style("Written binary bundle to:").cyan(),
            output_file.display()
        );
    }

    // Copy static assets to dist
    // Copy public directory
    let public_dir = Path::new(&config.dev.public_dir);
    if public_dir.exists() {
        let dest_public = output_path.join("public");
        copy_dir_recursive(public_dir, &dest_public)?;
        println!(
            "{} {} -> {}",
            style("Copied").green(),
            public_dir.display(),
            dest_public.display()
        );
    }

    // Copy static directory
    let static_dir = Path::new(&config.routing.static_dir);
    if static_dir.exists() {
        let dest_static = output_path.join("static");
        copy_dir_recursive(static_dir, &dest_static)?;
        println!(
            "{} {} -> {}",
            style("Copied").green(),
            static_dir.display(),
            dest_static.display()
        );
    }

    // Copy app.html if it exists
    let app_html_path = Path::new(&config.routing.app_html);
    if app_html_path.exists() {
        let dest_app_html = output_path.join("app.html");
        fs::copy(app_html_path, &dest_app_html)?;
        println!(
            "{} {} -> {}",
            style("Copied").green(),
            app_html_path.display(),
            dest_app_html.display()
        );
    }

    println!();
    println!(
        "{} {} {}",
        style("Build complete!").green().bold(),
        style("Templates compiled in").dim(),
        style(format!("{}ms", compile_time.as_millis())).cyan()
    );
    Ok(())
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Generate Lua table with server file sources (stored as strings for runtime execution)
fn generate_server_sources_lua(server_sources: &[(String, String)]) -> String {
    let mut lua = String::new();
    lua.push_str("-- Server file sources (executed at runtime with custom environment)\n");
    lua.push_str("__server_sources = {\n");

    for (name, source) in server_sources {
        // Escape the source for Lua long string literal
        // Use [=[ ... ]=] syntax to avoid issues with nested brackets
        lua.push_str(&format!("  [\"{}\"] = [=[\n", name.replace('\\', "/")));
        lua.push_str(source);
        lua.push_str("\n]=],\n");
    }

    lua.push_str("}\n\n");

    lua
}

/// Generate Lua table for pre-resolved require mappings
fn generate_require_map_lua(require_map: &HashMap<String, HashMap<String, String>>) -> String {
    let mut lua = String::new();
    lua.push_str("-- Pre-resolved require map (generated at build time)\n");
    lua.push_str("__require_map = {\n");

    for (module, deps) in require_map {
        lua.push_str(&format!("  [\"{}\"] = {{\n", module.replace('\\', "/")));
        for (req, resolved) in deps {
            lua.push_str(&format!(
                "    [\"{}\"] = \"{}\",\n",
                req.replace('\\', "/"),
                resolved.replace('\\', "/")
            ));
        }
        lua.push_str("  },\n");
    }

    lua.push_str("}\n");
    lua
}

fn extract_requires(source: &str) -> Vec<String> {
    let mut requires = Vec::new();
    let require_re = regex::Regex::new(r#"require\s*\(\s*["']([^"']+)["']\s*\)"#).unwrap();
    for cap in require_re.captures_iter(source) {
        requires.push(cap[1].to_string());
    }
    requires
}

fn build_require_map(
    source_paths: &[(String, String, bool)],
    path_map: &HashMap<String, String>,
    engine: &Engine<FileSystemResolver>,
) -> HashMap<String, HashMap<String, String>> {
    let mut require_map: HashMap<String, HashMap<String, String>> = HashMap::new();

    for (module_key, abs_path, is_template) in source_paths {
        let source = match fs::read_to_string(abs_path) {
            Ok(content) => content,
            Err(e) => {
                eprintln!(
                    "{} Failed to read {}: {}",
                    style("Warning:").yellow(),
                    module_key,
                    e
                );
                continue;
            }
        };
        let requires = if *is_template {
            let ast = match parse_template(&source) {
                Ok(ast) => ast,
                Err(e) => {
                    eprintln!(
                        "{} Failed to parse {}: {}",
                        style("Warning:").yellow(),
                        module_key,
                        e
                    );
                    continue;
                }
            };
            let mut script_content = String::new();
            if let Some(script) = ast.module_script {
                script_content.push_str(&script.content);
                script_content.push('\n');
            }
            if let Some(script) = ast.regular_script {
                script_content.push_str(&script.content);
            }
            if !script_content.is_empty() {
                warn_non_literal_requires(&script_content, module_key);
            }
            ast.imports
        } else {
            warn_non_literal_requires(&source, module_key);
            extract_requires(&source)
        };

        for req in requires {
            let resolved_path = engine
                .resolver()
                .get_resolved_path(abs_path, &req);

            let resolved_path = match resolved_path {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "{} Unresolved require '{}' in '{}': {}",
                        style("Warning:").yellow(),
                        req,
                        module_key,
                        e
                    );
                    continue;
                }
            };

            let resolved_abs = match fs::canonicalize(&resolved_path) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "{} Failed to canonicalize '{}' in '{}': {}",
                        style("Warning:").yellow(),
                        resolved_path,
                        module_key,
                        e
                    );
                    continue;
                }
            };
            let resolved_key = match path_map.get(&resolved_abs.to_string_lossy().to_string()) {
                Some(key) => key,
                None => {
                    eprintln!(
                        "{} Resolved '{}' in '{}' to '{}', but it is not part of the bundle",
                        style("Warning:").yellow(),
                        req,
                        module_key,
                        resolved_abs.display()
                    );
                    continue;
                }
            };

            require_map
                .entry(module_key.clone())
                .or_default()
                .insert(req, resolved_key.clone());
        }
    }

    require_map
}

fn warn_non_literal_requires(source: &str, module_key: &str) {
    let require_call_re = regex::Regex::new(r#"require\s*\("#).unwrap();
    let literal_require_re = regex::Regex::new(r#"require\s*\(\s*["']([^"']+)["']\s*\)"#).unwrap();
    let total_calls = require_call_re.find_iter(source).count();
    let literal_calls = literal_require_re.captures_iter(source).count();

    if total_calls != literal_calls {
        eprintln!(
            "{} Non-literal require() detected in '{}'. Build will skip pre-resolution for those calls.",
            style("Warning:").yellow(),
            module_key
        );
    }
}

/// Generate Lua code for routes metadata
fn generate_routes_lua(router: &LuatRouter, routes_dir: &Path) -> String {
    let mut lua = String::new();
    lua.push_str("-- Routes metadata (generated at build time)\n");
    lua.push_str("__routes = {\n");

    for route in router.routes() {
        lua.push_str("  {\n");
        lua.push_str(&format!("    pattern = \"{}\",\n", route.pattern));

        // Page template path (relative to routes_dir)
        if let Some(ref page) = route.page {
            if let Ok(rel) = page.strip_prefix(routes_dir) {
                lua.push_str(&format!(
                    "    page = \"{}\",\n",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }

        // Server file path
        if let Some(ref server) = route.server {
            if let Ok(rel) = server.strip_prefix(routes_dir) {
                lua.push_str(&format!(
                    "    server = \"{}\",\n",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }

        // API file path
        if let Some(ref api) = route.api {
            if let Ok(rel) = api.strip_prefix(routes_dir) {
                lua.push_str(&format!(
                    "    api = \"{}\",\n",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }

        // Error template path
        if let Some(ref error) = route.error {
            if let Ok(rel) = error.strip_prefix(routes_dir) {
                lua.push_str(&format!(
                    "    error = \"{}\",\n",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }

        // Layout paths
        if !route.layouts.is_empty() {
            lua.push_str("    layouts = {\n");
            for layout in &route.layouts {
                if let Ok(rel) = layout.strip_prefix(routes_dir) {
                    lua.push_str(&format!(
                        "      \"{}\",\n",
                        rel.to_string_lossy().replace('\\', "/")
                    ));
                }
            }
            lua.push_str("    },\n");
        }

        let mut layout_servers = Vec::new();
        for layout in &route.layouts {
            let server_path = layout.with_file_name("+layout.server.lua");
            if server_path.exists() {
                layout_servers.push(server_path);
            }
        }
        if !layout_servers.is_empty() {
            lua.push_str("    layout_servers = {\n");
            for server_path in &layout_servers {
                if let Ok(rel) = server_path.strip_prefix(routes_dir) {
                    lua.push_str(&format!(
                        "      \"{}\",\n",
                        rel.to_string_lossy().replace('\\', "/")
                    ));
                }
            }
            lua.push_str("    },\n");
        }

        if !route.action_templates.is_empty() {
            lua.push_str("    action_templates = {\n");
            let mut entries: Vec<(&String, &std::path::PathBuf)> =
                route.action_templates.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (name, path) in entries {
                if let Ok(rel) = path.strip_prefix(routes_dir) {
                    lua.push_str(&format!(
                        "      [\"{}\"] = \"{}\",\n",
                        name.replace('\\', "/"),
                        rel.to_string_lossy().replace('\\', "/")
                    ));
                }
            }
            lua.push_str("    },\n");
        }

        lua.push_str("  },\n");
    }

    lua.push_str("}\n");
    lua
}
