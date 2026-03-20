use std::collections::HashMap;
use std::ops::ControlFlow;

use async_lsp::client_monitor::ClientProcessMonitorLayer;
use async_lsp::panic::CatchUnwindLayer;
use async_lsp::router::Router;
use async_lsp::server::LifecycleLayer;
use async_lsp::tracing::TracingLayer;
use async_lsp::{ClientSocket, MainLoop};
use lsp_types::notification;
use lsp_types::request;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, MarkupContent, MarkupKind, PublishDiagnosticsParams,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo,
    TextDocumentItem, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    Url,
};
use tower::ServiceBuilder;
use tracing::{Level, warn};
use tree_sitter::Tree;
use vest_db::{AppliedDocumentChange, SourceDatabase};
use vest_syntax::{Parse, SemanticTokenKind, parse, parse_incremental};

pub struct VestServer {
    client: ClientSocket,
    source_db: SourceDatabase,
    parses: HashMap<Url, Parse>,
}

impl VestServer {
    pub fn new(client: ClientSocket) -> Self {
        Self {
            client,
            source_db: SourceDatabase::new(),
            parses: HashMap::new(),
        }
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

    pub fn initialized(&self) {}

    pub fn open_document(&mut self, document: TextDocumentItem) -> Vec<Diagnostic> {
        let uri = document.uri.clone();
        self.source_db
            .open(uri.clone(), document.version, document.text);
        self.parses.insert(
            uri.clone(),
            parse(
                self.source_db
                    .document(&uri)
                    .expect("document opened")
                    .text(),
            ),
        );
        self.diagnostics_for(&uri)
    }

    pub fn change_document(
        &mut self,
        params: DidChangeTextDocumentParams,
    ) -> Result<Vec<Diagnostic>, String> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let previous_tree = self.parses.get(&uri).map(|parse| parse.tree().clone());
        let edits = self
            .source_db
            .apply_changes(&uri, version, &params.content_changes)
            .map_err(|err| err.to_string())?;

        self.reparse_document(&uri, previous_tree, edits);
        Ok(self.diagnostics_for(&uri))
    }

    pub fn close_document(&mut self, params: DidCloseTextDocumentParams) {
        self.parses.remove(&params.text_document.uri);
        self.source_db.close(&params.text_document.uri);
    }

    pub fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let document = self.source_db.document(&uri)?;
        let parse = self.parses.get(&uri)?;
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
            format!("```vest\n{snippet}\n```\n\nSyntax node: `{}`", node.kind())
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

    pub fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Option<SemanticTokensResult> {
        let uri = params.text_document.uri;
        let document = self.source_db.document(&uri)?;
        let parse = self.parses.get(&uri)?;
        let data = encode_semantic_tokens(parse, document)?;

        Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        }))
    }

    pub fn diagnostics_for(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(document) = self.source_db.document(uri) else {
            return Vec::new();
        };
        let Some(parse) = self.parses.get(uri) else {
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

    fn reparse_document(
        &mut self,
        uri: &Url,
        mut previous_tree: Option<Tree>,
        edits: Vec<AppliedDocumentChange>,
    ) {
        if let Some(tree) = previous_tree.as_mut() {
            for edit in edits {
                tree.edit(&edit.input_edit);
            }
        }

        if let Some(document) = self.source_db.document(uri) {
            let parse = match previous_tree.as_ref() {
                Some(tree) => parse_incremental(document.text(), Some(tree)),
                None => parse(document.text()),
            };
            self.parses.insert(uri.clone(), parse);
        }
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
                    Err(err) => warn!("failed to handle document change: {err}"),
                }
                ControlFlow::Continue(())
            })
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
        .chain(keyword_completion("macro"))
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
            SemanticTokenType::MACRO,
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
        SemanticTokenKind::Macro => 4,
        SemanticTokenKind::Property => 5,
        SemanticTokenKind::Constant | SemanticTokenKind::Variable => 6,
        SemanticTokenKind::Parameter => 7,
        SemanticTokenKind::EnumMember => 8,
        SemanticTokenKind::Number => 9,
        SemanticTokenKind::String => 10,
        SemanticTokenKind::Operator => 11,
        SemanticTokenKind::Comment => 12,
    }
}

fn semantic_token_modifiers(kind: SemanticTokenKind) -> u32 {
    match kind {
        SemanticTokenKind::Constant => 1,
        _ => 0,
    }
}

fn encode_semantic_tokens(
    parse: &Parse,
    document: &vest_db::SourceDocument,
) -> Option<Vec<SemanticToken>> {
    let mut data = Vec::new();
    let mut last_line = 0;
    let mut last_start = 0;

    for token in parse.semantic_tokens(document.text()) {
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

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use lsp_types::{Position, Range, TextDocumentContentChangeEvent, TextDocumentIdentifier};

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

    fn full_change(uri: &Url, version: i32, text: &str) -> DidChangeTextDocumentParams {
        DidChangeTextDocumentParams {
            text_document: lsp_types::VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.into(),
            }],
        }
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
                    range: Some(Range::new(Position::new(1, 10), Position::new(1, 12))),
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

        assert!(server.source_db.contains(&uri));
        assert!(server.parses.contains_key(&uri));

        server.close_document(lsp_types::DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        });

        assert!(!server.source_db.contains(&uri));
        assert!(!server.parses.contains_key(&uri));
    }
}
