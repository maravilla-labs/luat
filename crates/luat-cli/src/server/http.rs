// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP server for development with live reload and routing support.
//!
//! This is a thin adapter that converts HTTP requests to `LuatRequest`,
//! calls `engine.respond()`, and converts `LuatResponse` back to HTTP.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State, WebSocketUpgrade},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use luat::{Engine, FileSystemResolver, LuatRequest, LuatResponse, NoOpCache};
use serde_json::json;
use tokio::sync::{broadcast, RwLock};
use tower_http::services::ServeDir;

use super::livereload::handle_websocket;
use crate::config::Config;
use crate::kv::KVManager;
use crate::server::request::to_luat_request;


/// Shared application state for the development server.
pub struct AppState {
    /// Template engine with filesystem resolver.
    pub engine: RwLock<Engine<FileSystemResolver>>,
    /// Channel for sending reload notifications.
    pub reload_tx: Arc<broadcast::Sender<()>>,
    /// Application configuration.
    pub config: Config,
    /// Whether SvelteKit-style routing is used (otherwise simplified mode).
    /// Routes are rediscovered on every request, so new route files work
    /// without a restart.
    pub routed: bool,
    /// Path to the routes directory.
    pub routes_dir: PathBuf,
    /// The HTML shell pages are rendered into (`src/app.html`).
    pub shell: luat::AppShell,
    /// KV store manager for server-side data persistence.
    pub kv_manager: Arc<KVManager>,
}

/// Creates and starts the development HTTP server.
pub async fn create_server(
    addr: &str,
    config: &Config,
    reload_tx: Arc<broadcast::Sender<()>>,
) -> anyhow::Result<()> {
    let working_dir = std::env::current_dir()?;

    // Determine which directory to use for templates
    let (templates_dir, routed) = if config.routing.simplified {
        // Simplified mode: use templates_dir directly
        (working_dir.join(&config.dev.templates_dir), false)
    } else {
        // SvelteKit-style routing: use routes_dir
        let routes_dir = working_dir.join(&config.routing.routes_dir);
        if routes_dir.exists() {
            let router = luat::Router::discover(&routes_dir)?;
            println!(
                "Discovered {} route(s) in {}",
                router.routes().len(),
                routes_dir.display()
            );
            for route in router.routes() {
                println!("  {} -> {}", route.pattern, route.fs_path);
            }
            (routes_dir.clone(), true)
        } else {
            // Fall back to templates_dir if routes_dir doesn't exist
            println!(
                "Routes directory {} not found, falling back to simplified mode",
                routes_dir.display()
            );
            (working_dir.join(&config.dev.templates_dir), false)
        }
    };

    // Create resolver with lib_dir for $lib alias support
    let lib_dir = working_dir.join(&config.routing.lib_dir);
    let resolver = FileSystemResolver::new(&templates_dir).with_lib_dir(&lib_dir);
    // Dev mode: no caching for fresh reloads on file changes
    let cache = NoOpCache::new();
    let mut engine = Engine::new(resolver, Box::new(cache))?;
    // Set root path for readable error messages (show relative paths)
    engine.set_root_path(&working_dir);

    // Dev mode: setup non-caching require() so modules always load fresh
    engine.setup_dev_mode()?;
    // Dev server: show error details to the developer
    engine.set_development_mode(true)?;

    // Create KV manager for server-side persistence
    let data_dir = working_dir.join(&config.routing.data_dir);
    let kv_manager = Arc::new(
        KVManager::new(&data_dir).expect("Failed to create KV manager")
    );
    println!("KV store initialized at {}", data_dir.display());

    // Register KV module on the engine's Lua instance
    // This ensures json AND kv modules are available in all Lua execution
    let factory = kv_manager.clone().factory();
    if let Err(e) = luat::kv::register_kv_module(engine.lua(), factory) {
        eprintln!("Warning: Failed to register KV module: {}", e);
    }

    // Register HTTP module for making HTTP requests from Lua
    if let Err(e) = crate::extensions::register_http_module(engine.lua()) {
        eprintln!("Warning: Failed to register HTTP module: {}", e);
    }

    // Load app.html if it exists
    let app_html_path = working_dir.join(&config.routing.app_html);
    let app_html_template = if app_html_path.exists() {
        match std::fs::read_to_string(&app_html_path) {
            Ok(content) => {
                println!("Loaded HTML shell from {}", app_html_path.display());
                Some(content)
            }
            Err(e) => {
                eprintln!("Warning: Could not load app.html: {}", e);
                None
            }
        }
    } else {
        println!("No app.html found, using inline HTML");
        None
    };

    let state = Arc::new(AppState {
        engine: RwLock::new(engine),
        reload_tx,
        config: config.clone(),
        routed,
        routes_dir: templates_dir,
        shell: app_html_template.map(luat::AppShell::new).unwrap_or_default(),
        kv_manager,
    });

    // Build the app with appropriate routes
    let app = Router::new()
        .route("/__livereload", get(livereload_handler))
        .nest_service("/public", ServeDir::new(&config.dev.public_dir))
        .nest_service("/static", ServeDir::new(&config.routing.static_dir))
        .fallback(fallback_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn livereload_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.reload_tx.subscribe();
    ws.on_upgrade(move |socket| handle_websocket(socket, rx))
}

/// Main fallback handler that routes requests
async fn fallback_handler(State(state): State<Arc<AppState>>, request: Request<Body>) -> Response {
    if !state.routed {
        return handle_simplified_route(&state, request.uri().path()).await;
    }
    let request = match to_luat_request(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    let router = match luat::Router::discover(&state.routes_dir) {
        Ok(router) => router,
        Err(e) => return crate::server::response::error(500, format!("Route discovery failed: {e}")),
    };

    let engine = state.engine.read().await;
    let result = match router.match_url(&request.path) {
        Some(route) => engine.respond_async(&route, &request).await,
        None => Ok(engine
            .respond_not_found_async(router.root_error(), &request)
            .await),
    };
    match result {
        Ok(response) => luat_response_to_axum(response, &state, &request),
        // Only execution-limit errors reach here; everything else is
        // already an error response.
        Err(e) => crate::server::response::error(500, format!("Error: {}", e)),
    }
}

fn luat_response_to_axum(response: LuatResponse, state: &AppState, request: &LuatRequest) -> Response {
    let options = luat::ShellOptions {
        head: collect_head_assets(&state.config),
        ..Default::default()
    };
    let mut http = luat::finalize(response, request, &state.shell, &options);
    if http.document {
        http.body = inject_livereload_script(&String::from_utf8_lossy(&http.body)).into_bytes();
    }
    crate::server::response::to_axum(http)
}

/// Handle simplified routing (direct file-to-URL mapping)
async fn handle_simplified_route(state: &AppState, path: &str) -> Response {
    let template_path = if path.is_empty() || path == "/" {
        "index.luat".to_string()
    } else {
        let clean_path = path.trim_start_matches('/');
        if clean_path.contains('.') {
            clean_path.to_string()
        } else {
            format!("{}.luat", clean_path)
        }
    };

    let engine = state.engine.read().await;

    // Create empty context for now (simplified mode doesn't have load functions)
    let context = match engine.to_value(json!({
        "title": "Luat App",
        "items": ["Item 1", "Item 2", "Item 3"]
    })) {
        Ok(ctx) => ctx,
        Err(e) => {
            return crate::server::response::error(500, format!("Context error: {}", e));
        }
    };

    let request = LuatRequest::new(path, "GET");
    let response = match engine.compile_entry(&template_path) {
        Ok(module) => match engine.render_async(&module, &context).await {
            Ok(body_html) => LuatResponse::html(200, body_html),
            Err(e) => LuatResponse::error(500, format!("Render error: {}", e)),
        },
        Err(e) => LuatResponse::error(404, format!("Compile error: {}", e)),
    };
    luat_response_to_axum(response, state, &request)
}




/// Collect head assets (CSS and JS files from public directory)
fn collect_head_assets(config: &Config) -> String {
    let mut head = String::new();
    let public_dir = std::path::Path::new(&config.dev.public_dir);

    // Collect CSS files
    if let Ok(entries) = std::fs::read_dir(public_dir.join("css")) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".css") {
                    head.push_str(&format!(
                        "    <link rel=\"stylesheet\" href=\"/public/css/{}\">\n",
                        name
                    ));
                }
            }
        }
    }

    // Collect JS files
    if let Ok(entries) = std::fs::read_dir(public_dir.join("js")) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".js") {
                    head.push_str(&format!(
                        "    <script src=\"/public/js/{}\" defer></script>\n",
                        name
                    ));
                }
            }
        }
    }

    head
}

/// Wrap rendered body content with app.html shell
fn inject_livereload_script(html: &str) -> String {
    let script = r#"
<script>
(function() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(protocol + '//' + window.location.host + '/__livereload');
    ws.onmessage = function(event) {
        if (event.data === 'reload') {
            console.log('[luat] Reloading...');
            window.location.reload();
        }
    };
    ws.onclose = function() {
        console.log('[luat] Connection lost, attempting to reconnect...');
        setTimeout(function() {
            window.location.reload();
        }, 1000);
    };
    ws.onerror = function(error) {
        console.error('[luat] WebSocket error:', error);
    };
})();
</script>
"#;

    if let Some(pos) = html.to_lowercase().rfind("</body>") {
        let mut result = html.to_string();
        result.insert_str(pos, script);
        result
    } else if let Some(pos) = html.to_lowercase().rfind("</html>") {
        let mut result = html.to_string();
        result.insert_str(pos, script);
        result
    } else {
        format!("{}{}", html, script)
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Config {
            project: crate::config::ProjectConfig {
                name: self.project.name.clone(),
                version: self.project.version.clone(),
            },
            dev: crate::config::DevConfig {
                port: self.dev.port,
                host: self.dev.host.clone(),
                templates_dir: self.dev.templates_dir.clone(),
                public_dir: self.dev.public_dir.clone(),
            },
            build: crate::config::BuildConfig {
                output_dir: self.build.output_dir.clone(),
                bundle_format: self.build.bundle_format.clone(),
            },
            frontend: self.frontend.clone(),
            routing: self.routing.clone(),
        }
    }
}
