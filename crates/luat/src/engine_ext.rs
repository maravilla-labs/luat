// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::enhanced_parser::parse_template_with_context;
use crate::engine::Engine;
use crate::error::Result;
use crate::resolver::ResourceResolver;
use crate::cache::SharedPtr;
use crate::Module;
use crate::transform::{transform_ast, validate_ir};
use crate::codegen::generate_lua_code_with_sourcemap;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

impl<R: ResourceResolver> Engine<R> {
    /// Compile a template from a string with enhanced error reporting
    pub fn compile_template_string(&self, name: &str, source: &str) -> Result<SharedPtr<Module>> {
        self.compile_template_string_with_path(name, source, None)
    }

    /// Compile a template from a string with enhanced error reporting and path tracking
    pub fn compile_template_string_with_path(
        &self,
        name: &str,
        source: &str,
        path: Option<String>,
    ) -> Result<SharedPtr<Module>> {
        // Parse template using enhanced parser
        let ast = parse_template_with_context(source, Some(name))?;

        // Transform to IR
        let ir = transform_ast(ast)?;
        validate_ir(&ir)?;

        // Generate Lua code with source map for error line translation
        let (lua_code, source_map) = generate_lua_code_with_sourcemap(ir, name)?;

        // Create the module with source map for error translation
        #[cfg(not(target_arch = "wasm32"))]
        let module = Arc::new(Module::with_source_map(
            name.to_string(),
            lua_code,
            Vec::new(),
            path,
            source_map,
        ));

        #[cfg(target_arch = "wasm32")]
        let module = Rc::new(Module::with_source_map(
            name.to_string(),
            lua_code,
            Vec::new(),
            path,
            source_map,
        ));

        Ok(module)
    }
}
