// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Production server command.
//!
//! Serves the application from the pre-built bundle (dist/bundle.bin).
//! No live reload, optimized for production.

use std::sync::Arc;
use std::collections::HashMap;
use std::path::Path;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{Html, IntoResponse, Response},
    Router,
};
use console::style;
use luat::{Engine, LuatRequest, LuatResponse, MemoryCache, MemoryResourceResolver, kv::register_kv_module};
use mlua::{Lua, Table};
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::kv::KVManager;

/// Route information parsed from __routes in the bundle.
#[derive(Debug, Clone)]
pub struct BundleRoute {
    /// URL pattern for matching (e.g., "/blog/:slug").
    pub pattern: String,
    /// Page template module path.
    pub page: Option<String>,
    /// Server-side load function module path.
    pub server: Option<String>,
    /// API handler module path.
    pub api: Option<String>,
    /// Error page template module path.
    pub error: Option<String>,
    /// Layout template module paths.
    pub layouts: Vec<String>,
    /// Layout server module paths.
    pub layout_servers: Vec<String>,
    /// Action template mappings (action name -> template path).
    pub action_templates: HashMap<String, String>,
}

impl BundleRoute {
    /// Returns true if this is an API-only route.
    pub fn is_api_route(&self) -> bool {
        self.api.is_some() && self.page.is_none()
    }

    /// Returns true if this route has a page template.
    pub fn is_page_route(&self) -> bool {
        self.page.is_some()
    }
}

/// URL router for matching requests to bundle routes.
pub struct BundleRouter {
    routes: Vec<BundleRoute>,
    matcher: matchit::Router<usize>,
}

impl BundleRouter {
    /// Creates a new router from a list of routes.
    pub fn new(routes: Vec<BundleRoute>) -> anyhow::Result<Self> {
        let mut matcher = matchit::Router::new();
        for (i, route) in routes.iter().enumerate() {
            matcher.insert(&route.pattern, i)?;
        }
        Ok(Self { routes, matcher })
    }

    /// Matches a URL path and returns the route with extracted parameters.
    pub fn match_url(&self, path: &str) -> Option<(&BundleRoute, Vec<(String, String)>)> {
        match self.matcher.at(path) {
            Ok(matched) => {
                let route = &self.routes[*matched.value];
                let params: Vec<(String, String)> = matched
                    .params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                Some((route, params))
            }
            Err(_) => None,
        }
    }
}

/// Shared application state for the production server.
pub struct AppState {
    /// Template engine with memory resolver.
    pub engine: RwLock<Engine<MemoryResourceResolver>>,
    /// Application configuration.
    pub config: Config,
    /// URL router for matching requests.
    pub router: Option<BundleRouter>,
    /// HTML template for wrapping rendered pages.
    pub app_html_template: Option<String>,
}

const MAX_BODY_SIZE: usize = 1024 * 1024;

/// Runs the production server using the pre-built bundle.
pub async fn run(host: &str, port: u16) -> anyhow::Result<()> {
    let config = Config::load()?;
    let working_dir = std::env::current_dir()?;
    let dist_dir = working_dir.join("dist");

    // Check if bundle exists
    let bundle_path = dist_dir.join("bundle.bin");
    if !bundle_path.exists() {
        println!(
            "{}",
            style("Error: dist/bundle.bin not found!").red().bold()
        );
        println!();
        println!("Run {} first to build your application.", style("luat build").cyan());
        println!();
        return Ok(());
    }

    println!("{}", style("Starting production server...").cyan().bold());
    println!(
        "{} {}",
        style("Loading bundle from:").dim(),
        bundle_path.display()
    );

    // Load the bundle
    let bundle_bytes = std::fs::read(&bundle_path)?;

    // Create engine with memory resolver (templates are in bundle, not filesystem)
    let resolver = MemoryResourceResolver::new();
    let cache = MemoryCache::new(1000);
    let engine = Engine::new(resolver, Box::new(cache))?;

    // Preload bundle into engine
    engine.preload_bundle_code_from_binary(&bundle_bytes)?;

    // Register KV module with SQLite backend on engine Lua
    let kv_dir = working_dir.join(".luat").join("kv");
    let kv_manager = Arc::new(KVManager::new(&kv_dir)?);
    register_kv_module(engine.lua(), kv_manager.clone().factory())?;

    // Register HTTP module for making HTTP requests from Lua
    crate::extensions::register_http_module(engine.lua())?;

    // Extract routes from __routes
    let routes = extract_routes_from_lua(engine.lua())?;
    let router = if !routes.is_empty() {
        println!(
            "{} {} route(s) from bundle",
            style("Loaded").green(),
            routes.len()
        );
        Some(BundleRouter::new(routes)?)
    } else {
        println!("{}", style("No routes found in bundle").yellow());
        None
    };

    // Load app.html from dist or use default
    let app_html_path = dist_dir.join("app.html");
    let app_html_template = if app_html_path.exists() {
        std::fs::read_to_string(&app_html_path).ok()
    } else {
        None
    };

    let state = Arc::new(AppState {
        engine: RwLock::new(engine),
        config: config.clone(),
        router,
        app_html_template,
    });

    // Serve static files from dist/
    let public_dir = dist_dir.join("public");
    let static_dir = dist_dir.join("static");

    let app = Router::new()
        .nest_service("/public", ServeDir::new(&public_dir))
        .nest_service("/static", ServeDir::new(&static_dir))
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
    axum::serve(listener, app).await?;

    Ok(())
}

/// Extract routes from __routes global in Lua state
fn extract_routes_from_lua(lua: &Lua) -> anyhow::Result<Vec<BundleRoute>> {
    let globals = lua.globals();
    let routes_table: Option<Table> = globals.get("__routes").ok();

    let Some(routes_table) = routes_table else {
        return Ok(Vec::new());
    };

    let mut routes = Vec::new();

    for pair in routes_table.pairs::<i64, Table>() {
        let (_, route_table) = pair?;

        let pattern: String = route_table.get("pattern")?;
        let page: Option<String> = route_table.get("page").ok();
        let server: Option<String> = route_table.get("server").ok();
        let api: Option<String> = route_table.get("api").ok();
        let error: Option<String> = route_table.get("error").ok();

        let layouts: Vec<String> = if let Ok(layouts_table) = route_table.get::<Table>("layouts") {
            layouts_table
                .pairs::<i64, String>()
                .filter_map(|p| p.ok().map(|(_, v)| v))
                .collect()
        } else {
            Vec::new()
        };

        let layout_servers: Vec<String> = if let Ok(layouts_table) = route_table.get::<Table>("layout_servers") {
            layouts_table
                .pairs::<i64, String>()
                .filter_map(|p| p.ok().map(|(_, v)| v))
                .collect()
        } else {
            Vec::new()
        };

        let action_templates: HashMap<String, String> =
            if let Ok(templates_table) = route_table.get::<Table>("action_templates") {
                templates_table
                    .pairs::<String, String>()
                    .filter_map(|p| p.ok())
                    .collect()
            } else {
                HashMap::new()
            };

        routes.push(BundleRoute {
            pattern,
            page,
            server,
            api,
            error,
            layouts,
            layout_servers,
            action_templates,
        });
    }

    Ok(routes)
}

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

    if let Some(ref router) = state.router {
        if let Some((route, params)) = router.match_url(&path) {
            let body_bytes = if method != Method::GET && method != Method::HEAD {
                match axum::body::to_bytes(body, MAX_BODY_SIZE).await {
                    Ok(bytes) => {
                        if bytes.is_empty() {
                            None
                        } else {
                            Some(bytes.to_vec())
                        }
                    }
                    Err(_) => return (StatusCode::BAD_REQUEST, "Body too large").into_response(),
                }
            } else {
                None
            };

            let headers_map: HashMap<String, String> = headers
                .iter()
                .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.to_string())))
                .collect();

            let luat_request = to_luat_request(&path, &method, query, body_bytes, headers_map);
            let engine_route = bundle_route_to_engine_route(route, &params);

            let engine = state.engine.read().await;
            return match engine.respond_async(&engine_route, &luat_request).await {
                Ok(response) => luat_response_to_http(response, &state),
                Err(e) => error_page(&format!("Error: {}", e)),
            };
        }
    }

    error_page("Page not found")
}

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

fn bundle_route_to_engine_route(
    route: &BundleRoute,
    params: &[(String, String)],
) -> luat::router::Route {
    let fs_path = route
        .page
        .as_ref()
        .and_then(|p| Path::new(p).parent().map(|p| p.to_string_lossy().to_string()))
        .or_else(|| {
            route
                .api
                .as_ref()
                .and_then(|p| Path::new(p).parent().map(|p| p.to_string_lossy().to_string()))
        })
        .unwrap_or_default();

    let mut engine_route = luat::router::Route::new(route.pattern.clone(), fs_path);
    engine_route.params = params.iter().cloned().collect();
    engine_route.page = route.page.clone();
    engine_route.page_server = route.server.clone();
    engine_route.api = route.api.clone();
    engine_route.error = route.error.clone();
    engine_route.layouts = route.layouts.clone();
    engine_route.layout_servers = route.layout_servers.clone();
    engine_route.action_templates = route.action_templates.clone();
    engine_route
}

fn luat_response_to_http(response: LuatResponse, state: &AppState) -> Response {
    match response {
        LuatResponse::Html {
            status,
            mut headers,
            body,
        } => {
            let is_fragment = headers.remove("x-luat-fragment").is_some()
                || headers.remove("X-Luat-Fragment").is_some();
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let has_content_type = has_content_type_header(&headers);

            let mut builder = axum::http::Response::builder().status(status_code);
            for (key, value) in headers {
                builder = builder.header(key, value);
            }

            if !has_content_type {
                builder = builder.header("content-type", "text/html; charset=utf-8");
            }

            if is_fragment {
                return builder.body(Body::from(body)).unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response")
                        .into_response()
                });
            }

            let title = "Luat App";
            let head_assets = collect_production_head_assets(&state.config);
            let app_html = state
                .app_html_template
                .as_deref()
                .unwrap_or(DEFAULT_APP_HTML);
            let full_html = wrap_with_app_html(app_html, &body, title, &head_assets);

            builder.body(Body::from(full_html)).unwrap_or_else(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
            })
        }
        LuatResponse::Json {
            status,
            headers,
            body,
        } => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let has_content_type = has_content_type_header(&headers);
            let mut builder = axum::http::Response::builder().status(status_code);

            for (key, value) in headers {
                builder = builder.header(key, value);
            }

            if !has_content_type {
                builder = builder.header("content-type", "application/json");
            }

            builder
                .body(Body::from(
                    serde_json::to_string(&body).unwrap_or_default(),
                ))
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response")
                        .into_response()
                })
        }
        LuatResponse::Redirect { status, location } => {
            let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::FOUND);
            Response::builder()
                .status(status_code)
                .header("location", location)
                .body(Body::empty())
                .unwrap_or_else(|_| {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response")
                        .into_response()
                })
        }
        LuatResponse::Error { status, message } => {
            let _ = status;
            error_page(&message)
        }
    }
}

fn has_content_type_header(headers: &HashMap<String, String>) -> bool {
    headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("content-type"))
}

fn error_page(message: &str) -> Response {
    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Error</title>
    <style>
        body {{ font-family: system-ui, sans-serif; padding: 2rem; background: #f5f5f5; color: #333; }}
        .error {{ background: white; border-left: 4px solid #e53e3e; padding: 1rem; border-radius: 4px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
        pre {{ background: #1a1a2e; color: #eee; padding: 1rem; overflow-x: auto; border-radius: 4px; }}
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

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Collect head assets for production
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

fn wrap_with_app_html(app_html: &str, body: &str, title: &str, head_assets: &str) -> String {
    app_html
        .replace("%luat.title%", title)
        .replace("%luat.head%", head_assets)
        .replace("%luat.body%", body)
}

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
