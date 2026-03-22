//! High-level Intermediate Representation (HIR) for Vest DSL.
//!
//! The HIR provides a simplified, ergonomic view of Vest source files for
//! semantic analysis. It captures definitions, types, and references while
//! abstracting away syntactic details.

mod lower;
mod names;
mod symbols;
mod types;

pub use lower::lower_to_hir;
pub use names::{dependent_binding_name, reference_name_text};
pub use symbols::{
    collect_references, definition_at_offset, file_definitions, resolve_local_symbol,
    resolve_symbol,
};
pub use types::{
    ArrayCombinator, ChoiceArm, ChoiceCombinator, Combinator, ConstValue, Definition,
    DefinitionKind, Endianness, EnumDef, EnumVariant, Field, FileHir, HirDiagnostic,
    HirDiagnosticKind, IntConstraint, IntType, LengthAtom, LengthExpr, LengthOp, LengthTerm,
    MacroDef, MacroParam, Name, OptionCombinator, Param, SizeTarget, Span, StructCombinator,
    VecCombinator, Visibility, WrapArg, WrapCombinator,
};
