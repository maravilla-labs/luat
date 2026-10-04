// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Integration tests for SvelteKit-style routing as the dev server runs it:
//! the core router discovers routes and the engine handles requests from
//! files on disk.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use luat::{Engine, FileSystemResolver, LuatRequest, LuatResponse, NoOpCache, Router};
use tempfile::tempdir;

/// Create a test project structure in a temp directory
fn setup_test_project(dir: &Path) {
    // Create routes directory structure
    fs::create_dir_all(dir.join("src/routes")).unwrap();
    fs::create_dir_all(dir.join("src/routes/about")).unwrap();
    fs::create_dir_all(dir.join("src/routes/blog")).unwrap();
    fs::create_dir_all(dir.join("src/routes/blog/[slug]")).unwrap();
    fs::create_dir_all(dir.join("src/routes/api/hello")).unwrap();
    fs::create_dir_all(dir.join("src/routes/users/[id]")).unwrap();
    fs::create_dir_all(dir.join("src/lib")).unwrap();
    fs::create_dir_all(dir.join("static")).unwrap();
    fs::create_dir_all(dir.join("public")).unwrap();

    // Create root layout
    // The document shell (<!DOCTYPE>, <html>, <head>) lives in app.html.
    let root_layout = r#"<nav>Test Nav</nav>
<main>{@html props.children}</main>"#;
    fs::write(dir.join("src/routes/+layout.luat"), root_layout).unwrap();

    // Create home page
    let home_page = r#"<h1>Home</h1>
<p>{props.message}</p>"#;
    fs::write(dir.join("src/routes/+page.luat"), home_page).unwrap();

    // Create home page server
    let home_server = r#"function load(ctx)
    return {
        title = "Home",
        message = "Welcome!"
    }
end"#;
    fs::write(dir.join("src/routes/+page.server.lua"), home_server).unwrap();

    // Create about page (no server file)
    let about_page = r#"<h1>About</h1>
<p>About page content</p>"#;
    fs::write(dir.join("src/routes/about/+page.luat"), about_page).unwrap();

    // Create blog list page
    let blog_page = r#"<h1>Blog</h1>
<p>Post count: {#props.posts}</p>"#;
    fs::write(dir.join("src/routes/blog/+page.luat"), blog_page).unwrap();

    // Create blog list server
    let blog_server = r#"function load(ctx)
    return {
        title = "Blog",
        posts = {
            { title = "Post 1", slug = "post-1" },
            { title = "Post 2", slug = "post-2" }
        }
    }
end"#;
    fs::write(dir.join("src/routes/blog/+page.server.lua"), blog_server).unwrap();

    // Create dynamic blog post page
    let post_page = r#"<article>
<h1>{props.post.title}</h1>
<p>{props.post.content}</p>
</article>"#;
    fs::write(dir.join("src/routes/blog/[slug]/+page.luat"), post_page).unwrap();

    // Create blog post server with dynamic param
    let post_server = r#"function load(ctx)
    local slug = ctx.params.slug
    return {
        title = slug,
        post = {
            title = "Post: " .. slug,
            content = "Content for " .. slug
        }
    }
end"#;
    fs::write(dir.join("src/routes/blog/[slug]/+page.server.lua"), post_server).unwrap();

    // Create user page with dynamic param
    let user_page = r#"<div class="user">
<h1>User {props.params.id}</h1>
<p>Name: {props.user.name}</p>
</div>"#;
    fs::write(dir.join("src/routes/users/[id]/+page.luat"), user_page).unwrap();

    let user_server = r#"function load(ctx)
    return {
        user = {
            id = ctx.params.id,
            name = "User " .. ctx.params.id
        }
    }
end"#;
    fs::write(dir.join("src/routes/users/[id]/+page.server.lua"), user_server).unwrap();

    // Create API route
    let api_server = r#"function GET(ctx)
    return {
        status = 200,
        body = {
            message = "Hello from API",
            timestamp = 12345
        }
    }
end

function POST(ctx)
    local name = "World"
    if ctx.form and ctx.form.name then
        name = ctx.form.name
    end
    return {
        status = 201,
        body = {
            greeting = "Hello, " .. name
        }
    }
end"#;
    fs::write(dir.join("src/routes/api/hello/+server.lua"), api_server).unwrap();
}


/// Discovers routes and builds an engine over the project's files.
fn app(dir: &Path) -> (Router, Engine<FileSystemResolver>) {
    let routes_dir = dir.join("src/routes");
    let router = Router::discover(&routes_dir).unwrap();
    let resolver = FileSystemResolver::new(&routes_dir).with_lib_dir(dir.join("src/lib"));
    let engine = Engine::new(resolver, Box::new(NoOpCache::new())).unwrap();
    (router, engine)
}

fn respond(router: &Router, engine: &Engine<FileSystemResolver>, request: LuatRequest) -> LuatResponse {
    let route = router.match_url(&request.path).expect("route");
    engine.set_development_mode(true).unwrap();
    engine.respond(&route, &request).unwrap()
}

fn html(response: LuatResponse) -> String {
    match response {
        LuatResponse::Html { body, .. } => body,
        other => panic!("expected html, got {other:?}"),
    }
}

fn json(response: LuatResponse) -> (u16, serde_json::Value) {
    match response {
        LuatResponse::Json { status, body, .. } => (status, body),
        other => panic!("expected json, got {other:?}"),
    }
}

mod route_discovery_tests {
    use super::*;

    #[test]
    fn discovers_all_routes() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        // /, /about, /blog, /blog/{slug}, /users/{id}, /api/hello
        assert_eq!(router.routes().len(), 6);
    }

    #[test]
    fn discovers_pages_api_routes_and_server_files() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());

        assert!(router.match_url("/").unwrap().page.is_some());
        assert!(router.match_url("/api/hello").unwrap().is_api_route());
        assert!(router.match_url("/").unwrap().page_server.is_some());
        assert!(router.match_url("/about").unwrap().page_server.is_none());
        assert!(router.match_url("/blog/test").unwrap().page_server.is_some());
    }

    #[test]
    fn discovers_layouts() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        assert_eq!(router.match_url("/").unwrap().layouts, ["+layout.luat"]);
        assert_eq!(router.match_url("/blog").unwrap().layouts, ["+layout.luat"]);
    }
}

mod request_tests {
    use super::*;

    #[test]
    fn load_function_props_reach_the_page_and_layout() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, engine) = app(dir.path());

        let body = html(respond(&router, &engine, LuatRequest::new("/", "GET")));
        assert!(body.contains("<p>Welcome!</p>"), "{body}");
        assert!(body.contains("<nav>Test Nav</nav>"), "layout missing: {body}");
    }

    #[test]
    fn load_function_receives_route_params() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, engine) = app(dir.path());

        let body = html(respond(&router, &engine, LuatRequest::new("/blog/hello-world", "GET")));
        assert!(body.contains("Post: hello-world"), "{body}");
    }

    #[test]
    fn api_get() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, engine) = app(dir.path());

        let (status, body) = json(respond(&router, &engine, LuatRequest::new("/api/hello", "GET")));
        assert_eq!(status, 200);
        assert_eq!(body["message"], "Hello from API");
    }

    #[test]
    fn api_post_reads_form_body() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, engine) = app(dir.path());

        let request = LuatRequest::new("/api/hello", "POST")
            .with_headers(HashMap::from([(
                "content-type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            )]))
            .with_body(b"name=Ada".to_vec());
        let (status, body) = json(respond(&router, &engine, request));
        assert_eq!(status, 201);
        assert_eq!(body["greeting"], "Hello, Ada");
    }

    #[test]
    fn api_unknown_method_is_405() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, engine) = app(dir.path());

        let (status, _) = json(respond(&router, &engine, LuatRequest::new("/api/hello", "DELETE")));
        assert_eq!(status, 405);
    }
}

mod url_matching_tests {
    use super::*;

    #[test]
    fn exact_matches() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        for path in ["/", "/about", "/blog"] {
            assert!(router.match_url(path).is_some(), "{path}");
        }
    }

    #[test]
    fn dynamic_params_are_extracted() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        assert_eq!(router.match_url("/blog/test-slug").unwrap().params["slug"], "test-slug");
        assert_eq!(router.match_url("/users/42").unwrap().params["id"], "42");
    }

    #[test]
    fn unknown_paths_do_not_match() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        assert!(router.match_url("/nonexistent").is_none());
        assert!(router.match_url("/blog/slug/extra/path").is_none());
    }

    #[test]
    fn trailing_slash_is_ignored() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());
        let (router, _) = app(dir.path());
        assert!(router.match_url("/about/").is_some());
    }
}
