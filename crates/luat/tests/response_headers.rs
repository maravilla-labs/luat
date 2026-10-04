// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! `ctx.setHeader`, `ctx.appendHeader` and `ctx.setStatus`, the
//! `partitioned` cookie option, and the request URL fields `ctx.path`,
//! `ctx.search` and `ctx.href`.

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, LuatRequest, LuatResponse, Router};

fn app(files: &[(&str, &str)]) -> (Engine<MemoryResourceResolver>, Router) {
    let resolver = MemoryResourceResolver::new();
    for (path, source) in files {
        resolver.add_template(path, source.to_string());
    }
    let engine = Engine::with_memory_cache(resolver, 32).unwrap();
    engine.set_development_mode(true).unwrap();
    let router = Router::from_paths(files.iter().map(|(p, _)| *p));
    (engine, router)
}

fn get(
    engine: &Engine<MemoryResourceResolver>,
    router: &Router,
    request: LuatRequest,
) -> LuatResponse {
    let route = router.match_url(&request.path).expect("route");
    engine.respond(&route, &request).unwrap()
}

fn all<'a>(response: &'a LuatResponse, name: &'a str) -> Vec<&'a str> {
    response.headers().get_all(name).collect()
}

fn html(response: &LuatResponse) -> &str {
    match response {
        LuatResponse::Html { body, .. } => body,
        other => panic!("expected html, got {other:?}"),
    }
}

#[test]
fn layout_and_page_loads_set_headers() {
    let (engine, router) = app(&[
        ("+layout.luat", "{@html props.children}"),
        (
            "+layout.server.lua",
            "function load(ctx) ctx.setHeader('Cache-Control', 'no-store'); ctx.appendHeader('Vary', 'Cookie'); return {} end",
        ),
        ("+page.luat", "<p>ok</p>"),
        (
            "+page.server.lua",
            r#"function load(ctx)
                ctx.setHeader('cache-control', 'public, max-age=60')
                ctx.appendHeader('Vary', 'Accept-Language')
                ctx.setHeader('Content-Security-Policy', "frame-ancestors 'self'")
                ctx.setHeader('X-Robots-Tag', 'noindex')
                ctx.setCookie('a', '1')
                return {}
            end"#,
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    assert_eq!(response.status(), 200);
    // The page (leaf) runs after the layout, so its setHeader wins.
    assert_eq!(all(&response, "cache-control"), ["public, max-age=60"]);
    assert_eq!(all(&response, "vary"), ["Cookie", "Accept-Language"]);
    assert_eq!(
        all(&response, "content-security-policy"),
        ["frame-ancestors 'self'"]
    );
    assert_eq!(all(&response, "x-robots-tag"), ["noindex"]);
    assert_eq!(
        all(&response, "set-cookie"),
        ["a=1; Path=/; SameSite=Lax; HttpOnly"]
    );
}

#[test]
fn set_status_changes_the_page_status() {
    let (engine, router) = app(&[
        ("+page.luat", "<p>{props.msg}</p>"),
        (
            "+page.server.lua",
            "function load(ctx) ctx.setStatus(410); return { msg = 'gone' } end",
        ),
        ("legacy/+page.luat", "<p>{props.msg}</p>"),
        (
            "legacy/+page.server.lua",
            "function load(ctx) return { status = 404, msg = 'not here' } end",
        ),
        ("odd/+page.luat", "<p>odd</p>"),
        (
            "odd/+page.server.lua",
            "function load(ctx) return { status = 301 } end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    assert_eq!((response.status(), html(&response)), (410, "<p>gone</p>"));

    // A returned `status` without `redirect` sets the page status too.
    let response = get(&engine, &router, LuatRequest::new("/legacy", "GET"));
    assert_eq!(
        (response.status(), html(&response)),
        (404, "<p>not here</p>")
    );

    // Redirect statuses without a redirect are ignored, as before.
    assert_eq!(
        get(&engine, &router, LuatRequest::new("/odd", "GET")).status(),
        200
    );
}

#[test]
fn redirect_return_keeps_working() {
    let (engine, router) = app(&[
        ("+page.luat", "<p>x</p>"),
        (
            "+page.server.lua",
            "function load(ctx) ctx.setHeader('Cache-Control', 'no-store'); return { redirect = '/login', status = 303 } end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    let LuatResponse::Redirect {
        status, location, ..
    } = &response
    else {
        panic!("expected redirect, got {response:?}");
    };
    assert_eq!((*status, location.as_str()), (303, "/login"));
    assert_eq!(all(&response, "cache-control"), ["no-store"]);
}

#[test]
fn invalid_headers_and_statuses_are_errors() {
    let cases = [
        (
            "ctx.setHeader('X-Evil', 'a\\r\\nSet-Cookie: x=1')",
            "invalid value",
        ),
        ("ctx.setHeader('Bad Name', 'v')", "invalid header name"),
        ("ctx.setHeader('Set-Cookie', 'a=1')", "ctx.setCookie"),
        ("ctx.appendHeader('set-cookie', 'a=1')", "ctx.setCookie"),
        ("ctx.setHeader('Content-Length', '1')", "cannot be set"),
        ("ctx.setStatus(302)", "redirect"),
        ("ctx.setStatus(99)", "200-299 or 400-599"),
    ];
    for (call, expected) in cases {
        let page_server = format!("function load(ctx) {call}; return {{}} end");
        let (engine, router) = app(&[
            ("+page.luat", "<p>x</p>"),
            ("+page.server.lua", &page_server),
        ]);
        let response = get(&engine, &router, LuatRequest::new("/", "GET"));
        let LuatResponse::Error {
            status, message, ..
        } = &response
        else {
            panic!("{call}: expected error, got {response:?}");
        };
        assert_eq!(*status, 500, "{call}");
        assert!(message.contains(expected), "{call}: {message}");
        assert!(response.headers().get("x-evil").is_none());
    }
}

#[test]
fn headers_are_dropped_when_the_request_fails() {
    let (engine, router) = app(&[
        ("+page.luat", "<p>x</p>"),
        (
            "+page.server.lua",
            "function load(ctx) ctx.setHeader('Cache-Control', 'public, max-age=3600'); ctx.setCookie('c', '1'); ctx.error(404) end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/", "GET"));
    assert_eq!(response.status(), 404);
    assert!(response.headers().get("cache-control").is_none());
    assert_eq!(
        all(&response, "set-cookie").len(),
        1,
        "cookies are still sent"
    );
}

#[test]
fn api_handlers_and_actions_set_headers() {
    let (engine, router) = app(&[
        (
            "api/+server.lua",
            r#"function GET(ctx)
                ctx.setHeader('Cache-Control', 'no-store')
                ctx.setHeader('X-From-Ctx', 'ctx')
                return { headers = { ['x-from-ctx'] = 'returned' }, body = 'x' }
            end"#,
        ),
        (
            "status/+server.lua",
            "function GET(ctx) ctx.setStatus(201) end",
        ),
        ("form/+page.luat", "<form></form>"),
        (
            "form/+page.server.lua",
            "actions = { default = function(ctx) ctx.setHeader('HX-Trigger', 'saved'); return { ok = true } end }",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/api", "GET"));
    assert_eq!(all(&response, "cache-control"), ["no-store"]);
    // Headers the handler returns win over ctx.setHeader.
    assert_eq!(all(&response, "x-from-ctx"), ["returned"]);

    let response = get(&engine, &router, LuatRequest::new("/status", "GET"));
    let LuatResponse::Json { status, body, .. } = &response else {
        panic!("expected json error, got {response:?}");
    };
    assert_eq!(*status, 500);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("only available in load"),
        "{body}"
    );

    let response = get(&engine, &router, LuatRequest::new("/form", "POST"));
    assert_eq!(all(&response, "hx-trigger"), ["saved"]);
}

#[test]
fn partitioned_cookies() {
    let (engine, router) = app(&[
        (
            "ok/+server.lua",
            "function GET(ctx) ctx.setCookie('embed', '1', { secure = true, sameSite = 'None', partitioned = true }); return { body = 'x' } end",
        ),
        (
            "insecure/+server.lua",
            "function GET(ctx) ctx.setCookie('embed', '1', { sameSite = 'None' }); return { body = 'x' } end",
        ),
    ]);
    let response = get(&engine, &router, LuatRequest::new("/ok", "GET"));
    assert_eq!(
        all(&response, "set-cookie"),
        ["embed=1; Path=/; SameSite=None; Secure; HttpOnly; Partitioned"]
    );
    let response = get(&engine, &router, LuatRequest::new("/insecure", "GET"));
    assert_eq!(response.status(), 500);
}

#[test]
fn request_url_fields() {
    let (engine, router) = app(&[
        ("page/+page.luat", "<p>{props.url}|{props.path}|{props.search}|{props.href}</p>"),
        (
            "page/+page.server.lua",
            "function load(ctx) return { url = ctx.url, path = ctx.path, search = ctx.search, href = ctx.href } end",
        ),
        (
            "api/+server.lua",
            "function GET(ctx) return { body = { url = ctx.url, path = ctx.path, search = ctx.search, href = ctx.href } } end",
        ),
        ("form/+page.luat", "<form></form>"),
        (
            "form/+page.server.lua",
            "actions = { save = function(ctx) return { path = ctx.path, search = ctx.search, href = ctx.href } end }",
        ),
    ]);
    let request = LuatRequest::new("/page", "GET")
        .with_query([("q".into(), "a b".into())].into())
        .with_raw_query("q=a%20b&x");
    let response = get(&engine, &router, request);
    // ctx.url stays the path for compatibility.
    assert_eq!(
        html(&response),
        "<p>/page|/page|?q=a%20b&amp;x|/page?q=a%20b&amp;x</p>"
    );

    let LuatResponse::Json { body, .. } = get(&engine, &router, LuatRequest::new("/api", "GET"))
    else {
        panic!("expected json");
    };
    assert_eq!(body["url"], "/api");
    assert_eq!(body["search"], "");
    assert_eq!(body["href"], "/api");

    let request = LuatRequest::new("/form", "POST")
        .with_query([("/save".into(), "".into())].into())
        .with_raw_query("/save");
    let LuatResponse::Json { body, .. } = get(&engine, &router, request) else {
        panic!("expected json");
    };
    assert_eq!(body["path"], "/form");
    assert_eq!(body["search"], "?/save");
    assert_eq!(body["href"], "/form?/save");
}
