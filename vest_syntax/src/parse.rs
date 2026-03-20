use std::collections::HashSet;

use tree_sitter::{InputEdit, Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::{HIGHLIGHTS_QUERY, language};

#[derive(Debug, Clone)]
pub struct Parse {
    tree: Tree,
    diagnostics: Vec<SyntaxDiagnostic>,
    semantic_tokens: Vec<SemanticToken>,
}

impl Parse {
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn root_node(&self) -> Node<'_> {
        self.tree.root_node()
    }

    pub fn diagnostics(&self) -> &[SyntaxDiagnostic] {
        &self.diagnostics
    }

    pub fn node_at_byte(&self, byte_offset: usize) -> Option<Node<'_>> {
        self.tree
            .root_node()
            .named_descendant_for_byte_range(byte_offset, byte_offset)
            .or_else(|| {
                self.tree
                    .root_node()
                    .descendant_for_byte_range(byte_offset, byte_offset)
            })
    }

    pub fn semantic_tokens(&self) -> &[SemanticToken] {
        &self.semantic_tokens
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxDiagnostic {
    pub message: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticTokenKind {
    Keyword,
    Modifier,
    Type,
    Function,
    Macro,
    Property,
    Constant,
    Parameter,
    Variable,
    EnumMember,
    Number,
    String,
    Operator,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticToken {
    pub kind: SemanticTokenKind,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub fn parse(source: &str) -> Parse {
    parse_incremental(source, None)
}

pub fn parse_with_edits(
    source: &str,
    previous_parse: Option<&Parse>,
    edits: &[InputEdit],
) -> Parse {
    let mut previous_tree = previous_parse.map(|parse| parse.tree.clone());
    if let Some(tree) = previous_tree.as_mut() {
        for edit in edits {
            tree.edit(edit);
        }
    }

    parse_incremental(source, previous_tree.as_ref())
}

pub fn parse_incremental(source: &str, previous_tree: Option<&Tree>) -> Parse {
    let mut parser = Parser::new();
    parser
        .set_language(&language())
        .expect("failed to load Vest grammar");

    let tree = parser
        .parse(source, previous_tree)
        .expect("Vest parser returned no syntax tree");
    let diagnostics = collect_syntax_diagnostics(tree.root_node(), source);
    let semantic_tokens = collect_semantic_tokens(&tree, source);

    Parse {
        tree,
        diagnostics,
        semantic_tokens,
    }
}

fn collect_syntax_diagnostics(root: Node<'_>, source: &str) -> Vec<SyntaxDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    let trimmed_end = source.trim_end_matches(char::is_whitespace).len();

    while let Some(node) = stack.pop() {
        if node.is_error() {
            let (message, start_byte, end_byte) = if node.end_byte() >= trimmed_end {
                (
                    "Unexpected end of file".to_string(),
                    trimmed_end,
                    trimmed_end,
                )
            } else {
                let snippet = source
                    .get(node.byte_range())
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .take(24)
                    .collect::<String>();
                let message = if snippet.is_empty() {
                    "Unexpected syntax".to_string()
                } else {
                    format!("Unexpected syntax near `{snippet}`")
                };
                (message, node.start_byte(), node.end_byte())
            };

            if seen.insert((start_byte, end_byte, message.clone())) {
                diagnostics.push(SyntaxDiagnostic {
                    message,
                    start_byte,
                    end_byte,
                });
            }
        }

        if node.is_missing() {
            let message = format!("Expected {}", node.kind());
            if seen.insert((node.start_byte(), node.end_byte(), message.clone())) {
                diagnostics.push(SyntaxDiagnostic {
                    message,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }

        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }

    diagnostics.sort_by_key(|diagnostic| (diagnostic.start_byte, diagnostic.end_byte));
    diagnostics
}

fn collect_semantic_tokens(tree: &Tree, source: &str) -> Vec<SemanticToken> {
    let query = Query::new(&language(), HIGHLIGHTS_QUERY).expect("invalid Vest highlight query");
    let mut cursor = QueryCursor::new();
    let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
    let capture_names = query.capture_names();

    let mut tokens = Vec::new();
    while {
        captures.advance();
        captures.get().is_some()
    } {
        let (query_match, capture_index) = captures.get().expect("capture exists");
        let capture = query_match.captures[*capture_index];
        let Some(kind) = semantic_kind_for_capture(capture_names[capture.index as usize]) else {
            continue;
        };

        let start_byte = capture.node.start_byte();
        let end_byte = capture.node.end_byte();
        if start_byte == end_byte {
            continue;
        }

        let token = SemanticToken {
            kind,
            start_byte,
            end_byte,
        };

        if tokens.last().copied() != Some(token) {
            tokens.push(token);
        }
    }

    tokens
}

fn semantic_kind_for_capture(capture_name: &str) -> Option<SemanticTokenKind> {
    match capture_name {
        "comment" => Some(SemanticTokenKind::Comment),
        "constant" => Some(SemanticTokenKind::Constant),
        "function.call" | "function.definition" => Some(SemanticTokenKind::Function),
        "function.macro" => Some(SemanticTokenKind::Macro),
        "keyword" | "keyword.directive" => Some(SemanticTokenKind::Keyword),
        "keyword.modifier" => Some(SemanticTokenKind::Modifier),
        "number" => Some(SemanticTokenKind::Number),
        "operator" => Some(SemanticTokenKind::Operator),
        "property" => Some(SemanticTokenKind::Property),
        "string" | "string.special" => Some(SemanticTokenKind::String),
        "type.builtin" => Some(SemanticTokenKind::Type),
        "type.enum.variant" => Some(SemanticTokenKind::EnumMember),
        "variable.parameter" => Some(SemanticTokenKind::Parameter),
        _ => None,
    }
}
