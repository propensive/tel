//! Rust bindings for tree-sitter-tel.

use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_tel() -> *const ();
}

/// The tree-sitter [`LanguageFn`] for TEL.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_tel) };

/// The content of [`node-types.json`](https://tree-sitter.github.io/tree-sitter/using-parsers#static-node-types).
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// Highlight queries for TEL.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Injection queries for TEL.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");

#[cfg(test)]
mod tests {
    #[test]
    fn loads_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&super::LANGUAGE.into())
            .expect("Error loading TEL grammar");
    }
}
