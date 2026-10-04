// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Async host functions must be callable from every server entry point
//! (`load`, API handlers, actions) and from templates when a request is
//! handled through `Engine::respond_async`.

#![cfg(feature = "async-lua")]

use std::time::Duration;

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, LuatRequest, LuatResponse, Router};

/// Builds an engine whose global `host.lookup(key)` is an async function
/// that sleeps before answering, so it really suspends the coroutine.
fn engine_with_async_host(files: &[(&str, &str)]) -> (Engine<MemoryResourceResolver>, Router) {
    let resolver = MemoryResourceResolver::new();
    for (path, source) in files {
        resolver.add_template(path, source.to_string());
    }
    let engine = Engine::with_memory_cache(resolver, 32).unwrap();

    let lua = engine.lua();
    let host = lua.create_table().unwrap();
    let lookup = lua
        .create_async_function(|_, key: String| async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            Ok(format!("value-of-{key}"))
        })
        .unwrap();
    host.set("lookup", lookup).unwrap();
    lua.globals().set("host", host).unwrap();

    let router = Router::from_paths(files.iter().map(|(p, _)| *p));
    (engine, router)
}

async fn respond(
    engine: &Engine<MemoryResourceResolver>,
    router: &Router,
    request: LuatRequest,
) -> LuatResponse {
    let route = router.match_url(&request.path).expect("route");
    engine.respond_async(&route, &request).await.unwrap()
}

#[tokio::test]
async fn load_function_can_await_host() {
    let (engine, router) = engine_with_async_host(&[
        ("+page.luat", "<p>{props.answer}</p>"),
        (
            "+page.server.lua",
            "function load(ctx) return { answer = host.lookup('page') } end",
        ),
    ]);

    let response = respond(&engine, &router, LuatRequest::new("/", "GET")).await;
    let LuatResponse::Html { body, .. } = response else {
        panic!("expected html, got {response:?}");
    };
    assert_eq!(body, "<p>value-of-page</p>");
}

#[tokio::test]
async fn layout_load_can_await_host() {
    let (engine, router) = engine_with_async_host(&[
        ("+layout.luat", "<main>{props.site}|{@html props.children}</main>"),
        (
            "+layout.server.lua",
            "function load(ctx) return { site = host.lookup('layout') } end",
        ),
        ("+page.luat", "<p>page</p>"),
    ]);

    let response = respond(&engine, &router, LuatRequest::new("/", "GET")).await;
    let LuatResponse::Html { body, .. } = response else {
        panic!("expected html, got {response:?}");
    };
    assert_eq!(body, "<main>value-of-layout|<p>page</p></main>");
}

#[tokio::test]
async fn api_handler_can_await_host() {
    let (engine, router) = engine_with_async_host(&[(
        "api/+server.lua",
        "function GET(ctx) return { status = 200, body = { v = host.lookup('api') } } end",
    )]);

    let response = respond(&engine, &router, LuatRequest::new("/api", "GET")).await;
    let LuatResponse::Json { body, status, .. } = response else {
        panic!("expected json, got {response:?}");
    };
    assert_eq!(status, 200);
    assert_eq!(body["v"], "value-of-api");
}

#[tokio::test]
async fn action_can_await_host() {
    let (engine, router) = engine_with_async_host(&[
        ("+page.luat", "<p>page</p>"),
        (
            "+page.server.lua",
            "actions = { default = function(ctx) return { v = host.lookup('action') } end }",
        ),
    ]);

    let request = LuatRequest::new("/", "POST");
    let response = respond(&engine, &router, request).await;
    let LuatResponse::Json { body, .. } = response else {
        panic!("expected json, got {response:?}");
    };
    assert_eq!(body["v"], "value-of-action");
}

#[tokio::test]
async fn template_script_can_await_host() {
    let (engine, router) = engine_with_async_host(&[(
        "+page.luat",
        "<script>local v = host.lookup('template')</script><p>{v}</p>",
    )]);

    let response = respond(&engine, &router, LuatRequest::new("/", "GET")).await;
    let LuatResponse::Html { body, .. } = response else {
        panic!("expected html, got {response:?}");
    };
    assert_eq!(body, "<p>value-of-template</p>");
}

#[tokio::test]
async fn handlers_do_not_leak_between_routes() {
    let (engine, router) = engine_with_async_host(&[
        ("a/+page.luat", "<p>a</p>"),
        (
            "a/+page.server.lua",
            "actions = { default = function() return { from = 'a' } end }",
        ),
        ("b/+page.luat", "<p>b</p>"),
        ("b/+page.server.lua", "-- no actions here"),
    ]);

    let _ = respond(&engine, &router, LuatRequest::new("/a", "POST")).await;
    let response = respond(&engine, &router, LuatRequest::new("/b", "POST")).await;
    // Route b defines no actions; it must not pick up route a's table.
    assert_eq!(response.status(), 500, "got {response:?}");
}
