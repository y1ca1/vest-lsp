use std::collections::HashMap;
use std::ops::ControlFlow;

use async_lsp::client_monitor::ClientProcessMonitorLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, ErrorCode, LanguageClient, MainLoop, ResponseError};
use lsp_types::notification;
use lsp_types::request;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, Location,
    LogMessageParams, MarkupContent, MarkupKind, MessageType, PrepareRenameResponse,
    PublishDiagnosticsParams, ReferenceParams, RenameOptions, RenameParams, SemanticToken,
    SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, TextDocumentItem,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextEdit, Url, WorkspaceEdit,
};
use tower::ServiceBuilder;
use tracing::{Level, warn};
use vest_db::{
    SourceDocument, Span, is_valid_identifier_text, lower_to_hir_with_parse,
    references_for_symbol_in_hir, symbol_at_offset_in_hir,
};
use vest_syntax::{SemanticToken as SyntaxToken, SemanticTokenKind};

use crate::workspace::{Workspace, WorkspaceError};

pub struct VestServer {
    client: ClientSocket,
    workspace: Workspace,
}

impl VestServer {
    pub fn new(client: ClientSocket) -> Self {
        Self {
            client,
            workspace: Workspace::new(),
        }
    }

    fn log_message(&self, typ: MessageType, message: impl Into<String>) {
        let mut client = self.client.clone();
        let _ = client.log_message(LogMessageParams {
            typ,
            message: message.into(),
        });
    }

    pub fn initialize_result(&self) -> InitializeResult {
        InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save: None,
                        will_save_wait_until: None,
                        save: None,
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(lsp_types::OneOf::Left(true)),
                references_provider: Some(lsp_types::OneOf::Left(true)),
                rename_provider: Some(lsp_types::OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["=".into(), "@".into(), "|".into()]),
                    ..CompletionOptions::default()
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_token_legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            work_done_progress_options: Default::default(),
                        },
                    ),
                ),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "vest_lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        }
    }

    pub fn initialized(&self) {
        self.log_message(MessageType::INFO, "vest_lsp initialized");
    }

    pub fn open_document(&mut self, document: TextDocumentItem) -> Vec<Diagnostic> {
        let uri = document.uri.clone();
        self.workspace.open_document(document);
        self.diagnostics_for(&uri)
    }

    pub fn change_document(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> Result<Vec<Diagnostic>, WorkspaceError> {
        let uri = params.text_document.uri;
        self.workspace.apply_document_changes(
            &uri,
            params.text_document.version,
            &params.content_changes,
        )?;
        Ok(self.diagnostics_for(&uri))
    }

    pub fn close_document(&mut self, params: DidCloseTextDocumentParams) {
        self.workspace.close_document(&params.text_document.uri);
    }

    pub fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let document = self.workspace.document(&uri)?;
        let parse = self.workspace.parse(&uri)?;
        let byte_offset = document.position_to_byte_offset(position).ok()?;
        let node = parse.node_at_byte(byte_offset)?;
        let snippet = document
            .text()
            .get(node.byte_range())
            .unwrap_or("")
            .trim()
            .to_string();

        let detail = if snippet.is_empty() {
            format!("`{}`", node.kind())
        } else {
            format!("```vest\n{snippet}\n```\n\n`{}`", node.kind())
        };

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: detail,
            }),
            range: document
                .byte_range_to_lsp_range(node.start_byte(), node.end_byte())
                .ok(),
        })
    }

    pub fn completion(&self, _params: CompletionParams) -> CompletionResponse {
        CompletionResponse::Array(completion_items())
    }

    pub fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let document = self.workspace.document(&uri)?;
        let source_file = self.workspace.source_file(&uri)?;
        let parse = self.workspace.parse(&uri)?;
        let db = self.workspace.db();
        let byte_offset = document.position_to_byte_offset(position).ok()?;
        let hir = lower_to_hir_with_parse(db, source_file, parse);
        let symbol = symbol_at_offset_in_hir(&hir, byte_offset)?;
        location_for_span(&uri, document, symbol.declaration_span())
            .map(GotoDefinitionResponse::Scalar)
    }

    pub fn references(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let document = self.workspace.document(&uri)?;
        let source_file = self.workspace.source_file(&uri)?;
        let parse = self.workspace.parse(&uri)?;
        let db = self.workspace.db();
        let byte_offset = document.position_to_byte_offset(position).ok()?;
        let hir = lower_to_hir_with_parse(db, source_file, parse);
        let symbol = symbol_at_offset_in_hir(&hir, byte_offset)?;

        Some(
            references_for_symbol_in_hir(&hir, symbol, params.context.include_declaration)
                .into_iter()
                .filter_map(|occurrence| location_for_span(&uri, document, occurrence.span))
                .collect(),
        )
    }

    pub fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let uri = params.text_document.uri;
        let position = params.position;

        let document = self.workspace.document(&uri)?;
        let source_file = self.workspace.source_file(&uri)?;
        let parse = self.workspace.parse(&uri)?;
        let db = self.workspace.db();
        let byte_offset = document.position_to_byte_offset(position).ok()?;
        let hir = lower_to_hir_with_parse(db, source_file, parse);
        let symbol = symbol_at_offset_in_hir(&hir, byte_offset)?;
        let occurrence = references_for_symbol_in_hir(&hir, symbol, true)
            .into_iter()
            .find(|occurrence| occurrence.span.contains(byte_offset))?;
        let prepare_span = symbol.prepare_rename_span(occurrence.span);
        let range = document
            .byte_range_to_lsp_range(prepare_span.start_byte, prepare_span.end_byte)
            .ok()?;

        Some(PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder: symbol.name().as_str(db).to_string(),
        })
    }

    pub fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>, ResponseError> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let document = self
            .workspace
            .document(&uri)
            .ok_or_else(|| ResponseError::new(ErrorCode::REQUEST_FAILED, "document is not open"))?;
        let source_file = self.workspace.source_file(&uri).ok_or_else(|| {
            ResponseError::new(
                ErrorCode::REQUEST_FAILED,
                "document source is not available",
            )
        })?;
        let parse = self.workspace.parse(&uri).ok_or_else(|| {
            ResponseError::new(ErrorCode::REQUEST_FAILED, "document parse is not available")
        })?;
        let db = self.workspace.db();
        let byte_offset = document.position_to_byte_offset(position).map_err(|_| {
            ResponseError::new(
                ErrorCode::INVALID_PARAMS,
                "position does not resolve to a symbol",
            )
        })?;
        let hir = lower_to_hir_with_parse(db, source_file, parse);
        let symbol = symbol_at_offset_in_hir(&hir, byte_offset).ok_or_else(|| {
            self.log_message(
                MessageType::WARNING,
                format!(
                    "rename rejected: no symbol at {}:{}",
                    position.line + 1,
                    position.character + 1
                ),
            );
            ResponseError::new(ErrorCode::REQUEST_FAILED, "cannot rename this location")
        })?;
        let new_name = symbol.normalize_rename_input(&params.new_name);
        if !is_valid_identifier_text(new_name) {
            self.log_message(
                MessageType::WARNING,
                format!("rename rejected: invalid identifier `{}`", params.new_name),
            );
            return Err(ResponseError::new(
                ErrorCode::INVALID_PARAMS,
                "new name must be a valid Vest identifier",
            ));
        }

        let edits = references_for_symbol_in_hir(&hir, symbol, true)
            .into_iter()
            .filter_map(|occurrence| {
                Some(TextEdit {
                    range: document
                        .byte_range_to_lsp_range(
                            occurrence.span.start_byte,
                            occurrence.span.end_byte,
                        )
                        .ok()?,
                    new_text: symbol.rename_text(new_name),
                })
            })
            .collect::<Vec<_>>();

        let mut changes = HashMap::new();
        changes.insert(uri, edits);
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    pub fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Option<SemanticTokensResult> {
        let uri = params.text_document.uri;
        let document = self.workspace.document(&uri)?;
        let parse = self.workspace.parse(&uri)?;
        let data = encode_semantic_tokens(parse.semantic_tokens(), document)?;

        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        }))
    }

    pub fn diagnostics_for(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(document) = self.workspace.document(uri) else {
            return Vec::new();
        };
        let Some(parse) = self.workspace.parse(uri) else {
            return Vec::new();
        };

        parse
            .diagnostics()
            .iter()
            .filter_map(|diagnostic| {
                Some(Diagnostic {
                    range: document
                        .byte_range_to_lsp_range(diagnostic.start_byte, diagnostic.end_byte)
                        .ok()?,
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("vest_lsp".into()),
                    message: diagnostic.message.clone(),
                    ..Diagnostic::default()
                })
            })
            .collect()
    }

    pub fn publish_diagnostics(
        &self,
        uri: Url,
        diagnostics: Vec<Diagnostic>,
        version: Option<i32>,
    ) {
        if let Err(err) =
            self.client
                .notify::<notification::PublishDiagnostics>(PublishDiagnosticsParams {
                    uri,
                    diagnostics,
                    version,
                })
        {
            warn!("failed to publish diagnostics: {err}");
        }
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.workspace.contains(uri)
    }
}

pub async fn run_stdio_server() -> async_lsp::Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();

    let (server, _) = MainLoop::new_server(|client| {
        let mut router = Router::new(VestServer::new(client.clone()));
        router
            .request::<request::Initialize, _>(|state, _params: InitializeParams| {
                let result = state.initialize_result();
                async move { Ok(result) }
            })
            .request::<request::Shutdown, _>(|_, ()| async move { Ok(()) })
            .request::<request::HoverRequest, _>(|state, params| {
                let hover = state.hover(params);
                async move { Ok(hover) }
            })
            .request::<request::Completion, _>(|state, params| {
                let completion = state.completion(params);
                async move { Ok(Some(completion)) }
            })
            .request::<request::SemanticTokensFullRequest, _>(|state, params| {
                let tokens = state.semantic_tokens_full(params);
                async move { Ok(tokens) }
            })
            .request::<request::GotoDefinition, _>(|state, params| {
                let definition = state.goto_definition(params);
                async move { Ok(definition) }
            })
            .request::<request::References, _>(|state, params| {
                let references = state.references(params);
                async move { Ok(references) }
            })
            .request::<request::PrepareRenameRequest, _>(|state, params| {
                let response = state.prepare_rename(params);
                async move { Ok(response) }
            })
            .request::<request::Rename, _>(|state, params| {
                let rename = state.rename(params);
                async move { rename }
            })
            .notification::<notification::Initialized>(|state, _| {
                state.initialized();
                ControlFlow::Continue(())
            })
            .notification::<notification::DidOpenTextDocument>(
                |state, params: DidOpenTextDocumentParams| {
                    let diagnostics = state.open_document(params.text_document.clone());
                    state.publish_diagnostics(
                        params.text_document.uri,
                        diagnostics,
                        Some(params.text_document.version),
                    );
                    ControlFlow::Continue(())
                },
            )
            .notification::<notification::DidChangeTextDocument>(|state, params| {
                match state.change_document(params.clone()) {
                    Ok(diagnostics) => state.publish_diagnostics(
                        params.text_document.uri,
                        diagnostics,
                        Some(params.text_document.version),
                    ),
                    Err(err) => {
                        state.log_message(
                            MessageType::ERROR,
                            format!("failed to handle document change: {err}"),
                        );
                        warn!("failed to handle document change: {err}");
                    }
                }
                ControlFlow::Continue(())
            })
            .notification::<notification::DidChangeConfiguration>(|_, _| ControlFlow::Continue(()))
            .notification::<notification::DidCloseTextDocument>(|state, params| {
                let uri = params.text_document.uri.clone();
                state.close_document(params);
                state.publish_diagnostics(uri, Vec::new(), None);
                ControlFlow::Continue(())
            });

        ServiceBuilder::new()
            .layer(TracingLayer::default())
            .layer(LifecycleLayer::default())
            .layer(CatchUnwindLayer::default())
            .layer(ClientProcessMonitorLayer::new(client))
            .service(router)
    });

    #[cfg(unix)]
    let (stdin, stdout) = (
        async_lsp::stdio::PipeStdin::lock_tokio().expect("failed to lock stdin"),
        async_lsp::stdio::PipeStdout::lock_tokio().expect("failed to lock stdout"),
    );

    #[cfg(not(unix))]
    let (stdin, stdout) = (
        tokio_util::compat::TokioAsyncReadCompatExt::compat(tokio::io::stdin()),
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tokio::io::stdout()),
    );

    server.run_buffered(stdin, stdout).await
}

fn completion_items() -> Vec<CompletionItem> {
    keyword_completion("const")
        .into_iter()
        .chain(keyword_completion("enum"))
        .chain(keyword_completion("choose"))
        .chain(keyword_completion("wrap"))
        .chain(keyword_completion("public"))
        .chain(keyword_completion("secret"))
        .chain(keyword_completion("Vec"))
        .chain(keyword_completion("Option"))
        .chain(keyword_completion("Tail"))
        .chain(type_completion("u8"))
        .chain(type_completion("u16"))
        .chain(type_completion("u24"))
        .chain(type_completion("u32"))
        .chain(type_completion("u64"))
        .chain(type_completion("i8"))
        .chain(type_completion("i16"))
        .chain(type_completion("i24"))
        .chain(type_completion("i32"))
        .chain(type_completion("i64"))
        .chain(type_completion("btc_varint"))
        .chain(type_completion("uleb128"))
        .collect()
}

fn keyword_completion(label: &str) -> Option<CompletionItem> {
    Some(CompletionItem {
        label: label.into(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..CompletionItem::default()
    })
}

fn type_completion(label: &str) -> Option<CompletionItem> {
    Some(CompletionItem {
        label: label.into(),
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        ..CompletionItem::default()
    })
}

fn semantic_token_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::MODIFIER,
            SemanticTokenType::TYPE,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::PROPERTY,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::ENUM_MEMBER,
            SemanticTokenType::NUMBER,
            SemanticTokenType::STRING,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::COMMENT,
        ],
        token_modifiers: vec![SemanticTokenModifier::READONLY],
    }
}

fn semantic_token_index(kind: SemanticTokenKind) -> u32 {
    match kind {
        SemanticTokenKind::Keyword => 0,
        SemanticTokenKind::Modifier => 1,
        SemanticTokenKind::Type => 2,
        SemanticTokenKind::Function => 3,
        SemanticTokenKind::Property => 4,
        SemanticTokenKind::Constant | SemanticTokenKind::Variable => 5,
        SemanticTokenKind::Parameter => 6,
        SemanticTokenKind::EnumMember => 7,
        SemanticTokenKind::Number => 8,
        SemanticTokenKind::String => 9,
        SemanticTokenKind::Operator => 10,
        SemanticTokenKind::Comment => 11,
    }
}

fn semantic_token_modifiers(kind: SemanticTokenKind) -> u32 {
    match kind {
        SemanticTokenKind::Constant => 1,
        _ => 0,
    }
}

fn encode_semantic_tokens(
    tokens: &[SyntaxToken],
    document: &SourceDocument,
) -> Option<Vec<SemanticToken>> {
    let mut data = Vec::new();
    let mut last_line = 0;
    let mut last_start = 0;

    for token in tokens {
        let range = document
            .byte_range_to_lsp_range(token.start_byte, token.end_byte)
            .ok()?;
        if range.start.line != range.end.line {
            continue;
        }

        let delta_line = range.start.line - last_line;
        let delta_start = if delta_line == 0 {
            range.start.character - last_start
        } else {
            range.start.character
        };
        let length = range.end.character - range.start.character;
        if length == 0 {
            continue;
        }

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: semantic_token_index(token.kind),
            token_modifiers_bitset: semantic_token_modifiers(token.kind),
        });

        last_line = range.start.line;
        last_start = range.start.character;
    }

    Some(data)
}

fn location_for_span(uri: &Url, document: &SourceDocument, span: Span) -> Option<Location> {
    let range = document
        .byte_range_to_lsp_range(span.start_byte, span.end_byte)
        .ok()?;
    Some(Location {
        uri: uri.clone(),
        range,
    })
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use lsp_types::{
        Position, Range, ReferenceContext, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    };

    use super::*;

    fn uri(name: &str) -> Url {
        Url::parse(&format!("file:///tmp/{name}.vest")).unwrap()
    }

    fn server() -> VestServer {
        VestServer::new(ClientSocket::new_closed())
    }

    fn open_document(
        server: &mut VestServer,
        uri: &Url,
        version: i32,
        text: &str,
    ) -> Vec<Diagnostic> {
        server.open_document(TextDocumentItem {
            uri: uri.clone(),
            language_id: "Vest".into(),
            version,
            text: text.into(),
        })
    }

    fn render_locations(locations: &[Location]) -> String {
        locations
            .iter()
            .map(|location| {
                format!(
                    "{}:{}-{}:{}",
                    location.range.start.line,
                    location.range.start.character,
                    location.range.end.line,
                    location.range.end.character
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_workspace_edit(edit: &WorkspaceEdit) -> String {
        let mut rendered = Vec::new();
        let Some(changes) = &edit.changes else {
            return String::new();
        };
        for edits in changes.values() {
            let mut edits = edits.clone();
            edits.sort_by_key(|edit| {
                (
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.line,
                    edit.range.end.character,
                )
            });
            rendered.extend(edits.into_iter().map(|edit| {
                format!(
                    "{}:{}-{}:{} => {}",
                    edit.range.start.line,
                    edit.range.start.character,
                    edit.range.end.line,
                    edit.range.end.character,
                    edit.new_text
                )
            }));
        }
        rendered.join("\n")
    }

    #[test]
    fn initialize_advertises_incremental_sync_and_semantic_tokens() {
        let capabilities = server().initialize_result().capabilities;
        assert!(matches!(
            capabilities.text_document_sync,
            Some(TextDocumentSyncCapability::Options(
                TextDocumentSyncOptions {
                    change: Some(TextDocumentSyncKind::INCREMENTAL),
                    open_close: Some(true),
                    ..
                }
            ))
        ));
        assert!(capabilities.semantic_tokens_provider.is_some());
        assert!(capabilities.hover_provider.is_some());
        assert!(capabilities.references_provider.is_some());
        assert!(matches!(
            capabilities.rename_provider,
            Some(lsp_types::OneOf::Right(RenameOptions {
                prepare_provider: Some(true),
                ..
            }))
        ));
    }

    #[test]
    fn open_change_and_hover_work_against_server_state() {
        let uri = uri("packet");
        let mut server = server();
        let diagnostics = open_document(&mut server, &uri, 1, "packet = {\n    field: u8,\n}\n");

        assert!(diagnostics.is_empty());

        let hover = server
            .hover(HoverParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 6),
                },
                work_done_progress_params: Default::default(),
            })
            .expect("hover should exist");

        let HoverContents::Markup(contents) = hover.contents else {
            panic!("expected markdown hover");
        };
        assert!(contents.value.contains("field"));

        let diagnostics = server
            .change_document(DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version: 2 },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(1, 11), Position::new(1, 13))),
                    range_length: None,
                    text: "u16".into(),
                }],
            })
            .unwrap();
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn syntax_errors_flow_into_lsp_diagnostics_and_semantic_tokens() {
        let uri = uri("broken");
        let mut server = server();
        let diagnostics = open_document(&mut server, &uri, 1, "packet = {\n    field: u8\n");

        let rendered = diagnostics
            .iter()
            .map(|diagnostic| {
                format!(
                    "{} @ {}:{}-{}:{}",
                    diagnostic.message,
                    diagnostic.range.start.line,
                    diagnostic.range.start.character,
                    diagnostic.range.end.line,
                    diagnostic.range.end.character
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        expect![[r#"
Unexpected end of file @ 1:13-1:13"#]]
        .assert_eq(&rendered);

        let tokens = server
            .semantic_tokens_full(SemanticTokensParams {
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                text_document: TextDocumentIdentifier { uri },
            })
            .expect("tokens should exist");

        let SemanticTokensResult::Tokens(tokens) = tokens else {
            panic!("expected full semantic tokens");
        };

        assert!(!tokens.data.is_empty());
    }

    #[test]
    fn close_document_removes_state() {
        let uri = uri("close");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = {}\n");

        assert!(server.contains(&uri));

        server.close_document(lsp_types::DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        });

        assert!(!server.contains(&uri));
    }

    #[test]
    fn changing_a_missing_document_returns_a_workspace_error() {
        let uri = uri("missing");
        let mut server = server();

        let err = server
            .change_document(DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier { uri, version: 1 },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "packet = {}\n".into(),
                }],
            })
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "document is not open: file:///tmp/missing.vest"
        );
    }

    #[test]
    fn goto_definition_resolves_type_reference() {
        let uri = uri("goto");
        let mut server = server();
        // "other" is defined on line 0 (byte 0-12)
        // "packet" references "other" at line 1, column 15
        open_document(
            &mut server,
            &uri,
            1,
            "other = u8\npacket = { field: other, }\n",
        );

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 18), // cursor on "other" reference
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        // Definition starts at line 0, column 0
        assert_eq!(location.range.start.line, 0);
        assert_eq!(location.range.start.character, 0);
    }

    #[test]
    fn goto_definition_resolves_dependent_parameter_reference() {
        let uri = uri("goto_local");
        let mut server = server();
        open_document(&mut server, &uri, 1, "msg(@len: u16) = [u8; @len]\n");

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(0, 23),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 0);
        assert_eq!(location.range.start.character, 4);
    }

    #[test]
    fn goto_definition_resolves_dotted_dependent_reference() {
        let uri = uri("goto_dotted");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg = { @len: u16, data: [u8; @len.value], }\n",
        );

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(0, 31),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 0);
        assert_eq!(location.range.start.character, 8);
    }

    #[test]
    fn goto_definition_resolves_enum_definition_reference() {
        let uri = uri("goto_enum");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "my_enum = enum { A = 0, }\npacket = my_enum\n",
        );

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 10),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 0);
        assert_eq!(location.range.start.character, 0);
    }

    #[test]
    fn goto_definition_on_top_level_declaration_prefers_definition_name() {
        let uri = uri("goto_decl");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = {\n    packet: u8,\n}\n");

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(0, 1),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 0);
        assert_eq!(location.range.start.character, 0);
    }

    #[test]
    fn goto_definition_on_field_declaration_prefers_field_over_shadowed_param() {
        let uri = uri("goto_field_decl");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg(@len: u16) = {\n    @len: u8,\n    data: [u8; @len],\n}\n",
        );

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 5),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 1);
        assert_eq!(location.range.start.character, 4);
    }

    #[test]
    fn goto_definition_on_shadowed_field_reference_prefers_field() {
        let uri = uri("goto_shadowed_ref");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg(@len: u16) = {\n    @len: u8,\n    data: [u8; @len],\n}\n",
        );

        let definition = server
            .goto_definition(GotoDefinitionParams {
                text_document_position_params: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(2, 16),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .expect("definition should exist");

        let GotoDefinitionResponse::Scalar(location) = definition else {
            panic!("expected scalar location");
        };

        assert_eq!(location.uri, uri);
        assert_eq!(location.range.start.line, 1);
        assert_eq!(location.range.start.character, 4);
    }

    #[test]
    fn references_return_top_level_locations_with_optional_declaration() {
        let uri = uri("refs_top_level");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "other = u8\npacket = { field: other, next: other, }\n",
        );

        let with_declaration = server
            .references(ReferenceParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 20),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .expect("references should exist");
        let without_declaration = server
            .references(ReferenceParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(1, 20),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: false,
                },
            })
            .expect("references should exist");

        assert_eq!(
            render_locations(&with_declaration),
            "0:0-0:5\n1:18-1:23\n1:31-1:36"
        );
        assert_eq!(
            render_locations(&without_declaration),
            "1:18-1:23\n1:31-1:36"
        );
    }

    #[test]
    fn references_return_shadowed_local_locations() {
        let uri = uri("refs_local");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg(@len: u16) = {\n    @len: u8,\n    data: [u8; @len],\n    rest: [u8; @len],\n}\n",
        );

        let references = server
            .references(ReferenceParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(2, 16),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            })
            .expect("references should exist");

        assert_eq!(
            render_locations(&references),
            "1:4-1:8\n2:15-2:19\n3:15-3:19"
        );
    }

    #[test]
    fn rename_returns_edits_for_top_level_symbol() {
        let uri = uri("rename_top_level");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "other = u8\npacket = { field: other, next: other, }\n",
        );

        let edit = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(1, 20),
                },
                new_name: "size".into(),
                work_done_progress_params: Default::default(),
            })
            .expect("rename should succeed")
            .expect("workspace edit should exist");

        assert_eq!(
            render_workspace_edit(&edit),
            "0:0-0:5 => size\n1:18-1:23 => size\n1:31-1:36 => size"
        );
    }

    #[test]
    fn prepare_rename_returns_reference_range_and_placeholder() {
        let uri = uri("prepare_rename_top_level");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "other = u8\npacket = { field: other, next: other, }\n",
        );

        let response = server
            .prepare_rename(lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(1, 20),
            })
            .expect("prepare rename should exist");

        let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
            panic!("expected range with placeholder");
        };

        assert_eq!(placeholder, "other");
        assert_eq!(range.start, Position::new(1, 18));
        assert_eq!(range.end, Position::new(1, 23));
    }

    #[test]
    fn prepare_rename_returns_dotted_dependent_base_range() {
        let uri = uri("prepare_rename_dotted");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg = {\n    @len: u16,\n    data: [u8; @len.value],\n}\n",
        );

        let response = server
            .prepare_rename(lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(2, 16),
            })
            .expect("prepare rename should exist");

        let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response else {
            panic!("expected range with placeholder");
        };

        assert_eq!(placeholder, "len");
        assert_eq!(range.start, Position::new(2, 16));
        assert_eq!(range.end, Position::new(2, 19));
    }

    #[test]
    fn rename_returns_edits_for_shadowed_local_symbol() {
        let uri = uri("rename_local");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg(@len: u16) = {\n    @len: u8,\n    data: [u8; @len],\n    rest: [u8; @len],\n}\n",
        );

        let edit = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(2, 16),
                },
                new_name: "size".into(),
                work_done_progress_params: Default::default(),
            })
            .expect("rename should succeed")
            .expect("workspace edit should exist");

        assert_eq!(
            render_workspace_edit(&edit),
            "1:4-1:8 => @size\n2:15-2:19 => @size\n3:15-3:19 => @size"
        );
    }

    #[test]
    fn rename_accepts_sigiled_new_name_for_dependent_symbol() {
        let uri = uri("rename_local_sigiled_input");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg(@len: u16) = {\n    @len: u8,\n    data: [u8; @len],\n}\n",
        );

        let edit = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(2, 16),
                },
                new_name: "@size".into(),
                work_done_progress_params: Default::default(),
            })
            .expect("rename should succeed")
            .expect("workspace edit should exist");

        assert_eq!(
            render_workspace_edit(&edit),
            "1:4-1:8 => @size\n2:15-2:19 => @size"
        );
    }

    #[test]
    fn rename_preserves_dotted_dependent_suffixes() {
        let uri = uri("rename_dotted_local");
        let mut server = server();
        open_document(
            &mut server,
            &uri,
            1,
            "msg = {\n    @len: u16,\n    data: [u8; @len.value],\n}\n",
        );

        let edit = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(2, 16),
                },
                new_name: "size".into(),
                work_done_progress_params: Default::default(),
            })
            .expect("rename should succeed")
            .expect("workspace edit should exist");

        assert_eq!(
            render_workspace_edit(&edit),
            "1:4-1:8 => @size\n2:15-2:19 => @size"
        );
    }

    #[test]
    fn rename_rejects_invalid_new_name() {
        let uri = uri("rename_invalid_name");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = { field: u8, }\n");

        let err = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(0, 1),
                },
                new_name: "9bad".into(),
                work_done_progress_params: Default::default(),
            })
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn rename_rejects_reserved_new_name() {
        let uri = uri("rename_reserved_name");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = { field: u8, }\n");

        let err = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(0, 1),
                },
                new_name: "enum".into(),
                work_done_progress_params: Default::default(),
            })
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[test]
    fn rename_rejects_non_symbol_location() {
        let uri = uri("rename_invalid_target");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = { field: u8, }\n");

        let err = server
            .rename(RenameParams {
                text_document_position: lsp_types::TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: Position::new(0, 9),
                },
                new_name: "size".into(),
                work_done_progress_params: Default::default(),
            })
            .unwrap_err();

        assert_eq!(err.code, ErrorCode::REQUEST_FAILED);
    }

    #[test]
    fn goto_definition_returns_none_for_non_identifier() {
        let uri = uri("goto2");
        let mut server = server();
        open_document(&mut server, &uri, 1, "packet = { field: u8, }\n");

        let definition = server.goto_definition(GotoDefinitionParams {
            text_document_position_params: lsp_types::TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(0, 9), // cursor on "{"
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        });

        assert!(definition.is_none());
    }

    #[test]
    fn initialize_advertises_definition_provider() {
        let capabilities = server().initialize_result().capabilities;
        assert!(capabilities.definition_provider.is_some());
    }
}
