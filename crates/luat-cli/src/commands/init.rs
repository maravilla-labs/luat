// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Project initialization command for creating new LUAT projects.

use include_dir::{include_dir, Dir, DirEntry};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

static DEFAULT_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates/default");
static MINIMAL_TEMPLATE: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates/minimal");

/// Initializes a new LUAT project from a template.
pub async fn run(name: Option<String>, template: Option<String>) -> anyhow::Result<()> {
    // If no template specified, show selection menu
    let template_name = match template {
        Some(t) => t,
        None => select_template()?,
    };

    // Handle "." or no argument to init in current directory
    let is_current_dir = matches!(name.as_deref(), Some(".") | None);
    let (project_dir, project_name) = resolve_project_path(name)?;

    if project_dir.exists() {
        tracing::info!(
            "Initializing luat project in existing directory: {}",
            project_name
        );
    } else {
        fs::create_dir_all(&project_dir)?;
        tracing::info!("Created project directory: {}", project_name);
    }

    let template_dir = match template_name.as_str() {
        "minimal" => &MINIMAL_TEMPLATE,
        _ => &DEFAULT_TEMPLATE,
    };

    extract_template(template_dir, &project_dir, &project_name)?;

    // Create empty directories that aren't in the template
    fs::create_dir_all(project_dir.join("public/css"))?;
    fs::create_dir_all(project_dir.join("public/js"))?;

    print_success(&project_name, &template_name, is_current_dir);

    Ok(())
}

fn select_template() -> anyhow::Result<String> {
    println!();
    println!("Select a template:");
    println!();
    println!("  1. default (recommended)");
    println!("     Full-featured starter with HTMX, Idiomorph, TypeScript,");
    println!("     Tailwind CSS, and a todo example with form actions & fragments");
    println!();
    println!("  2. minimal");
    println!("     Simple starter with TypeScript and Tailwind CSS");
    println!();
    print!("Enter choice [1]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    match input {
        "" | "1" | "default" => Ok("default".to_string()),
        "2" | "minimal" => Ok("minimal".to_string()),
        _ => {
            println!("Invalid choice, using default template");
            Ok("default".to_string())
        }
    }
}

fn resolve_project_path(name: Option<String>) -> anyhow::Result<(std::path::PathBuf, String)> {
    match name.as_deref() {
        Some(".") | None => {
            let current_dir = std::env::current_dir()?;
            let dir_name = current_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "my-luat-app".to_string());
            Ok((current_dir, dir_name))
        }
        Some(name) => {
            let project_path = Path::new(name).to_path_buf();
            let dir_name = project_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| name.to_string());
            Ok((project_path, dir_name))
        }
    }
}

fn extract_template(template: &Dir, target: &Path, project_name: &str) -> anyhow::Result<()> {
    for entry in template.entries() {
        extract_entry(entry, target, project_name)?;
    }
    Ok(())
}

fn extract_entry(entry: &DirEntry, target: &Path, project_name: &str) -> anyhow::Result<()> {
    match entry {
        DirEntry::Dir(dir) => {
            let dir_path = target.join(dir.path());
            fs::create_dir_all(&dir_path)?;
            for child in dir.entries() {
                extract_entry(child, target, project_name)?;
            }
        }
        DirEntry::File(file) => {
            let file_path = file.path();
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid file name: {:?}", file_path))?;

            // Handle special file names
            let target_name: &str = match file_name {
                "gitignore" => ".gitignore",
                name if name.ends_with(".tmpl") => &name[..name.len() - 5],
                name => name,
            };

            let target_path = if let Some(parent) = file_path.parent() {
                target.join(parent).join(target_name)
            } else {
                target.join(target_name)
            };

            // Ensure parent directory exists
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let content = file
                .contents_utf8()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 file: {:?}", file_path))?;

            // Substitute project name in .tmpl files
            let content = if file_name.ends_with(".tmpl") {
                content.replace("{{project_name}}", project_name)
            } else {
                content.to_string()
            };

            fs::write(&target_path, content)?;
        }
    }
    Ok(())
}

fn print_success(project_name: &str, template_name: &str, is_current_dir: bool) {
    println!("Created luat project: {} ({} template)", project_name, template_name);
    println!();
    println!("Next steps:");
    if !is_current_dir {
        println!("  cd {}", project_name);
    }
    println!("  npm install");
    println!("  luat dev");

    if template_name == "default" {
        println!();
        println!("Visit http://localhost:3000/todos to see the HTMX example.");
    }
}
