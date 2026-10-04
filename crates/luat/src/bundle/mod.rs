// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Production bundles: one Lua source file holding a whole app.
//!
//! A bundle contains the compiled templates, the server and library Lua
//! sources, the route file list and the app shell. Its first line is a
//! header naming the bundle ABI and the Luat version that built it:
//!
//! ```text
//! -- luat-bundle abi=1 luat=0.2.0
//! ```
//!
//! Hosts check the header before loading ([`Bundle::from_source`]) and turn a
//! bundle into a ready-to-serve [`App`] with [`Bundle::instantiate`]. Each
//! call creates a fresh engine, so hosts that want per-request isolation can
//! instantiate per request (or keep compiled bytecode from
//! [`Bundle::compile`] and use [`App::from_bytecode`]).

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
mod build;
mod emit;
#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
mod walk;

#[cfg(all(not(target_arch = "wasm32"), feature = "filesystem"))]
pub use build::{build, BuildOptions, BuildOutput, ModuleDir};

use mlua::Table;

use crate::app_shell::AppShell;
use crate::cache::MemoryCache;
use crate::engine::Engine;
use crate::error::{LuatError, Result};
use crate::memory_resolver::MemoryResourceResolver;
use crate::router::Router;

/// Bundle format version. Bumped whenever generated bundles stop being
/// loadable by older engines, or old bundles by newer ones.
pub const BUNDLE_ABI: u32 = 1;

/// Version of this crate, recorded in bundle headers.
pub const LUAT_VERSION: &str = env!("CARGO_PKG_VERSION");

const HEADER_PREFIX: &str = "-- luat-bundle ";

/// Parsed first line of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleHeader {
    /// Bundle ABI the bundle was built for.
    pub abi: u32,
    /// Luat version that built it.
    pub luat_version: String,
}

impl BundleHeader {
    fn current() -> Self {
        Self {
            abi: BUNDLE_ABI,
            luat_version: LUAT_VERSION.to_string(),
        }
    }

    fn to_line(&self) -> String {
        format!("{HEADER_PREFIX}abi={} luat={}", self.abi, self.luat_version)
    }

    /// Reads the header from the first line of `source`.
    pub fn parse(source: &str) -> Result<Self> {
        let first = source.lines().next().unwrap_or_default();
        let fields = first.strip_prefix(HEADER_PREFIX).ok_or_else(|| {
            LuatError::InvalidTemplate(
                "not a luat bundle (missing header); rebuild it with this luat version".to_string(),
            )
        })?;
        let mut abi = None;
        let mut luat_version = None;
        for field in fields.split_whitespace() {
            match field.split_once('=') {
                Some(("abi", v)) => abi = v.parse().ok(),
                Some(("luat", v)) => luat_version = Some(v.to_string()),
                _ => {}
            }
        }
        match (abi, luat_version) {
            (Some(abi), Some(luat_version)) => Ok(Self { abi, luat_version }),
            _ => Err(LuatError::InvalidTemplate(format!("malformed bundle header: {first}"))),
        }
    }
}

/// A bundle's Lua source, with its header checked.
#[derive(Debug, Clone)]
pub struct Bundle {
    header: BundleHeader,
    source: String,
}

impl Bundle {
    /// Wraps bundle source after checking that this engine can load it.
    pub fn from_source(source: impl Into<String>) -> Result<Self> {
        let source = source.into();
        let header = BundleHeader::parse(&source)?;
        if header.abi != BUNDLE_ABI {
            return Err(LuatError::InvalidTemplate(format!(
                "bundle ABI {} (built by luat {}) is not supported by luat {} (ABI {}); rebuild the bundle",
                header.abi, header.luat_version, LUAT_VERSION, BUNDLE_ABI
            )));
        }
        Ok(Self { header, source })
    }

    /// The bundle header.
    pub fn header(&self) -> &BundleHeader {
        &self.header
    }

    /// The bundle's Lua source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Compiles the bundle to Lua bytecode for faster loading with
    /// [`App::from_bytecode`]. Bytecode is only valid for the same Lua
    /// build; cache it per engine version, never ship it between versions.
    pub fn compile(&self) -> Result<Vec<u8>> {
        let engine = Engine::with_memory_cache(MemoryResourceResolver::new(), 1)?;
        engine.compile_bundle(&self.source)
    }

    /// Creates a fresh engine with this bundle loaded.
    pub fn instantiate(&self) -> Result<App> {
        let engine = new_engine()?;
        engine.preload_bundle_code(&self.source)?;
        App::from_loaded(engine)
    }

    /// Like [`instantiate`](Self::instantiate), with `limits` in force
    /// before any of the bundle's code runs. Use this for bundles you did
    /// not build yourself: loading a bundle executes its top-level code.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn instantiate_with_limits(&self, limits: &crate::limits::EngineLimits) -> Result<App> {
        let engine = new_engine()?;
        engine.set_limits(limits)?;
        engine.preload_bundle_code(&self.source)?;
        App::from_loaded(engine)
    }
}

/// An app ready to handle requests: an engine with a bundle loaded, its
/// router and its app shell.
pub struct App {
    /// The engine running the bundle.
    pub engine: Engine<MemoryResourceResolver>,
    /// Routes discovered when the bundle was built.
    pub router: Router,
    /// The bundle's `app.html`, or the default shell.
    pub shell: AppShell,
}

impl App {
    /// Creates a fresh engine from bytecode produced by [`Bundle::compile`].
    pub fn from_bytecode(bytecode: &[u8]) -> Result<Self> {
        let engine = new_engine()?;
        engine.preload_bundle_code_from_binary(bytecode)?;
        Self::from_loaded(engine)
    }

    /// Like [`from_bytecode`](Self::from_bytecode), with `limits` in force
    /// before any of the bundle's code runs.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_bytecode_with_limits(bytecode: &[u8], limits: &crate::limits::EngineLimits) -> Result<Self> {
        let engine = new_engine()?;
        engine.set_limits(limits)?;
        engine.preload_bundle_code_from_binary(bytecode)?;
        Self::from_loaded(engine)
    }

    fn from_loaded(engine: Engine<MemoryResourceResolver>) -> Result<Self> {
        let globals = engine.lua().globals();
        let route_files: Vec<String> = match globals.get::<Option<Table>>("__route_files")? {
            Some(files) => files.sequence_values::<String>().collect::<mlua::Result<_>>()?,
            None => Vec::new(),
        };
        let shell = match globals.get::<Option<String>>("__app_html")? {
            Some(html) => AppShell::new(html),
            None => AppShell::default(),
        };
        let router = Router::from_paths(route_files.iter());
        Ok(Self {
            engine,
            router,
            shell,
        })
    }
}

fn new_engine() -> Result<Engine<MemoryResourceResolver>> {
    Engine::new(MemoryResourceResolver::new(), Box::new(MemoryCache::new(1000)))
}
