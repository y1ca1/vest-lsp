mod db;
pub mod hir;
mod input;
mod parse;
mod source;

pub use db::{Database, Db};
pub use hir::{
    ArrayCombinator, CheckDiagnostic, CheckDiagnosticKind, ChoiceArm, ChoiceCombinator, Combinator,
    ConstValue, Definition, DefinitionKind, DiscriminantClass, Endianness, EnumDef, EnumVariant,
    Field, FileHir, HirDiagnostic, HirDiagnosticKind, HostType, IntConstraint, IntType, LengthAtom,
    LengthExpr, LengthOp, LengthTerm, Name, NameRef, OptionCombinator, Param, SemanticDiagnostic,
    Signature, SignatureEntry, SizeTarget, Span, StructCombinator, SymbolId, SymbolOccurrence,
    SymbolOccurrenceKind, VecCombinator, Visibility, WrapArg, WrapCombinator, check_file,
    check_hir, collect_references, compute_static_size, declaration_at_offset,
    definition_at_offset, definition_at_offset_in_hir, dependent_binding_name, file_definitions,
    is_valid_identifier_text, lower_to_hir, lower_to_hir_with_parse, reference_name_text,
    references_for_symbol, references_for_symbol_in_hir, resolve_local_symbol, resolve_symbol,
    resolve_symbol_in_hir, symbol_at_offset, symbol_at_offset_in_hir,
};
pub use input::SourceFile;
pub use parse::{ParseSummary, parse_file};
pub use salsa::Setter;
pub use source::{AppliedDocumentChange, ByteSpan, SourceDatabase, SourceDocument, SourceError};
