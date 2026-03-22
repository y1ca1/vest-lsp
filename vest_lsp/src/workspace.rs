use std::collections::HashMap;

use lsp_types::{TextDocumentContentChangeEvent, TextDocumentItem, Url};
use thiserror::Error;
use tree_sitter::InputEdit;
use vest_db::{Database, Setter, SourceDatabase, SourceDocument, SourceError, SourceFile};
use vest_syntax::{Parse, parse, parse_with_edits};

struct DocumentState {
    parse: Parse,
    /// Salsa input for HIR and semantic analysis.
    file: SourceFile,
}

/// Authoritative hot-path state for the current LSP session.
///
/// This keeps editor-facing text state and the live incremental CST cache together,
/// while also maintaining Salsa inputs for semantic queries above the CST.
pub struct Workspace {
    sources: SourceDatabase,
    documents: HashMap<Url, DocumentState>,
    revision: u64,
    /// Salsa database for HIR and semantic analysis.
    db: Database,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            sources: SourceDatabase::new(),
            documents: HashMap::new(),
            revision: 0,
            db: Database::new(),
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn db(&self) -> &dyn vest_db::Db {
        &self.db
    }

    pub fn open_document(&mut self, document: TextDocumentItem) {
        let uri = document.uri;
        let version = document.version;
        let text = document.text;

        self.sources.open(uri.clone(), version, text.clone());

        let file = match self.documents.get(&uri).map(|state| state.file) {
            Some(file) => {
                file.set_version(&mut self.db).to(version);
                file.set_text(&mut self.db).to(text.clone());
                file
            }
            None => SourceFile::new(&self.db, uri.to_string(), version, text.clone()),
        };

        self.documents.insert(
            uri,
            DocumentState {
                parse: parse(&text),
                file,
            },
        );
        self.bump_revision();
    }

    pub fn apply_document_changes(
        &mut self,
        uri: &Url,
        version: i32,
        changes: &[TextDocumentContentChangeEvent],
    ) -> Result<(), WorkspaceError> {
        let previous_parse = self
            .documents
            .get(uri)
            .map(|state| state.parse.clone())
            .ok_or_else(|| WorkspaceError::DocumentNotOpen(uri.clone()))?;

        let edits = self.sources.apply_changes(uri, version, changes)?;
        let text = self
            .sources
            .document(uri)
            .ok_or_else(|| WorkspaceError::DocumentNotOpen(uri.clone()))?
            .text()
            .to_owned();

        let input_edits: Vec<InputEdit> = edits.into_iter().map(|edit| edit.input_edit).collect();
        let updated_parse = parse_with_edits(&text, Some(&previous_parse), &input_edits);

        if let Some(state) = self.documents.get_mut(uri) {
            state.parse = updated_parse;
            // Update Salsa input
            state.file.set_version(&mut self.db).to(version);
            state.file.set_text(&mut self.db).to(text);
        }

        self.bump_revision();
        Ok(())
    }

    pub fn close_document(&mut self, uri: &Url) -> bool {
        let document_removed = self.documents.remove(uri).is_some();
        let source_removed = self.sources.close(uri).is_some();
        let removed = document_removed || source_removed;
        if removed {
            self.bump_revision();
        }
        removed
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    pub fn document(&self, uri: &Url) -> Option<&SourceDocument> {
        self.sources.document(uri)
    }

    pub fn parse(&self, uri: &Url) -> Option<&Parse> {
        self.documents.get(uri).map(|state| &state.parse)
    }

    pub fn source_file(&self, uri: &Url) -> Option<SourceFile> {
        self.documents.get(uri).map(|state| state.file)
    }

    fn bump_revision(&mut self) {
        self.revision += 1;
    }
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("document is not open: {0}")]
    DocumentNotOpen(Url),
    #[error(transparent)]
    Source(#[from] SourceError),
}

#[cfg(test)]
mod tests {
    use lsp_types::{Position, Range};

    use super::*;

    fn uri(name: &str) -> Url {
        Url::parse(&format!("file:///tmp/{name}.vest")).unwrap()
    }

    fn document(uri: &Url, version: i32, text: &str) -> TextDocumentItem {
        TextDocumentItem {
            uri: uri.clone(),
            language_id: "Vest".into(),
            version,
            text: text.into(),
        }
    }

    #[test]
    fn open_change_and_close_document_updates_workspace_state() {
        let uri = uri("packet");
        let mut workspace = Workspace::new();

        workspace.open_document(document(&uri, 1, "packet = {\n    field: u8,\n}\n"));
        assert_eq!(workspace.revision(), 1);
        assert_eq!(
            workspace.document(&uri).map(SourceDocument::text),
            Some("packet = {\n    field: u8,\n}\n")
        );

        workspace
            .apply_document_changes(
                &uri,
                2,
                &[TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(1, 11), Position::new(1, 13))),
                    range_length: None,
                    text: "u16".into(),
                }],
            )
            .unwrap();

        assert_eq!(workspace.revision(), 2);
        assert_eq!(
            workspace.document(&uri).map(SourceDocument::text),
            Some("packet = {\n    field: u16,\n}\n")
        );
        assert!(
            workspace
                .parse(&uri)
                .unwrap()
                .semantic_tokens()
                .iter()
                .any(|token| {
                    token.kind == vest_syntax::SemanticTokenKind::Type
                        && &workspace.document(&uri).unwrap().text()
                            [token.start_byte..token.end_byte]
                            == "u16"
                })
        );

        assert!(workspace.close_document(&uri));
        assert_eq!(workspace.revision(), 3);
        assert!(!workspace.contains(&uri));
    }

    #[test]
    fn incremental_parse_tracks_incremental_updates() {
        let uri = uri("broken");
        let mut workspace = Workspace::new();

        workspace.open_document(document(&uri, 1, "packet = {\n    field: u8,\n}\n"));
        let parse1 = workspace.parse(&uri).unwrap();
        assert!(parse1.diagnostics().is_empty());

        workspace
            .apply_document_changes(
                &uri,
                2,
                &[TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(1, 11), Position::new(1, 13))),
                    range_length: None,
                    text: "u16".into(),
                }],
            )
            .unwrap();

        let parse2 = workspace.parse(&uri).unwrap();
        assert!(!parse2.root_node().has_error());
        assert!(parse2.diagnostics().is_empty());
        assert!(parse2.semantic_tokens().iter().any(|token| {
            token.kind == vest_syntax::SemanticTokenKind::Type
                && &workspace.document(&uri).unwrap().text()[token.start_byte..token.end_byte]
                    == "u16"
        }));

        workspace
            .apply_document_changes(
                &uri,
                3,
                &[TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(2, 0), Position::new(2, 1))),
                    range_length: None,
                    text: "".into(),
                }],
            )
            .unwrap();

        let parse3 = workspace.parse(&uri).unwrap();
        assert!(parse3.root_node().has_error());
        assert!(!parse3.diagnostics().is_empty());
    }

    #[test]
    fn full_document_replacement_refreshes_the_live_parse() {
        let uri = uri("replace");
        let mut workspace = Workspace::new();

        workspace.open_document(document(&uri, 1, "packet = { field: u8, }\n"));

        workspace
            .apply_document_changes(
                &uri,
                2,
                &[TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "packet = {\n    field: u16,\n}\n".into(),
                }],
            )
            .unwrap();

        let parse = workspace.parse(&uri).unwrap();
        assert!(!parse.root_node().has_error());
        assert!(parse.semantic_tokens().iter().any(|token| {
            token.kind == vest_syntax::SemanticTokenKind::Type
                && &workspace.document(&uri).unwrap().text()[token.start_byte..token.end_byte]
                    == "u16"
        }));
    }

    #[test]
    fn changing_a_closed_document_returns_an_error() {
        let workspace = &mut Workspace::new();
        let uri = uri("missing");

        let err = workspace
            .apply_document_changes(
                &uri,
                1,
                &[TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "packet = {}\n".into(),
                }],
            )
            .unwrap_err();

        assert_eq!(err.to_string(), format!("document is not open: {uri}"));
    }
}
