//! High-level Intermediate Representation (HIR) for Vest DSL.
//!
//! The HIR provides a simplified, ergonomic view of Vest source files for
//! semantic analysis. It captures definitions, types, and references while
//! abstracting away syntactic details.

mod lower;
mod names;
mod symbols;
mod types;

pub use lower::{lower_to_hir, lower_to_hir_with_parse};
pub use names::{dependent_binding_name, is_valid_identifier_text, reference_name_text};
pub use symbols::{
    collect_references, declaration_at_offset, definition_at_offset, definition_at_offset_in_hir,
    file_definitions, references_for_symbol, references_for_symbol_in_hir, resolve_local_symbol,
    resolve_symbol, resolve_symbol_in_hir, symbol_at_offset, symbol_at_offset_in_hir,
};
pub use types::{
    ArrayCombinator, ChoiceArm, ChoiceCombinator, Combinator, ConstValue, Definition,
    DefinitionKind, Endianness, EnumDef, EnumVariant, Field, FileHir, HirDiagnostic,
    HirDiagnosticKind, IntConstraint, IntType, LengthAtom, LengthExpr, LengthOp, LengthTerm, Name,
    NameRef, OptionCombinator, Param, SizeTarget, Span, StructCombinator, SymbolId,
    SymbolOccurrence, SymbolOccurrenceKind, VecCombinator, Visibility, WrapArg, WrapCombinator,
};
