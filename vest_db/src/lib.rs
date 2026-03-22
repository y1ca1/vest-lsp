mod db;
pub mod hir;
mod input;
mod parse;
mod source;

pub use db::{Database, Db};
pub use hir::{
    ArrayCombinator, ChoiceArm, ChoiceCombinator, Combinator, ConstValue, Definition,
    DefinitionKind, Endianness, EnumDef, EnumVariant, Field, FileHir, HirDiagnostic,
    HirDiagnosticKind, IntConstraint, IntType, LengthAtom, LengthExpr, LengthOp, LengthTerm,
    MacroDef, MacroParam, Name, OptionCombinator, Param, SizeTarget, Span, StructCombinator,
    VecCombinator, Visibility, WrapArg, WrapCombinator, collect_references, definition_at_offset,
    dependent_binding_name, file_definitions, lower_to_hir, reference_name_text,
    resolve_local_symbol, resolve_symbol,
};
pub use input::SourceFile;
pub use parse::{ParseSummary, parse_file};
pub use salsa::Setter;
pub use source::{AppliedDocumentChange, ByteSpan, SourceDatabase, SourceDocument, SourceError};
