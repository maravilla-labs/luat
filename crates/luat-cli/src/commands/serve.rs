// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Production server command.
//!
//! Serves the application from the pre-built bundle (`dist/bundle.lua`).
//! No live reload, optimized for production.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    response::Response,
    Router,
};
use console::style;
use luat::{kv::register_kv_module, App, Bundle};
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::config::Config;
use crate::kv::KVManager;
use crate::server::{request::to_luat_request, response};

/// Shared application state for the production server.
pub struct AppState {
    /// The app loaded from the bundle: engine, router and shell.
    pub app: App,
    /// Head markup (stylesheets, scripts) added to every page.
    pub shell_options: luat::ShellOptions,
}

/// Runs the production server using the pre-built bundle.
pub async fn run(host: &str, port: u16) -> anyhow::Result<()> {
    let config = Config::load()?;
    let working_dir = std::env::current_dir()?;
    let dist_dir = working_dir.join(&config.build.output_dir);

    let bundle_path = dist_dir.join("bundle.lua");
    if !bundle_path.exists() {
        println!(
            "{}",
            style(format!("Error: {} not found!", bundle_path.display())).red().bold()
        );
        println!();
        println!("Run {} first to build your application.", style("luat build").cyan());
        println!();
        return Ok(());
    }

    println!("{}", style("Starting production server...").cyan().bold());
    println!("{} {}", style("Loading bundle from:").dim(), bundle_path.display());

    let bundle = Bundle::from_source(std::fs::read_to_string(&bundle_path)?)?;
    let app = bundle.instantiate()?;

    // Register KV module with SQLite backend
    let kv_dir = working_dir.join(".luat").join("kv");
    let kv_manager = Arc::new(KVManager::new(&kv_dir)?);
    register_kv_module(app.engine.lua(), kv_manager.clone().factory())?;

    // Register HTTP module for making HTTP requests from Lua
    crate::extensions::register_http_module(app.engine.lua())?;

    println!(
        "{} {} route(s) from bundle",
        style("Loaded").green(),
        app.router.routes().len()
    );

    let head = if app.head.is_empty() {
        collect_production_head_assets(&config)
    } else {
        app.head.clone()
    };
    let state = Arc::new(AppState {
        app,
        shell_options: luat::ShellOptions {
            head,
            ..Default::default()
        },
    });

    let router = Router::new()
        .nest_service("/public", ServeDir::new(dist_dir.join("public")))
        .nest_service("/static", ServeDir::new(dist_dir.join("static")))
        .merge(immutable_assets(&dist_dir))
        .fallback(fallback_handler)
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    println!();
    println!(
        "{} {}",
        style("Production server running at").green().bold(),
        style(format!("http://{}", addr)).cyan().underlined()
    );
    println!("{}", style("Press Ctrl+C to stop").dim());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

async fn fallback_handler(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    let request = match to_luat_request(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let app = &state.app;
    let result = match app.router.match_url(&request.path) {
        Some(route) => app.engine.respond_async(&route, &request).await,
        None => Ok(app
            .engine
            .respond_not_found_async(app.router.root_error(), &request)
            .await),
    };
    match result {
        Ok(luat_response) => response::to_axum(luat::finalize(
            luat_response,
            &request,
            &app.shell,
            &state.shell_options,
        )),
        // Only execution-limit errors reach here.
        Err(e) => {
            tracing::error!(error = %e, "request stopped");
            response::error(500, "Internal Server Error")
        }
    }
}

/// Hashed client assets, cached for good: a changed file is a new URL.
fn immutable_assets<S: Clone + Send + Sync + 'static>(dist_dir: &std::path::Path) -> Router<S> {
    Router::new()
        .nest_service(
            &format!("/{}", luat::assets::IMMUTABLE_DIR),
            ServeDir::new(dist_dir.join(luat::assets::IMMUTABLE_DIR)),
        )
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
        ))
}

fn collect_production_head_assets(config: &Config) -> String {
    let mut head = String::new();

    use crate::toolchain::Tool;
    let enabled_tools = config.frontend.get_enabled_tools();

    if enabled_tools.contains(&Tool::Sass) {
        let path = config.frontend.sass_output.trim_start_matches("public/");
        head.push_str(&format!(
            "    <link rel=\"stylesheet\" href=\"/public/{}\">\n",
            path
        ));
    }

    if enabled_tools.contains(&Tool::Tailwind) {
        let path = config.frontend.tailwind_output.trim_start_matches("public/");
        head.push_str(&format!(
            "    <link rel=\"stylesheet\" href=\"/public/{}\">\n",
            path
        ));
    }

    if enabled_tools.contains(&Tool::TypeScript) {
        let path = config.frontend.typescript_output.trim_start_matches("public/");
        head.push_str(&format!(
            "    <script src=\"/public/{}\" defer></script>\n",
            path
        ));
    }

    head
}

