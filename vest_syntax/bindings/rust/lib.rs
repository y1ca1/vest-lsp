//! Rust bindings for tree-sitter-vest
//!
//! This crate provides a Tree-sitter grammar for the Vest DSL.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_vest() -> *const ();
}

/// Returns the tree-sitter Language for Vest.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_vest) };

/// The tree-sitter node types as JSON.
pub const NODE_TYPES: &str = include_str!("../../src/node-types.json");

/// The source of the syntax highlighting query.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_load_grammar() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Failed to load Vest grammar");
    }

    #[test]
    fn test_parse_simple_definition() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Failed to load Vest grammar");

        let source = r#"
            foo = {
                a: u8,
                b: u16,
            }
        "#;

        let tree = parser.parse(source, None).expect("Failed to parse");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_parse_enum() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Failed to load Vest grammar");

        let source = r#"
            my_enum = enum {
                A = 0,
                B = 1,
                C = 2,
            }
        "#;

        let tree = parser.parse(source, None).expect("Failed to parse");
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_parse_choice() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&LANGUAGE.into())
            .expect("Failed to load Vest grammar");

        let source = r#"
            my_choice(@tag: u8) = choose(@tag) {
                0 => u8,
                1 => u16,
                _ => u32,
            }
        "#;

        let tree = parser.parse(source, None).expect("Failed to parse");
        assert!(!tree.root_node().has_error());
    }
}
