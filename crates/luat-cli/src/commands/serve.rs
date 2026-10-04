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
    response::{IntoResponse, Response},
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
    /// The HTML shell pages are rendered into (`dist/app.html`).
    pub shell: luat::AppShell,
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
        shell: app_html_template.map(luat::AppShell::new).unwrap_or_default(),
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
                Ok(response) => luat_response_to_http(response, &state, &luat_request),
                // Only execution-limit errors reach here.
                Err(e) => {
                    tracing::error!(error = %e, "request stopped");
                    crate::server::response::error(500, "Internal Server Error")
                }
            };
        }
    }

    crate::server::response::error(404, "Not Found")
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

fn luat_response_to_http(response: LuatResponse, state: &AppState, request: &LuatRequest) -> Response {
    let options = luat::ShellOptions {
        head: collect_production_head_assets(&state.config),
        ..Default::default()
    };
    crate::server::response::to_axum(luat::finalize(response, request, &state.shell, &options))
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

