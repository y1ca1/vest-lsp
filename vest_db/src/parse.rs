//! Staged Salsa query layer for syntax-derived metadata.
//!
//! The live LSP loop currently keeps its incremental Tree-sitter state outside Salsa.
//! These queries are kept for the milestone-3 analysis boundary and later HIR work.

use crate::SourceFile;

/// Cached syntax metadata for a source file.
#[salsa::tracked]
pub struct ParseSummary<'db> {
    /// The source file this was parsed from.
    pub source: SourceFile,

    /// Syntax diagnostics (errors found during parsing).
    #[returns(ref)]
    pub diagnostics: Vec<vest_syntax::SyntaxDiagnostic>,

    /// Semantic tokens derived from the parse tree.
    #[returns(ref)]
    pub semantic_tokens: Vec<vest_syntax::SemanticToken>,

    /// Whether the parse tree has any errors.
    pub has_errors: bool,
}

impl<'db> ParseSummary<'db> {
    /// Get the syntax tree for this summary.
    /// Tree-sitter trees stay outside Salsa because they contain internal pointers.
    /// Higher layers keep transient incremental trees when they need CST access.
    pub fn tree(&self, db: &'db dyn crate::Db) -> tree_sitter::Tree {
        let text = self.source(db).text(db);
        vest_syntax::parse(text).tree().clone()
    }
}
/// Parse a source file into cached syntax metadata.
#[salsa::tracked]
pub fn parse_file<'db>(db: &'db dyn crate::Db, source: SourceFile) -> ParseSummary<'db> {
    let text = source.text(db);
    let parse = vest_syntax::parse(text);
    let diagnostics = parse.diagnostics().to_vec();
    let semantic_tokens = parse.semantic_tokens().to_vec();
    let has_errors = parse.root_node().has_error();

    ParseSummary::new(db, source, diagnostics, semantic_tokens, has_errors)
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use salsa::Setter;

    use crate::{Database, SourceFile, parse_file};

    #[test]
    fn parse_valid_source_has_no_errors() {
        let db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = { field: u8, }\n".into(),
        );

        let result = parse_file(&db, source);
        assert!(!result.has_errors(&db));
        assert!(result.diagnostics(&db).is_empty());
    }

    #[test]
    fn parse_invalid_source_has_errors() {
        let db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = {\n    field: u8\n".into(),
        );

        let result = parse_file(&db, source);
        assert!(result.has_errors(&db));

        let rendered = result
            .diagnostics(&db)
            .iter()
            .map(|d| format!("{} @ {}..{}", d.message, d.start_byte, d.end_byte))
            .collect::<Vec<_>>()
            .join("\n");

        expect![[r#"Unexpected end of file @ 24..24"#]].assert_eq(&rendered);
    }

    #[test]
    fn parse_summary_is_memoized() {
        let db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = { field: u8, }\n".into(),
        );

        // Call parse_file twice - should return the cached summary
        let result1 = parse_file(&db, source);
        let result2 = parse_file(&db, source);

        // Results should be equal (same cached summary)
        assert_eq!(result1.has_errors(&db), result2.has_errors(&db));
        assert_eq!(result1.diagnostics(&db), result2.diagnostics(&db));
    }

    #[test]
    fn parse_summary_is_invalidated_on_text_change() {
        let mut db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = { field: u8, }\n".into(),
        );

        let result1 = parse_file(&db, source);
        assert!(!result1.has_errors(&db));

        // Update the source to have an error
        source
            .set_text(&mut db)
            .to("packet = {\n    field: u8\n".into());

        let result2 = parse_file(&db, source);
        assert!(result2.has_errors(&db));
    }

    #[test]
    fn semantic_tokens_available() {
        let db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = { field: u8, }\n".into(),
        );

        let result = parse_file(&db, source);
        let tokens = result.semantic_tokens(&db);
        assert!(!tokens.is_empty());
    }

    #[test]
    fn version_changes_do_not_change_parse_output() {
        let mut db = Database::new();
        let source = SourceFile::new(
            &db,
            "file:///test.vest".into(),
            1,
            "packet = { field: u8, }\n".into(),
        );

        let result1 = parse_file(&db, source);
        let diagnostics1 = result1.diagnostics(&db).clone();
        let tokens1 = result1.semantic_tokens(&db).clone();
        let has_errors1 = result1.has_errors(&db);
        source.set_version(&mut db).to(2);
        let result2 = parse_file(&db, source);

        assert_eq!(diagnostics1.as_slice(), result2.diagnostics(&db).as_slice());
        assert_eq!(tokens1.as_slice(), result2.semantic_tokens(&db).as_slice());
        assert_eq!(has_errors1, result2.has_errors(&db));
    }
}
