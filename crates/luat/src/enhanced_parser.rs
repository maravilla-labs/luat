// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::parser::{parse_template};
use crate::error::{LuatError, SourceContext, Result};
use crate::ast::TemplateAST;

/// Enhanced parser that includes source context in error messages
pub fn parse_template_with_context(source: &str, template_name: Option<&str>) -> Result<TemplateAST> {
    
    
    match parse_template(source) {
        Ok(mut ast) => {
            // Update the path if template_name is provided
            if let Some(name) = template_name {
                ast.path = Some(name.to_string());
            }
            Ok(ast)
        },
        Err(e) => {
            // If we already have a parse error, add source context to it
            match e {
                LuatError::ParseError { message, line, column, .. } => {
                    // Create source context
                    let source_context = SourceContext::from_source(source, line, column);
                    
                    // Return a more detailed error
                    Err(LuatError::ParseError {
                        message,
                        line,
                        column,
                        file: template_name.map(String::from),
                        source_context: Some(source_context),
                    })
                },
                _ => Err(e) // Pass through other errors unchanged
            }
        }
    }
}
