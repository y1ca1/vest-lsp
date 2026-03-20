mod parse;

use tree_sitter_language::LanguageFn;

pub use parse::{
    Parse, SemanticToken, SemanticTokenKind, SyntaxDiagnostic, parse, parse_incremental,
    parse_with_edits,
};

unsafe extern "C" {
    fn tree_sitter_vest() -> *const ();
}

pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_vest) };
pub const NODE_TYPES: &str = include_str!("node-types.json");
pub const HIGHLIGHTS_QUERY: &str = include_str!("../queries/highlights.scm");

pub fn language() -> tree_sitter::Language {
    LANGUAGE.into()
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use tree_sitter::{InputEdit, Point};

    use crate::{SemanticTokenKind, parse, parse_with_edits};

    #[test]
    fn grammar_loads() {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&crate::language())
            .expect("failed to load Vest grammar");
    }

    #[test]
    fn parses_valid_definition_without_errors() {
        let source = r#"
packet = {
    version: u8,
    length: u16,
}
"#;

        let parse = parse(source);
        assert!(!parse.root_node().has_error());
        assert!(parse.diagnostics().is_empty());
    }

    #[test]
    fn reports_syntax_errors_with_stable_ranges() {
        let source = "packet = {\n    field: u8\n";
        let parse = parse(source);
        let rendered = parse
            .diagnostics()
            .iter()
            .map(|diagnostic| {
                format!(
                    "{} @ {}..{}",
                    diagnostic.message, diagnostic.start_byte, diagnostic.end_byte
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        expect![[r#"
Unexpected end of file @ 24..24"#]]
        .assert_eq(&rendered);
    }

    #[test]
    fn exposes_semantic_tokens_from_highlight_query() {
        let source = "macro wrap_packet!(item) = wrap(u8 = 1, item)\n";
        let parse = parse(source);
        let rendered = parse
            .semantic_tokens()
            .iter()
            .map(|token| format!("{:?}@{}..{}", token.kind, token.start_byte, token.end_byte))
            .collect::<Vec<_>>()
            .join("\n");

        expect![[r#"
Keyword@0..5
Macro@6..17
Operator@17..18
Operator@25..26
Keyword@27..31
Type@32..34
Operator@35..36
Number@37..38
Function@40..44"#]]
        .assert_eq(&rendered);
    }

    #[test]
    fn semantic_token_kinds_are_specific() {
        let source = "choice(@tag: u8) = choose(@tag) { 1 => u16, }\n";
        let parse = parse(source);
        assert!(
            parse
                .semantic_tokens()
                .iter()
                .any(|token| token.kind == SemanticTokenKind::Parameter)
        );
        assert!(
            parse
                .semantic_tokens()
                .iter()
                .any(|token| token.kind == SemanticTokenKind::Number)
        );
    }

    #[test]
    fn incremental_reparse_matches_full_reparse() {
        let original = "packet = {\n    field: u8,\n}\n";
        let updated = "packet = {\n    field: u16,\n}\n";
        let initial = parse(original);

        let reparsed = parse_with_edits(
            updated,
            Some(&initial),
            &[InputEdit {
                start_byte: 22,
                old_end_byte: 24,
                new_end_byte: 25,
                start_position: Point::new(1, 11),
                old_end_position: Point::new(1, 13),
                new_end_position: Point::new(1, 14),
            }],
        );
        let fresh = parse(updated);

        assert_eq!(reparsed.root_node().to_sexp(), fresh.root_node().to_sexp());
        assert_eq!(reparsed.diagnostics(), fresh.diagnostics());
        assert_eq!(reparsed.semantic_tokens(), fresh.semantic_tokens());
    }
}
