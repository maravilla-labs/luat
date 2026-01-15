// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! HTTP server for development with live reload and routing support.
//!
//! This is a thin adapter that converts HTTP requests to `LuatRequest`,
//! calls `engine.respond()`, and converts `LuatResponse` back to HTTP.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State, WebSocketUpgrade},
    http::{Method, StatusCode},
    response::{Html, IntoResponse, Response},
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
use crate::router::{Route, Router as LuatRouter};

const MAX_BODY_SIZE: usize = 1024 * 1024;

/// Shared application state for the development server.
pub struct AppState {
    /// Template engine with filesystem resolver.
    pub engine: RwLock<Engine<FileSystemResolver>>,
    /// Channel for sending reload notifications.
    pub reload_tx: Arc<broadcast::Sender<()>>,
    /// Application configuration.
    pub config: Config,
    /// URL router for matching requests.
    pub router: Option<LuatRouter>,
    /// Path to the routes directory.
    pub routes_dir: PathBuf,
    /// The app.html template content (HTML shell).
    pub app_html_template: Option<String>,
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
    let (templates_dir, router) = if config.routing.simplified {
        // Simplified mode: use templates_dir directly
        (working_dir.join(&config.dev.templates_dir), None)
    } else {
        // SvelteKit-style routing: use routes_dir
        let routes_dir = working_dir.join(&config.routing.routes_dir);
        if routes_dir.exists() {
            let router = LuatRouter::discover(&routes_dir)?;
            println!(
                "Discovered {} route(s) in {}",
                router.routes().len(),
                routes_dir.display()
            );
            for route in router.routes() {
                println!("  {} -> {}", route.pattern, route.fs_path.display());
            }
            (routes_dir.clone(), Some(router))
        } else {
            // Fall back to templates_dir if routes_dir doesn't exist
            println!(
                "Routes directory {} not found, falling back to simplified mode",
                routes_dir.display()
            );
            (working_dir.join(&config.dev.templates_dir), None)
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
        router,
        routes_dir: templates_dir,
        app_html_template,
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
async fn fallback_handler(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
) -> Response {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri.clone();
    let headers = parts.headers.clone();
    let path = uri.path().to_string();
    let query_string = uri.query().unwrap_or_default().to_string();

    // Parse query parameters
    let query: HashMap<String, String> = query_string
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.to_string();
            let value = parts.next().unwrap_or("").to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect();

    // Check if we have a SvelteKit-style router
    if let Some(ref router) = state.router {
        // Try to match the URL
        if let Some(route_match) = router.match_url(&path) {
            let body_bytes = if method != Method::GET && method != Method::HEAD {
                match axum::body::to_bytes(body, MAX_BODY_SIZE).await {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            None
                        } else {
                            Some(bytes.to_vec())
                        }
                    }
                    Err(_) => {
                        return (StatusCode::BAD_REQUEST, "Body too large").into_response();
                    }
                }
            } else {
                None
            };

            let headers_map: HashMap<String, String> = headers
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
                .collect();

            // Create LuatRequest
            let luat_request = to_luat_request(&path, &method, query, body_bytes, headers_map);

            // Handle route using unified engine.respond_async()
            return handle_route(&state, route_match.route, route_match.params.clone(), luat_request).await;
        }
    }

    // Fall back to simplified routing
    handle_simplified_route(&state, &path).await
}

/// Convert CLI Route to Engine Route for use with engine.respond()
fn cli_route_to_engine_route(
    cli_route: &Route,
    params: &[(String, String)],
    routes_dir: &PathBuf,
) -> luat::router::Route {
    let to_relative_string = |path: &PathBuf| -> String {
        path.strip_prefix(routes_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    };

    let mut route = luat::router::Route::new(
        cli_route.pattern.clone(),
        to_relative_string(&cli_route.fs_path),
    );

    // Set params
    route.params = params.iter().cloned().collect();

    // Convert page paths
    route.page = cli_route.page.as_ref().map(&to_relative_string);
    route.layout = cli_route.layout.as_ref().map(&to_relative_string);
    route.page_server = cli_route.server.as_ref().map(&to_relative_string);
    route.api = cli_route.api.as_ref().map(&to_relative_string);
    route.error = cli_route.error.as_ref().map(&to_relative_string);

    // Convert layout chains
    route.layouts = cli_route.layouts.iter().map(&to_relative_string).collect();

    // Convert action templates
    route.action_templates = cli_route
        .action_templates
        .iter()
        .map(|(k, v)| (k.clone(), to_relative_string(v)))
        .collect();

    // Note: layout_servers not directly available in CLI Route, but we can derive from layouts
    route.layout_servers = cli_route.layouts.iter()
        .filter_map(|layout_path| {
            let server_path = layout_path.with_file_name("+layout.server.lua");
            if server_path.exists() {
                Some(to_relative_string(&server_path))
            } else {
                None
            }
        })
        .collect();

    route
}

/// Convert axum Request parts to LuatRequest
fn to_luat_request(
    path: &str,
    method: &Method,
    query: HashMap<String, String>,
    body: Option<Vec<u8>>,
    headers: HashMap<String, String>,
) -> LuatRequest {
    let mut request = LuatRequest::new(path, method.as_str())
        .with_query(query)
        .with_headers(headers);

    if let Some(body) = body {
        request = request.with_body(body);
    }

    request
}

/// Convert LuatResponse to axum Response
fn luat_response_to_axum(
    response: LuatResponse,
    state: &AppState,
    request_headers: &HashMap<String, String>,
) -> Response {
    match response {
        LuatResponse::Html { status, mut headers, body } => {
            let is_fragment = headers.remove("x-luat-fragment").is_some()
                || headers.remove("X-Luat-Fragment").is_some();

            // Check for HTMX boosted navigation (hx-boost="true")
            let is_htmx_boosted = request_headers
                .get("hx-boosted")
                .map(|v| v == "true")
                .unwrap_or(false);

            // Extract title from response headers (set by setContext("view_title", ...))
            let title = headers
                .remove("x-luat-title")
                .unwrap_or_else(|| "Luat App".to_string());

            // Collect head assets
            let head_assets = collect_head_assets(&state.config);

            // Wrap with app.html shell
            let app_html = state
                .app_html_template
                .as_deref()
                .unwrap_or(DEFAULT_APP_HTML);

            // Decide how to render based on request type
            let (full_html, include_livereload, extra_headers) = if is_fragment {
                // Fragment: return body only, include title header if set
                let mut extra = Vec::new();
                if title != "Luat App" {
                    extra.push(("x-luat-title".to_string(), title.clone()));
                }
                (body, false, extra)
            } else if is_htmx_boosted {
                // HTMX Boosted: return body only, add HX-Title header for document.title update
                (body, false, vec![("HX-Title".to_string(), title)])
            } else {
                // Full page: wrap with app.html shell, title goes in <title> tag
                (wrap_with_app_html(app_html, &body, &title, &head_assets), true, vec![])
            };

            let html_with_livereload = if include_livereload {
                inject_livereload_script(&full_html)
            } else {
                full_html
            };

            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let mut builder = axum::http::Response::builder().status(status_code);

            // Add remaining response headers
            for (key, value) in headers {
                builder = builder.header(key, value);
            }
            // Add extra headers (HX-Title, x-luat-title for fragments)
            for (key, value) in extra_headers {
                builder = builder.header(key, value);
            }
            builder = builder.header("content-type", "text/html; charset=utf-8");

            builder
                .body(Body::from(html_with_livereload))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
                })
        }
        LuatResponse::Json { status, headers, body } => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let mut builder = axum::http::Response::builder().status(status_code);

            for (key, value) in headers {
                builder = builder.header(key, value);
            }
            builder = builder.header("content-type", "application/json");

            builder
                .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
                })
        }
        LuatResponse::Redirect { status, location } => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::FOUND);
            Response::builder()
                .status(status_code)
                .header("location", location)
                .body(Body::empty())
                .unwrap()
        }
        LuatResponse::Error { status: _, message } => {
            error_page(&message)
        }
    }
}

/// Handle any route (API or page) using engine.respond()
async fn handle_route(
    state: &AppState,
    route: &Route,
    params: Vec<(String, String)>,
    request: LuatRequest,
) -> Response {
    // Convert CLI route to engine route
    let engine_route = cli_route_to_engine_route(route, &params, &state.routes_dir);

    // Keep a reference to request headers for response handling
    let request_headers = request.headers.clone();

    // Use engine.respond() for unified handling - it handles both API and page routes
    let engine = state.engine.read().await;

    match engine.respond_async(&engine_route, &request).await {
        Ok(response) => luat_response_to_axum(response, state, &request_headers),
        Err(e) => error_page(&format!("Error: {}", e)),
    }
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
            return error_page(&format!("Context error: {}", e));
        }
    };

    match engine.compile_entry(&template_path) {
        Ok(module) => match engine.render(&module, &context) {
            Ok(body_html) => {
                // Collect head assets
                let head_assets = collect_head_assets(&state.config);

                // Wrap with app.html shell
                let app_html = state
                    .app_html_template
                    .as_deref()
                    .unwrap_or(DEFAULT_APP_HTML);

                let full_html = wrap_with_app_html(app_html, &body_html, "Luat App", &head_assets);
                let html_with_livereload = inject_livereload_script(&full_html);
                Html(html_with_livereload).into_response()
            }
            Err(e) => error_page(&format!("Render error: {}", e)),
        },
        Err(e) => error_page(&format!("Compile error: {}", e)),
    }
}

fn error_page(message: &str) -> Response {
    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Error - Luat</title>
    <style>
        body {{ font-family: system-ui, sans-serif; padding: 2rem; background: #1a1a2e; color: #eee; }}
        .error {{ background: #16213e; border-left: 4px solid #e94560; padding: 1rem; border-radius: 4px; }}
        pre {{ background: #0f0f1a; padding: 1rem; overflow-x: auto; border-radius: 4px; }}
    </style>
</head>
<body>
    <h1>Error</h1>
    <div class="error">
        <pre>{}</pre>
    </div>
</body>
</html>"#,
        html_escape(message)
    ))
    .into_response()
}

/// Creates a redirect response (reserved for future use).
#[allow(dead_code)]
fn redirect_response(url: &str) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header("location", url)
        .body(Body::empty())
        .unwrap()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
fn wrap_with_app_html(
    app_html: &str,
    body: &str,
    title: &str,
    head_assets: &str,
) -> String {
    app_html
        .replace("%luat.title%", title)
        .replace("%luat.head%", head_assets)
        .replace("%luat.body%", body)
}

/// Default app.html template when no app.html exists
const DEFAULT_APP_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>%luat.title%</title>
    %luat.head%
</head>
<body>
    %luat.body%
</body>
</html>
"#;

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
