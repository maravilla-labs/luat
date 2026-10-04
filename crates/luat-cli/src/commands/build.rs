// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Build command for compiling LUAT templates into a production bundle.

use crate::config::Config;
use crate::toolchain::{build::BuildOrchestrator, prepare_build_tools};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use luat::bundle::{build, BuildOptions};
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Runs the build command to compile templates into a production bundle
/// (`<output>/bundle.lua`) and copy static assets next to it.
///
/// `source` is accepted for compatibility: bundles are always Lua source
/// now, since bytecode only loads into the exact Lua build that made it.
pub async fn run(source: bool, output: &str) -> anyhow::Result<()> {
    let _ = source;
    let config = Config::load()?;
    let working_dir = std::env::current_dir()?;

    // Run frontend build if any tools are enabled
    let enabled_tools = config.frontend.get_enabled_tools();
    if !enabled_tools.is_empty() {
        println!("Building frontend assets...");
        let tool_paths = prepare_build_tools(&config.frontend, false).await?;
        let mut orchestrator =
            BuildOrchestrator::new(config.frontend.clone(), working_dir.clone(), true, false);
        for (tool, path) in &tool_paths {
            orchestrator.register_tool(*tool, path.clone());
        }
        orchestrator.build_all().await?;
        println!();
    }

    // Use routes_dir for SvelteKit-style routing, templates_dir for simplified mode
    let source_dir = if config.routing.simplified {
        config.dev.templates_dir.clone()
    } else {
        config.routing.routes_dir.clone()
    };
    println!("{} {}", style("Building templates from:").cyan(), source_dir);

    let options = BuildOptions {
        routes_dir: working_dir.join(&source_dir),
        lib_dir: Some(working_dir.join(&config.routing.lib_dir)),
        app_html: Some(working_dir.join(&config.routing.app_html)),
        module_dirs: Vec::new(),
    };

    let pb = ProgressBar::new(0);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} Compiling [{bar:30.cyan/blue}] {pos}/{len}")
            .unwrap()
            .progress_chars("━━╺"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let start_compile = Instant::now();
    let pb_progress = pb.clone();
    let built = build(&options, move |done, total| {
        pb_progress.set_length(total as u64);
        pb_progress.set_position(done as u64);
    });
    pb.finish_and_clear();
    let built = built?;
    let compile_time = start_compile.elapsed();

    for warning in &built.warnings {
        eprintln!("{} {}", style("Warning:").yellow(), warning);
    }
    println!(
        "{} {} template(s), {} Lua source(s), {} route(s)",
        style("Compiled").green(),
        built.template_count,
        built.server_source_count,
        built.route_count
    );

    let output_path = Path::new(output);
    fs::create_dir_all(output_path)?;
    let bundle_file = output_path.join("bundle.lua");
    fs::write(&bundle_file, built.bundle.source())?;
    println!("{} {}", style("Written bundle to:").cyan(), bundle_file.display());

    for (dir, name) in [(config.dev.public_dir.as_str(), "public"), (config.routing.static_dir.as_str(), "static")] {
        let src = Path::new(dir);
        if src.exists() {
            let dest = output_path.join(name);
            copy_dir_recursive(src, &dest)?;
            println!("{} {} -> {}", style("Copied").green(), src.display(), dest.display());
        }
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

