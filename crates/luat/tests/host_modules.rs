// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Hosts can expose their own modules to guest code via `require`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use luat::memory_resolver::MemoryResourceResolver;
use luat::{Engine, LuatRequest, LuatResponse, Router};

fn counting_engine(files: &[(&str, &str)]) -> (Engine<MemoryResourceResolver>, Arc<AtomicUsize>) {
    let resolver = MemoryResourceResolver::new();
    for (path, source) in files {
        resolver.add_template(path, source.to_string());
    }
    let engine = Engine::with_memory_cache(resolver, 8).unwrap();
    let builds = Arc::new(AtomicUsize::new(0));
    let counter = builds.clone();
    engine
        .register_module("myhost", move |lua| {
            counter.fetch_add(1, Ordering::SeqCst);
            let module = lua.create_table()?;
            module.set(
                "greet",
                lua.create_function(|_, name: String| Ok(format!("hello {name}")))?,
            )?;
            Ok(module)
        })
        .unwrap();
    (engine, builds)
}

#[test]
fn module_is_built_lazily_and_once() {
    let (engine, builds) = counting_engine(&[]);
    assert_eq!(builds.load(Ordering::SeqCst), 0, "built before anyone required it");

    let lua = engine.lua();
    let a: String = lua.load("return require('myhost').greet('a')").eval().unwrap();
    let b: String = lua.load("return require('myhost').greet('b')").eval().unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("hello a", "hello b"));
    assert_eq!(builds.load(Ordering::SeqCst), 1);
}

#[test]
fn host_module_wins_over_file_of_same_name() {
    let (engine, _) = counting_engine(&[("myhost.lua", "return { greet = function() return 'file' end }")]);
    let got: String = engine
        .lua()
        .load("return require('myhost').greet('x')")
        .eval()
        .unwrap();
    assert_eq!(got, "hello x");
}

#[cfg(feature = "async-lua")]
#[tokio::test]
async fn server_code_can_require_host_module() {
    let (engine, _) = counting_engine(&[(
        "api/+server.lua",
        "local host = require('myhost')\nfunction GET(ctx) return { body = { msg = host.greet('api') } } end",
    )]);
    let router = Router::from_paths(["api/+server.lua"].into_iter());
    let route = router.match_url("/api").unwrap();
    let response = engine
        .respond_async(&route, &LuatRequest::new("/api", "GET"))
        .await
        .unwrap();
    let LuatResponse::Json { body, .. } = response else {
        panic!("expected json, got {response:?}");
    };
    assert_eq!(body["msg"], "hello api");
}
