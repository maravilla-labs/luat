// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Integration tests for the SvelteKit-style routing system.
//!
//! These tests verify the full routing pipeline using the actual crate code.

use std::fs;
use std::path::Path;

use tempfile::tempdir;

// Import the actual router from the crate
use luat_cli::router::Router as LuatRouter;
use luat_cli::server::loader::{run_api_handler, run_load_function, LoadContext};

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
    let root_layout = r#"<!DOCTYPE html>
<html>
<head><title>{props.title or "Test"}</title></head>
<body>
<nav>Test Nav</nav>
<main>{@html props.children}</main>
</body>
</html>"#;
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

#[cfg(test)]
mod route_discovery_tests {
    use super::*;

    #[test]
    fn test_discovers_all_routes() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        let routes = router.routes();

        // Should have: /, /about, /blog, /blog/{slug}, /users/{id}, /api/hello
        assert!(routes.len() >= 6, "Expected at least 6 routes, found {}", routes.len());
    }

    #[test]
    fn test_discovers_root_route() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        let root = router.match_url("/");
        assert!(root.is_some(), "Should discover root route");
        assert!(root.unwrap().route.page.is_some(), "Root should have page");
    }

    #[test]
    fn test_discovers_static_routes() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        let about = router.match_url("/about");
        assert!(about.is_some(), "Should discover /about route");

        let blog = router.match_url("/blog");
        assert!(blog.is_some(), "Should discover /blog route");
    }

    #[test]
    fn test_discovers_dynamic_routes() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        // Test blog slug route
        let post = router.match_url("/blog/my-first-post");
        assert!(post.is_some(), "Should match /blog/my-first-post");
        let post_match = post.unwrap();
        assert_eq!(post_match.param("slug"), Some("my-first-post"));

        // Test user id route
        let user = router.match_url("/users/123");
        assert!(user.is_some(), "Should match /users/123");
        let user_match = user.unwrap();
        assert_eq!(user_match.param("id"), Some("123"));
    }

    #[test]
    fn test_discovers_api_routes() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        let api = router.match_url("/api/hello");
        assert!(api.is_some(), "Should discover /api/hello route");
        assert!(api.unwrap().route.is_api_route(), "Should be API route");
    }

    #[test]
    fn test_discovers_layouts() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        // Home page should have root layout
        let home = router.match_url("/");
        assert!(home.is_some());
        let home_route = home.unwrap().route;
        assert!(!home_route.layouts.is_empty(), "Home should have layouts");

        // Blog page should also have root layout
        let blog = router.match_url("/blog");
        assert!(blog.is_some());
        let blog_route = blog.unwrap().route;
        assert!(!blog_route.layouts.is_empty(), "Blog should have layouts");
    }

    #[test]
    fn test_discovers_server_files() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        // Home should have server file
        let home = router.match_url("/");
        assert!(home.unwrap().route.server.is_some(), "Home should have +page.server.lua");

        // About should NOT have server file
        let about = router.match_url("/about");
        assert!(about.unwrap().route.server.is_none(), "About should not have +page.server.lua");

        // Blog post should have server file
        let post = router.match_url("/blog/test");
        assert!(post.unwrap().route.server.is_some(), "Blog post should have +page.server.lua");
    }
}

#[cfg(test)]
mod load_function_tests {
    use super::*;
    use axum::http::Method;
    use mlua::Lua;

    #[test]
    fn test_load_function_basic() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let server_file = dir.path().join("src/routes/+page.server.lua");
        let lua = Lua::new();
        let ctx = LoadContext::new("/".to_string(), Method::GET, vec![]);

        let result = run_load_function(&lua, &server_file, &ctx, None).unwrap();

        // Check that props were returned
        assert!(result.props.is_object());
        let props = result.props.as_object().unwrap();
        assert!(props.contains_key("message"));
        assert_eq!(props.get("message").unwrap(), "Welcome!");
    }

    #[test]
    fn test_load_function_with_params() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let server_file = dir.path().join("src/routes/blog/[slug]/+page.server.lua");
        let lua = Lua::new();
        let ctx = LoadContext::new(
            "/blog/hello-world".to_string(),
            Method::GET,
            vec![("slug".to_string(), "hello-world".to_string())],
        );

        let result = run_load_function(&lua, &server_file, &ctx, None).unwrap();

        let props = result.props.as_object().unwrap();
        assert!(props.contains_key("post"));

        let post = props.get("post").unwrap().as_object().unwrap();
        assert!(post.get("title").unwrap().as_str().unwrap().contains("hello-world"));
    }

    #[test]
    fn test_api_handler_get() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let server_file = dir.path().join("src/routes/api/hello/+server.lua");
        let lua = Lua::new();
        let ctx = LoadContext::new("/api/hello".to_string(), Method::GET, vec![]);

        let result = run_api_handler(&lua, &server_file, &ctx, None).unwrap();

        assert_eq!(result.status, 200);
        let body = result.body.as_object().unwrap();
        assert_eq!(body.get("message").unwrap(), "Hello from API");
    }

    #[test]
    fn test_api_handler_post() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let server_file = dir.path().join("src/routes/api/hello/+server.lua");
        let lua = Lua::new();

        let mut form = std::collections::HashMap::new();
        form.insert("name".to_string(), "Claude".to_string());

        let ctx = LoadContext::new("/api/hello".to_string(), Method::POST, vec![])
            .with_form(form);

        let result = run_api_handler(&lua, &server_file, &ctx, None).unwrap();

        assert_eq!(result.status, 201);
        let body = result.body.as_object().unwrap();
        assert_eq!(body.get("greeting").unwrap(), "Hello, Claude");
    }

    #[test]
    fn test_api_handler_method_not_allowed() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let server_file = dir.path().join("src/routes/api/hello/+server.lua");
        let lua = Lua::new();
        let ctx = LoadContext::new("/api/hello".to_string(), Method::DELETE, vec![]);

        let result = run_api_handler(&lua, &server_file, &ctx, None).unwrap();

        assert_eq!(result.status, 405); // Method not allowed
    }
}

#[cfg(test)]
mod url_matching_tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        assert!(router.match_url("/").is_some());
        assert!(router.match_url("/about").is_some());
        assert!(router.match_url("/blog").is_some());
    }

    #[test]
    fn test_dynamic_param_extraction() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        let matched = router.match_url("/blog/test-slug").unwrap();
        assert_eq!(matched.param("slug"), Some("test-slug"));

        let matched = router.match_url("/users/42").unwrap();
        assert_eq!(matched.param("id"), Some("42"));
    }

    #[test]
    fn test_no_match_returns_none() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        assert!(router.match_url("/nonexistent").is_none());
        assert!(router.match_url("/blog/slug/extra/path").is_none());
    }

    #[test]
    fn test_trailing_slash_handling() {
        let dir = tempdir().unwrap();
        setup_test_project(dir.path());

        let routes_dir = dir.path().join("src/routes");
        let router = LuatRouter::discover(&routes_dir).unwrap();

        // Should match with or without trailing slash
        assert!(router.match_url("/about").is_some());
        assert!(router.match_url("/about/").is_some());
    }
}
