use std::collections::HashMap;

use lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};
use thiserror::Error;
use tree_sitter::{InputEdit, Point};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDocument {
    version: i32,
    text: String,
}

impl SourceDocument {
    pub fn new(version: i32, text: String) -> Self {
        Self { version, text }
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_version(&mut self, version: i32) {
        self.version = version;
    }

    pub fn full_span(&self) -> ByteSpan {
        ByteSpan {
            start_byte: 0,
            end_byte: self.text.len(),
            start_point: Point::new(0, 0),
            end_point: point_after(Point::new(0, 0), &self.text),
        }
    }

    pub fn byte_span_for_range(&self, range: Range) -> Result<ByteSpan, SourceError> {
        let start_byte = self.position_to_byte_offset(range.start)?;
        let end_byte = self.position_to_byte_offset(range.end)?;
        Ok(ByteSpan {
            start_byte,
            end_byte,
            start_point: self.position_to_point(range.start)?,
            end_point: self.position_to_point(range.end)?,
        })
    }

    pub fn byte_range_to_lsp_range(
        &self,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Range, SourceError> {
        Ok(Range::new(
            self.byte_offset_to_position(start_byte)?,
            self.byte_offset_to_position(end_byte)?,
        ))
    }

    pub fn byte_offset_to_position(&self, byte_offset: usize) -> Result<Position, SourceError> {
        let (line, line_start) = line_info_for_offset(&self.text, byte_offset)?;
        let character = self.text[line_start..byte_offset]
            .chars()
            .map(|ch| ch.len_utf16() as u32)
            .sum();
        Ok(Position::new(line as u32, character))
    }

    pub fn position_to_point(&self, position: Position) -> Result<Point, SourceError> {
        let line_start = line_start_offset(&self.text, position.line)?;
        let byte_offset = position_to_byte_offset_on_line(
            &self.text,
            line_start,
            position.line,
            position.character,
        )?;
        Ok(Point::new(position.line as usize, byte_offset - line_start))
    }

    pub fn position_to_byte_offset(&self, position: Position) -> Result<usize, SourceError> {
        let line_start = line_start_offset(&self.text, position.line)?;
        position_to_byte_offset_on_line(&self.text, line_start, position.line, position.character)
    }

    fn apply_change(
        &mut self,
        change: &TextDocumentContentChangeEvent,
    ) -> Result<AppliedDocumentChange, SourceError> {
        let old_span = match change.range {
            Some(range) => self.byte_span_for_range(range)?,
            None => self.full_span(),
        };

        self.text
            .replace_range(old_span.start_byte..old_span.end_byte, &change.text);

        Ok(AppliedDocumentChange {
            input_edit: InputEdit {
                start_byte: old_span.start_byte,
                old_end_byte: old_span.end_byte,
                new_end_byte: old_span.start_byte + change.text.len(),
                start_position: old_span.start_point,
                old_end_position: old_span.end_point,
                new_end_position: point_after(old_span.start_point, &change.text),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_point: Point,
    pub end_point: Point,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDocumentChange {
    pub input_edit: InputEdit,
}

#[derive(Debug, Default)]
pub struct SourceDatabase {
    documents: HashMap<Url, SourceDocument>,
}

impl SourceDatabase {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self, uri: Url, version: i32, text: String) {
        self.documents
            .insert(uri, SourceDocument::new(version, text));
    }

    pub fn close(&mut self, uri: &Url) -> Option<SourceDocument> {
        self.documents.remove(uri)
    }

    pub fn contains(&self, uri: &Url) -> bool {
        self.documents.contains_key(uri)
    }

    pub fn document(&self, uri: &Url) -> Option<&SourceDocument> {
        self.documents.get(uri)
    }

    pub fn apply_changes(
        &mut self,
        uri: &Url,
        version: i32,
        changes: &[TextDocumentContentChangeEvent],
    ) -> Result<Vec<AppliedDocumentChange>, SourceError> {
        let document = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| SourceError::DocumentNotOpen(uri.clone()))?;

        let mut applied = Vec::with_capacity(changes.len());
        for change in changes {
            applied.push(document.apply_change(change)?);
        }
        document.set_version(version);

        Ok(applied)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SourceError {
    #[error("document is not open: {0}")]
    DocumentNotOpen(Url),
    #[error("line {line} does not exist")]
    InvalidLine { line: u32 },
    #[error("character {character} is not valid for line {line}")]
    InvalidCharacter { line: u32, character: u32 },
    #[error("byte offset {byte_offset} is out of bounds")]
    InvalidByteOffset { byte_offset: usize },
    #[error("byte offset {byte_offset} does not align to a character boundary")]
    InvalidCharacterBoundary { byte_offset: usize },
}

fn line_start_offset(text: &str, line: u32) -> Result<usize, SourceError> {
    if line == 0 {
        return Ok(0);
    }

    let mut current_line = 0;
    for (idx, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            current_line += 1;
            if current_line == line {
                return Ok(idx + 1);
            }
        }
    }

    Err(SourceError::InvalidLine { line })
}

fn position_to_byte_offset_on_line(
    text: &str,
    line_start: usize,
    line: u32,
    character: u32,
) -> Result<usize, SourceError> {
    let line_end = text[line_start..]
        .find('\n')
        .map(|offset| line_start + offset)
        .unwrap_or(text.len());
    let line_text = &text[line_start..line_end];
    let target = character as usize;

    let mut consumed = 0;
    for (byte_offset, ch) in line_text.char_indices() {
        if consumed == target {
            return Ok(line_start + byte_offset);
        }

        consumed += ch.len_utf16();
        if consumed > target {
            return Err(SourceError::InvalidCharacter { line, character });
        }
    }

    if consumed == target {
        Ok(line_end)
    } else {
        Err(SourceError::InvalidCharacter { line, character })
    }
}

fn line_info_for_offset(text: &str, byte_offset: usize) -> Result<(usize, usize), SourceError> {
    if byte_offset > text.len() {
        return Err(SourceError::InvalidByteOffset { byte_offset });
    }
    if !text.is_char_boundary(byte_offset) {
        return Err(SourceError::InvalidCharacterBoundary { byte_offset });
    }

    let mut line = 0;
    let mut line_start = 0;
    for (idx, ch) in text.char_indices() {
        if idx >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + 1;
        }
    }

    Ok((line, line_start))
}

fn point_after(start: Point, text: &str) -> Point {
    match text.rfind('\n') {
        Some(last_newline) => Point::new(
            start.row + text.bytes().filter(|byte| *byte == b'\n').count(),
            text.len() - last_newline - 1,
        ),
        None => Point::new(start.row, start.column + text.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(name: &str) -> Url {
        Url::parse(&format!("file:///tmp/{name}.vest")).unwrap()
    }

    #[test]
    fn open_change_and_close_document() {
        let uri = uri("example");
        let mut db = SourceDatabase::new();
        db.open(uri.clone(), 1, "packet = { field: u8, }\n".into());

        let edits = db
            .apply_changes(
                &uri,
                2,
                &[TextDocumentContentChangeEvent {
                    range: Some(Range::new(Position::new(0, 18), Position::new(0, 20))),
                    range_length: None,
                    text: "u16".into(),
                }],
            )
            .unwrap();

        let document = db.document(&uri).unwrap();
        assert_eq!(document.version(), 2);
        assert_eq!(document.text(), "packet = { field: u16, }\n");
        assert_eq!(edits.len(), 1);
        assert_eq!(
            edits[0].input_edit.old_end_byte - edits[0].input_edit.start_byte,
            2
        );
        assert_eq!(
            edits[0].input_edit.new_end_byte - edits[0].input_edit.start_byte,
            3
        );

        assert!(db.close(&uri).is_some());
        assert!(!db.contains(&uri));
    }

    #[test]
    fn utf16_position_round_trip() {
        let document = SourceDocument::new(1, "label = \"💡\"\n".into());
        let after_emoji = Position::new(0, 11);
        let byte_offset = document.position_to_byte_offset(after_emoji).unwrap();

        assert_eq!(byte_offset, 13);
        assert_eq!(
            document.byte_offset_to_position(byte_offset).unwrap(),
            after_emoji
        );
    }

    #[test]
    fn document_not_open_error() {
        let mut db = SourceDatabase::new();
        let uri = uri("missing");
        let result = db.apply_changes(&uri, 1, &[]);
        assert_eq!(result.unwrap_err(), SourceError::DocumentNotOpen(uri));
    }

    #[test]
    fn position_and_byte_offset_round_trip_across_lines_and_utf16() {
        let document = SourceDocument::new(1, "a\né\n💡z\n".into());

        let positions = [
            Position::new(0, 0),
            Position::new(0, 1),
            Position::new(1, 0),
            Position::new(1, 1),
            Position::new(2, 0),
            Position::new(2, 2),
            Position::new(2, 3),
        ];

        for position in positions {
            let byte = document.position_to_byte_offset(position).unwrap();
            let round_trip = document.byte_offset_to_position(byte).unwrap();
            assert_eq!(round_trip, position);
        }
    }

    #[test]
    fn byte_span_for_range_matches_expected_points_and_bytes() {
        let document = SourceDocument::new(1, "ab\ncd\n".into());
        let range = Range::new(Position::new(0, 1), Position::new(1, 1));

        let span = document.byte_span_for_range(range).unwrap();

        assert_eq!(span.start_byte, 1);
        assert_eq!(span.end_byte, 4);
        assert_eq!(span.start_point, Point::new(0, 1));
        assert_eq!(span.end_point, Point::new(1, 1));
    }

    #[test]
    fn byte_range_to_lsp_range_round_trip() {
        let document = SourceDocument::new(1, "hello\n💡x\n".into());
        let original = Range::new(Position::new(1, 0), Position::new(1, 3));

        let start = document.position_to_byte_offset(original.start).unwrap();
        let end = document.position_to_byte_offset(original.end).unwrap();

        let converted = document.byte_range_to_lsp_range(start, end).unwrap();
        assert_eq!(converted, original);
    }

    #[test]
    fn apply_multiple_incremental_changes_updates_text_and_input_edits() {
        let uri = uri("multi_change");
        let mut db = SourceDatabase::new();
        db.open(uri.clone(), 1, "name = u8\n".into());

        let edits = db
            .apply_changes(
                &uri,
                2,
                &[
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 7), Position::new(0, 9))),
                        range_length: None,
                        text: "u16".into(),
                    },
                    TextDocumentContentChangeEvent {
                        range: Some(Range::new(Position::new(0, 0), Position::new(0, 4))),
                        range_length: None,
                        text: "packet".into(),
                    },
                ],
            )
            .unwrap();

        let document = db.document(&uri).unwrap();
        assert_eq!(document.version(), 2);
        assert_eq!(document.text(), "packet = u16\n");
        assert_eq!(edits.len(), 2);

        assert_eq!(edits[0].input_edit.start_byte, 7);
        assert_eq!(edits[0].input_edit.old_end_byte, 9);
        assert_eq!(edits[0].input_edit.new_end_byte, 10);

        assert_eq!(edits[1].input_edit.start_byte, 0);
        assert_eq!(edits[1].input_edit.old_end_byte, 4);
        assert_eq!(edits[1].input_edit.new_end_byte, 6);
    }

    #[test]
    fn full_document_replacement_uses_full_span() {
        let uri = uri("full_replace");
        let mut db = SourceDatabase::new();
        db.open(uri.clone(), 1, "old\ntext\n".into());

        let edits = db
            .apply_changes(
                &uri,
                3,
                &[TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: "new".into(),
                }],
            )
            .unwrap();

        let document = db.document(&uri).unwrap();
        assert_eq!(document.version(), 3);
        assert_eq!(document.text(), "new");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].input_edit.start_byte, 0);
        assert_eq!(edits[0].input_edit.old_end_byte, "old\ntext\n".len());
        assert_eq!(edits[0].input_edit.new_end_byte, 3);
        assert_eq!(edits[0].input_edit.start_position, Point::new(0, 0));
        assert_eq!(edits[0].input_edit.new_end_position, Point::new(0, 3));
    }

    #[test]
    fn invalid_character_inside_utf16_code_unit_is_rejected() {
        let document = SourceDocument::new(1, "💡\n".into());
        let err = document
            .position_to_byte_offset(Position::new(0, 1))
            .unwrap_err();
        assert_eq!(
            err,
            SourceError::InvalidCharacter {
                line: 0,
                character: 1
            }
        );
    }

    #[test]
    fn invalid_line_and_byte_offsets_are_reported() {
        let document = SourceDocument::new(1, "line\n".into());

        assert_eq!(
            document
                .position_to_byte_offset(Position::new(3, 0))
                .unwrap_err(),
            SourceError::InvalidLine { line: 3 }
        );

        assert_eq!(
            document.byte_offset_to_position(99).unwrap_err(),
            SourceError::InvalidByteOffset { byte_offset: 99 }
        );
    }

    #[test]
    fn invalid_character_boundary_is_reported() {
        let document = SourceDocument::new(1, "é".into());
        assert_eq!(
            document.byte_offset_to_position(1).unwrap_err(),
            SourceError::InvalidCharacterBoundary { byte_offset: 1 }
        );
    }
}
