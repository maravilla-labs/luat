// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Response model: cookies, error pages and status codes, error detail
//! scrubbing, and raw API bodies.

use std::collections::HashMap;

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, LuatRequest, LuatResponse, Router};

fn app(files: &[(&str, &str)]) -> (Engine<MemoryResourceResolver>, Router) {
    let resolver = MemoryResourceResolver::new();
    for (path, source) in files {
        resolver.add_template(path, source.to_string());
    }
    let engine = Engine::with_memory_cache(resolver, 32).unwrap();
    let router = Router::from_paths(files.iter().map(|(p, _)| *p));
    (engine, router)
}

fn get(engine: &Engine<MemoryResourceResolver>, router: &Router, request: LuatRequest) -> LuatResponse {
    let route = router.match_url(&request.path).expect("route");
    engine.respond(&route, &request).unwrap()
}

fn set_cookies(response: &LuatResponse) -> Vec<String> {
    response.headers().get_all("set-cookie").map(str::to_string).collect()
}

#[test]
fn load_function_sets_cookies() {
    let (engine, router) = app(&[
        ("+page.luat", "<p>ok</p>"),
        (
            "+page.server.lua",
            "function load(ctx) ctx.setCookie('a', '1'); ctx.setCookie('b', 'x y', { maxAge = 60, secure = true }); return {} end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    assert_eq!(response.status(), 200);
    assert_eq!(
        set_cookies(&response),
        [
            "a=1; Path=/; SameSite=Lax; HttpOnly",
            "b=x%20y; Path=/; Max-Age=60; SameSite=Lax; Secure; HttpOnly",
        ]
    );
}

#[test]
fn action_can_set_cookie_and_redirect() {
    let (engine, router) = app(&[
        ("login/+page.luat", "<form></form>"),
        (
            "login/+page.server.lua",
            "actions = { default = function(ctx) ctx.setCookie('session', 'abc'); return { redirect = '/home' } end }",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/login", "POST"));
    let LuatResponse::Redirect { status, location, .. } = &response else {
        panic!("expected redirect, got {response:?}");
    };
    assert_eq!((*status, location.as_str()), (302, "/home"));
    assert_eq!(set_cookies(&response), ["session=abc; Path=/; SameSite=Lax; HttpOnly"]);
}

#[test]
fn delete_cookie_expires_it() {
    let (engine, router) = app(&[(
        "logout/+server.lua",
        "function POST(ctx) ctx.deleteCookie('session'); return { status = 204 } end",
    )]);
    let response = get(&engine, &router, LuatRequest::new("/logout", "POST"));
    assert_eq!(
        set_cookies(&response),
        ["session=; Path=/; Max-Age=0; SameSite=Lax; HttpOnly"]
    );
}

#[test]
fn request_cookies_come_from_the_cookie_header() {
    let (engine, router) = app(&[(
        "whoami/+server.lua",
        "function GET(ctx) return { body = { session = ctx.cookies.session } } end",
    )]);
    let request = LuatRequest::new("/whoami", "GET")
        .with_headers(HashMap::from([("Cookie".to_string(), "theme=dark; session=s%201".to_string())]));
    let LuatResponse::Json { body, .. } = get(&engine, &router, request) else {
        panic!("expected json");
    };
    assert_eq!(body["session"], "s 1");
}

#[test]
fn ctx_error_renders_nearest_error_page_with_status() {
    let (engine, router) = app(&[
        ("+error.luat", "<h1>{props.status}: {props.message}</h1>"),
        ("posts/[id]/+page.luat", "<p>post</p>"),
        (
            "posts/[id]/+page.server.lua",
            "function load(ctx) ctx.error(404, 'No such post') end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/posts/7", "GET"));
    let LuatResponse::Html { status, body, .. } = &response else {
        panic!("expected html, got {response:?}");
    };
    assert_eq!(*status, 404);
    assert_eq!(body, "<h1>404: No such post</h1>");
}

#[test]
fn ctx_error_without_error_page_keeps_status() {
    let (engine, router) = app(&[
        ("+page.luat", "<p>x</p>"),
        ("+page.server.lua", "function load(ctx) ctx.error(403) end"),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    let LuatResponse::Error { status, message, .. } = &response else {
        panic!("expected error, got {response:?}");
    };
    assert_eq!((*status, message.as_str()), (403, "Forbidden"));
}

#[test]
fn internal_errors_are_hidden_outside_development() {
    let files = [
        ("+page.luat", "<p>x</p>"),
        (
            "+page.server.lua",
            "function load(ctx) error('db password is hunter2') end",
        ),
    ];

    let (engine, router) = app(&files);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    let LuatResponse::Error { status, message, .. } = &response else {
        panic!("expected error, got {response:?}");
    };
    assert_eq!(*status, 500);
    assert_eq!(message, "Internal Server Error");

    let (engine, router) = app(&files);
    engine.set_development_mode(true).unwrap();
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    let LuatResponse::Error { message, .. } = &response else {
        panic!("expected error, got {response:?}");
    };
    assert!(message.contains("hunter2"), "dev mode should show details: {message}");
}

#[test]
fn api_errors_are_json() {
    let (engine, router) = app(&[(
        "api/+server.lua",
        "function GET(ctx) ctx.error(422, 'name is required') end",
    )]);
    let response = get(&engine, &router, LuatRequest::new("/api", "GET"));
    let LuatResponse::Json { status, body, .. } = &response else {
        panic!("expected json, got {response:?}");
    };
    assert_eq!(*status, 422);
    assert_eq!(body["error"], "name is required");
}

#[test]
fn api_string_body_is_sent_raw() {
    let (engine, router) = app(&[
        (
            "feed/+server.lua",
            "function GET(ctx) return { headers = { ['content-type'] = 'application/rss+xml' }, body = '<rss/>' } end",
        ),
        ("hello/+server.lua", "function GET(ctx) return { body = 'hi' } end"),
        ("bytes/+server.lua", "function GET(ctx) return { body = '\\0\\1\\255' } end"),
    ]);

    let response = get(&engine, &router, LuatRequest::new("/feed", "GET"));
    let LuatResponse::Body { body, headers, .. } = &response else {
        panic!("expected raw body, got {response:?}");
    };
    assert_eq!((body.as_slice(), headers.get("content-type")), (&b"<rss/>"[..], Some("application/rss+xml")));

    let response = get(&engine, &router, LuatRequest::new("/hello", "GET"));
    assert_eq!(response.headers().get("content-type"), Some("text/plain; charset=utf-8"));

    let LuatResponse::Body { body, .. } = get(&engine, &router, LuatRequest::new("/bytes", "GET")) else {
        panic!("expected raw body");
    };
    assert_eq!(body, [0u8, 1, 255]);
}

#[test]
fn api_header_lists_repeat_the_header() {
    let (engine, router) = app(&[(
        "multi/+server.lua",
        "function GET(ctx) return { headers = { Link = { '</a>; rel=preload', '</b>; rel=preload' } }, body = 'x' } end",
    )]);
    let response = get(&engine, &router, LuatRequest::new("/multi", "GET"));
    assert_eq!(response.headers().get_all("link").count(), 2);
}

#[cfg(feature = "async-lua")]
#[tokio::test]
async fn async_path_has_the_same_error_handling() {
    let (engine, router) = app(&[
        ("+error.luat", "<h1>{props.status}</h1>"),
        ("+page.luat", "<p>x</p>"),
        (
            "+page.server.lua",
            "function load(ctx) ctx.setCookie('seen', '1'); ctx.error(410, 'Gone for good') end",
        ),
    ]);
    let route = router.match_url("/").unwrap();
    let response = engine
        .respond_async(&route, &LuatRequest::new("/", "GET"))
        .await
        .unwrap();
    assert_eq!(response.status(), 410);
    assert_eq!(set_cookies(&response), ["seen=1; Path=/; SameSite=Lax; HttpOnly"]);
}
