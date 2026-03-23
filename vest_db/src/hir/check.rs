//! Type checking and semantic validation for Vest DSL.
//!
//! Implements the typing rules from `typing-rules.md` to:
//! - Build a global signature mapping names to their types
//! - Infer host types for format expressions
//! - Validate constraints and produce diagnostics for errors

use std::collections::{HashMap, HashSet};

use super::lower::lower_to_hir;
use crate::hir::types::*;
use crate::{Db, SourceFile};

/// Host type representation following typing-rules.md Section 1.2.
#[derive(Clone, PartialEq, Eq)]
pub enum HostType<'db> {
    /// Primitive integer type induced by a carrier κ
    Prim(IntType),
    /// Nominal enum type introduced by a source definition
    Enum(Name<'db>),
    /// Dynamically sized byte sequence (&[u8])
    Bytes,
    /// Fixed-size array [T; n]
    Array(Box<HostType<'db>>, u64),
    /// Variable-size vector Vec<T>
    Vec(Box<HostType<'db>>),
    /// Optional value Option<T>
    Option(Box<HostType<'db>>),
    /// Structural record type (field name -> type)
    Struct(Vec<StructFieldType<'db>>),
    /// Choice type with discriminant class and branch types
    Choice(DiscriminantClass<'db>, Vec<HostType<'db>>),
    /// Error placeholder for failed type inference
    Error,
}

#[derive(Clone, PartialEq, Eq)]
pub struct StructFieldType<'db> {
    pub name: Name<'db>,
    pub ty: HostType<'db>,
    pub is_dependent: bool,
}

impl<'db> std::fmt::Debug for HostType<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostType::Prim(t) => write!(f, "Prim({:?})", t),
            HostType::Enum(_) => write!(f, "Enum(..)"),
            HostType::Bytes => write!(f, "Bytes"),
            HostType::Array(elem, len) => write!(f, "Array({:?}, {})", elem, len),
            HostType::Vec(elem) => write!(f, "Vec({:?})", elem),
            HostType::Option(elem) => write!(f, "Option({:?})", elem),
            HostType::Struct(_) => write!(f, "Struct(..)"),
            HostType::Choice(_, _) => write!(f, "Choice(..)"),
            HostType::Error => write!(f, "Error"),
        }
    }
}

/// Discriminant class for choice combinators.
#[derive(Clone, PartialEq, Eq)]
pub enum DiscriminantClass<'db> {
    /// No discriminant (labeled choice)
    None,
    /// Primitive integer discriminant
    Prim(IntType),
    /// Enum discriminant
    Enum(Name<'db>),
    /// Dynamic bytes discriminant
    Bytes,
    /// Fixed-size byte array discriminant
    ByteArray(u64),
}

impl<'db> std::fmt::Debug for DiscriminantClass<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscriminantClass::None => write!(f, "None"),
            DiscriminantClass::Prim(t) => write!(f, "Prim({:?})", t),
            DiscriminantClass::Enum(_) => write!(f, "Enum(..)"),
            DiscriminantClass::Bytes => write!(f, "Bytes"),
            DiscriminantClass::ByteArray(n) => write!(f, "ByteArray({})", n),
        }
    }
}

/// Signature entry for a top-level definition.
#[derive(Clone, PartialEq, Eq)]
pub enum SignatureEntry<'db> {
    /// Format definition with parameter types and result type
    Format {
        params: Vec<(Name<'db>, HostType<'db>)>,
        result: HostType<'db>,
    },
    /// Enum definition
    Enum {
        repr: IntType,
        is_open: bool,
        variants: HashMap<Name<'db>, u64>,
    },
    /// Constant definition
    Const { ty: HostType<'db> },
}

/// Global signature mapping names to their declarations.
pub struct Signature<'db> {
    pub entries: HashMap<Name<'db>, SignatureEntry<'db>>,
}

/// Semantic diagnostic from type checking.
#[derive(Clone, PartialEq, Eq)]
pub struct CheckDiagnostic<'db> {
    pub message: String,
    pub span: Span,
    pub kind: CheckDiagnosticKind<'db>,
}

impl<'db> std::fmt::Debug for CheckDiagnostic<'db> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CheckDiagnostic {{ message: {:?}, span: {:?} }}",
            self.message, self.span
        )
    }
}

/// Owned semantic diagnostic payload for cached query results.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct SemanticDiagnostic {
    pub message: String,
    pub span: Span,
}

/// The kind of check diagnostic.
#[derive(Clone, PartialEq, Eq)]
pub enum CheckDiagnosticKind<'db> {
    DuplicateDefinition(Name<'db>),
    UndefinedSymbol(Name<'db>),
    UndefinedEnumVariant {
        variant: Name<'db>,
        enum_name: Name<'db>,
    },
    IntLiteralOutOfRange {
        value: u64,
        int_type: IntType,
    },
    ConstTypeMismatch,
    ConstLengthMismatch {
        expected: u64,
        actual: u64,
    },
    ConstValueOutOfRange {
        value: u64,
        int_type: IntType,
    },
    DuplicateField(Name<'db>),
    DuplicateEnumVariant(Name<'db>),
    DuplicateEnumValue(u64),
    EnumTypeSuffixMismatch,
    EnumTypeSuffixOutOfRange {
        value: u64,
        int_type: IntType,
    },
    ChoiceDiscriminantNotDependent,
    ChoiceDiscriminantUndefined(Name<'db>),
    ChoiceDiscriminantTypeMismatch {
        expected: String,
        actual: String,
    },
    ChoicePatternTypeMismatch,
    ChoicePatternLengthMismatch {
        expected: u64,
        actual: u64,
    },
    ChoicePatternDuplicate,
    ChoicePatternUndefinedVariant {
        variant: Name<'db>,
        enum_name: Name<'db>,
    },
    ChoiceNonExhaustive {
        missing: Vec<Name<'db>>,
    },
    ChoiceWildcardWithExhaustive,
    ChoicePatternOverlap,
    InvocationArgCountMismatch {
        expected: usize,
        actual: usize,
    },
    InvocationArgTypeMismatch {
        param: Name<'db>,
        expected: String,
        actual: String,
    },
    InvocationArgUndefined(Name<'db>),
    LengthExprTypeError {
        expected: String,
        actual: String,
    },
    LengthExprUndefined(Name<'db>),
    SizeExprUndefined(Name<'db>),
    SizeExprNotStatic(Name<'db>),
    SizeExprParameterized(Name<'db>),
    BindSourceNotRaw,
    ConstraintOnNonEnum,
    InvalidIntConstraint,
}

/// Local context for type checking (Δ in typing rules).
#[derive(Clone, Default)]
struct LocalContext<'db> {
    /// Maps dependent binding names to their host types
    bindings: HashMap<Name<'db>, HostType<'db>>,
}

impl<'db> LocalContext<'db> {
    fn extend(&mut self, name: Name<'db>, ty: HostType<'db>) {
        self.bindings.insert(name, ty);
    }

    fn lookup(&self, name: Name<'db>) -> Option<&HostType<'db>> {
        self.bindings.get(&name)
    }
}

#[derive(Clone, Copy)]
struct ConstraintInterval {
    start: u64,
    end: u64,
}

impl ConstraintInterval {
    fn singleton(value: u64) -> Self {
        Self {
            start: value,
            end: value,
        }
    }

    fn overlaps(self, other: Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Type checker context.
pub struct CheckContext<'db> {
    db: &'db dyn Db,
    signature: Signature<'db>,
    definitions: HashMap<Name<'db>, Definition<'db>>,
    in_progress: HashSet<Name<'db>>,
    diagnostics: Vec<CheckDiagnostic<'db>>,
}

impl<'db> CheckContext<'db> {
    fn new(db: &'db dyn Db, definitions: HashMap<Name<'db>, Definition<'db>>) -> Self {
        Self {
            db,
            signature: Signature {
                entries: HashMap::new(),
            },
            definitions,
            in_progress: HashSet::new(),
            diagnostics: Vec::new(),
        }
    }

    fn emit(&mut self, kind: CheckDiagnosticKind<'db>, message: String, span: Span) {
        self.diagnostics.push(CheckDiagnostic {
            message,
            span,
            kind,
        });
    }

    fn get_signature_entry(&mut self, name: Name<'db>) -> Option<SignatureEntry<'db>> {
        if let Some(entry) = self.signature.entries.get(&name).cloned() {
            return Some(entry);
        }
        self.ensure_signature(name);
        self.signature.entries.get(&name).cloned()
    }

    fn ensure_signature(&mut self, name: Name<'db>) {
        if self.signature.entries.contains_key(&name) || self.in_progress.contains(&name) {
            return;
        }

        let Some(def) = self.definitions.get(&name).cloned() else {
            return;
        };

        self.in_progress.insert(name);

        match &def.kind {
            DefinitionKind::Combinator { params, body } => {
                self.check_combinator_def(def.name, params, body, def.span);
            }
            DefinitionKind::Enum(enum_def) => {
                self.check_enum_def(def.name, enum_def, def.span);
            }
            DefinitionKind::Const { ty, value } => {
                self.check_const_def(def.name, ty, value, def.span);
            }
            DefinitionKind::Endianness(_) => {}
        }

        self.in_progress.remove(&name);
    }

    /// Check if an integer value fits the given integer type.
    fn value_fits_type(&self, value: u64, int_type: IntType) -> bool {
        match int_type {
            IntType::U8 => value <= u8::MAX as u64,
            IntType::U16 => value <= u16::MAX as u64,
            IntType::U24 => value <= 0xFF_FFFF,
            IntType::U32 => value <= u32::MAX as u64,
            IntType::U64 => true,
            IntType::I8 => value <= i8::MAX as u64,
            IntType::I16 => value <= i16::MAX as u64,
            IntType::I24 => value <= 0x7F_FFFF,
            IntType::I32 => value <= i32::MAX as u64,
            IntType::I64 => value <= i64::MAX as u64,
            IntType::BtcVarint | IntType::Uleb128 => true,
        }
    }

    /// Get the byte size of a primitive type.
    fn prim_byte_size(&self, int_type: IntType) -> Option<u64> {
        match int_type {
            IntType::U8 | IntType::I8 => Some(1),
            IntType::U16 | IntType::I16 => Some(2),
            IntType::U24 | IntType::I24 => Some(3),
            IntType::U32 | IntType::I32 => Some(4),
            IntType::U64 | IntType::I64 => Some(8),
            IntType::BtcVarint | IntType::Uleb128 => None,
        }
    }

    fn int_type_bounds(&self, int_type: IntType) -> ConstraintInterval {
        let end = match int_type {
            IntType::U8 => u8::MAX as u64,
            IntType::U16 => u16::MAX as u64,
            IntType::U24 => 0xFF_FFFF,
            IntType::U32 => u32::MAX as u64,
            IntType::U64 => u64::MAX,
            IntType::I8 => i8::MAX as u64,
            IntType::I16 => i16::MAX as u64,
            IntType::I24 => 0x7F_FFFF,
            IntType::I32 => i32::MAX as u64,
            IntType::I64 => i64::MAX as u64,
            IntType::BtcVarint | IntType::Uleb128 => u64::MAX,
        };

        ConstraintInterval { start: 0, end }
    }

    fn choice_anchor_span(&self, choice_comb: &ChoiceCombinator<'db>) -> Span {
        choice_comb
            .discriminant
            .map(|d| d.span)
            .or_else(|| choice_comb.arms.first().map(|arm| arm.span))
            .unwrap_or_else(|| Span::new(0, 0))
    }

    fn constraint_interval(
        &mut self,
        elem: &ConstraintElement<'db>,
        int_type: IntType,
    ) -> Option<ConstraintInterval> {
        match elem {
            ConstraintElement::Single {
                value: ConstValue::Int(v),
                ..
            } => {
                if self.value_fits_type(*v, int_type) {
                    Some(ConstraintInterval::singleton(*v))
                } else {
                    None
                }
            }
            ConstraintElement::Range { start, end, .. } => {
                let start_val = start.as_ref().and_then(ConstValue::as_int);
                let end_val = end.as_ref().and_then(ConstValue::as_int);
                let bounds = self.int_type_bounds(int_type);

                if start_val.is_none() && end_val.is_none() {
                    return None;
                }

                if start_val.is_some_and(|value| !self.value_fits_type(value, int_type))
                    || end_val.is_some_and(|value| !self.value_fits_type(value, int_type))
                {
                    return None;
                }

                let start = start_val.unwrap_or(bounds.start);
                let end = end_val.unwrap_or(bounds.end);
                if start > end {
                    return None;
                }

                Some(ConstraintInterval { start, end })
            }
            ConstraintElement::Single { .. } => None,
        }
    }

    /// Compute the exact static byte size of a host type.
    fn compute_static_size(&mut self, ty: &HostType<'db>) -> Option<u64> {
        match ty {
            HostType::Prim(int_type) => self.prim_byte_size(*int_type),
            HostType::Enum(name) => {
                if let Some(SignatureEntry::Enum { repr, .. }) = self.get_signature_entry(*name) {
                    self.prim_byte_size(repr)
                } else {
                    None
                }
            }
            HostType::Array(elem, len) => {
                let elem_size = self.compute_static_size(elem)?;
                Some(elem_size * len)
            }
            HostType::Struct(fields) => {
                let mut total = 0u64;
                for field in fields {
                    total += self.compute_static_size(&field.ty)?;
                }
                Some(total)
            }
            // These do not have static sizes
            HostType::Bytes | HostType::Vec(_) | HostType::Option(_) | HostType::Choice(_, _) => {
                None
            }
            HostType::Error => None,
        }
    }

    /// Try to evaluate a length expression to a static value.
    fn eval_length_expr(
        &mut self,
        expr: &LengthExpr<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> Option<u64> {
        if expr.terms.is_empty() {
            return None;
        }

        let mut result = self.eval_length_term(&expr.terms[0], local_ctx)?;

        for (i, op) in expr.ops.iter().enumerate() {
            let term_val = self.eval_length_term(&expr.terms[i + 1], local_ctx)?;
            match op {
                LengthOp::Add => result = result.checked_add(term_val)?,
                LengthOp::Sub => result = result.checked_sub(term_val)?,
                LengthOp::Mul => result = result.checked_mul(term_val)?,
                LengthOp::Div => {
                    if term_val == 0 {
                        return None;
                    }
                    result = result.checked_div(term_val)?;
                }
            }
        }

        Some(result)
    }

    fn eval_length_term(
        &mut self,
        term: &LengthTerm<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> Option<u64> {
        if term.atoms.is_empty() {
            return None;
        }

        let mut result = self.eval_length_atom(&term.atoms[0], local_ctx)?;

        for (i, op) in term.ops.iter().enumerate() {
            let atom_val = self.eval_length_atom(&term.atoms[i + 1], local_ctx)?;
            match op {
                LengthOp::Add => result = result.checked_add(atom_val)?,
                LengthOp::Sub => result = result.checked_sub(atom_val)?,
                LengthOp::Mul => result = result.checked_mul(atom_val)?,
                LengthOp::Div => {
                    if atom_val == 0 {
                        return None;
                    }
                    result = result.checked_div(atom_val)?;
                }
            }
        }

        Some(result)
    }

    fn eval_length_atom(
        &mut self,
        atom: &LengthAtom<'db>,
        _local_ctx: &LocalContext<'db>,
    ) -> Option<u64> {
        match atom {
            LengthAtom::Const(n) => Some(*n),
            LengthAtom::Param(_) => {
                // Parameters are not statically known
                None
            }
            LengthAtom::SizeOf(target) => match target {
                SizeTarget::Type(int_type) => self.prim_byte_size(*int_type),
                SizeTarget::Named(name_ref) => {
                    let entry = self.get_signature_entry(name_ref.name)?;
                    match entry {
                        SignatureEntry::Format { params, result } => {
                            if !params.is_empty() {
                                // Parameterized formats don't have static size
                                None
                            } else {
                                self.compute_static_size(&result)
                            }
                        }
                        SignatureEntry::Enum { repr, .. } => self.prim_byte_size(repr),
                        SignatureEntry::Const { ty } => self.compute_static_size(&ty),
                    }
                }
            },
            LengthAtom::Paren(inner) => self.eval_length_expr(inner, _local_ctx),
            LengthAtom::ProjectedParam { .. } => None,
        }
    }

    /// Check if a length expression references only valid symbols.
    fn check_length_expr_validity(
        &mut self,
        expr: &LengthExpr<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> bool {
        let mut valid = true;
        for term in &expr.terms {
            if !self.check_length_term_validity(term, local_ctx) {
                valid = false;
            }
        }
        valid
    }

    fn check_length_term_validity(
        &mut self,
        term: &LengthTerm<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> bool {
        let mut valid = true;
        for atom in &term.atoms {
            if !self.check_length_atom_validity(atom, local_ctx) {
                valid = false;
            }
        }
        valid
    }

    fn check_length_atom_validity(
        &mut self,
        atom: &LengthAtom<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> bool {
        match atom {
            LengthAtom::Const(_) => true,
            LengthAtom::Param(name_ref) => self.check_length_binding_type(name_ref, &[], local_ctx),
            LengthAtom::ProjectedParam { base, fields } => {
                self.check_length_binding_type(base, fields, local_ctx)
            }
            LengthAtom::SizeOf(target) => match target {
                SizeTarget::Type(_) => true,
                SizeTarget::Named(name_ref) => {
                    if let Some(entry) = self.get_signature_entry(name_ref.name) {
                        match entry {
                            SignatureEntry::Format { params, result } => {
                                if !params.is_empty() {
                                    self.emit(
                                        CheckDiagnosticKind::SizeExprParameterized(name_ref.name),
                                        format!(
                                            "size expression `|{}|` is invalid: format takes parameters",
                                            name_ref.name.as_str(self.db)
                                        ),
                                        name_ref.span,
                                    );
                                    false
                                } else if self.compute_static_size(&result).is_none() {
                                    self.emit(
                                        CheckDiagnosticKind::SizeExprNotStatic(name_ref.name),
                                        format!(
                                            "size expression `|{}|` is invalid: format does not have a static size",
                                            name_ref.name.as_str(self.db)
                                        ),
                                        name_ref.span,
                                    );
                                    false
                                } else {
                                    true
                                }
                            }
                            _ => true,
                        }
                    } else {
                        self.emit(
                            CheckDiagnosticKind::SizeExprUndefined(name_ref.name),
                            format!(
                                "undefined type `{}` in size expression",
                                name_ref.name.as_str(self.db)
                            ),
                            name_ref.span,
                        );
                        false
                    }
                }
            },
            LengthAtom::Paren(inner) => self.check_length_expr_validity(inner, local_ctx),
        }
    }

    fn resolve_length_binding_type(
        &mut self,
        name_ref: &NameRef<'db>,
        fields: &[Name<'db>],
        local_ctx: &LocalContext<'db>,
    ) -> Option<HostType<'db>> {
        let Some(mut ty) = local_ctx.lookup(name_ref.name).cloned() else {
            self.emit(
                CheckDiagnosticKind::LengthExprUndefined(name_ref.name),
                format!(
                    "undefined variable `@{}` in length expression",
                    name_ref.name.as_str(self.db)
                ),
                name_ref.span,
            );
            return None;
        };

        for field in fields {
            match ty {
                HostType::Struct(ref members) => {
                    if let Some(member) = members.iter().find(|member| member.name == *field) {
                        if !member.is_dependent {
                            self.emit(
                                CheckDiagnosticKind::LengthExprUndefined(*field),
                                format!(
                                    "field `{}` is not dependent and cannot be referenced in a length expression",
                                    field.as_str(self.db)
                                ),
                                name_ref.span,
                            );
                            return None;
                        }
                        ty = member.ty.clone();
                    } else {
                        self.emit(
                            CheckDiagnosticKind::LengthExprUndefined(*field),
                            format!(
                                "undefined field `{}` in length expression path",
                                field.as_str(self.db)
                            ),
                            name_ref.span,
                        );
                        return None;
                    }
                }
                _ => {
                    self.emit(
                        CheckDiagnosticKind::LengthExprTypeError {
                            expected: "struct".to_string(),
                            actual: self.type_description(&ty),
                        },
                        format!(
                            "length expression cannot project field `{}` from {}",
                            field.as_str(self.db),
                            self.type_description(&ty)
                        ),
                        name_ref.span,
                    );
                    return None;
                }
            }
        }

        Some(ty)
    }

    fn check_length_binding_type(
        &mut self,
        name_ref: &NameRef<'db>,
        fields: &[Name<'db>],
        local_ctx: &LocalContext<'db>,
    ) -> bool {
        let Some(ty) = self.resolve_length_binding_type(name_ref, fields, local_ctx) else {
            return false;
        };

        match ty {
            HostType::Prim(_) => true,
            _ => {
                self.emit(
                    CheckDiagnosticKind::LengthExprTypeError {
                        expected: "integer".to_string(),
                        actual: self.type_description(&ty),
                    },
                    format!(
                        "length expression requires integer type, found {}",
                        self.type_description(&ty)
                    ),
                    name_ref.span,
                );
                false
            }
        }
    }

    /// Get a human-readable description of a host type.
    fn type_description(&self, ty: &HostType<'db>) -> String {
        match ty {
            HostType::Prim(int_type) => format!("{:?}", int_type).to_lowercase(),
            HostType::Enum(name) => format!("enum {}", name.as_str(self.db)),
            HostType::Bytes => "bytes".to_string(),
            HostType::Array(elem, len) => format!("[{}; {}]", self.type_description(elem), len),
            HostType::Vec(elem) => format!("Vec<{}>", self.type_description(elem)),
            HostType::Option(elem) => format!("Option<{}>", self.type_description(elem)),
            HostType::Struct(_) => "struct".to_string(),
            HostType::Choice(_, _) => "choice".to_string(),
            HostType::Error => "error".to_string(),
        }
    }

    /// Infer the host type of a combinator.
    fn infer_combinator_type(
        &mut self,
        comb: &Combinator<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> HostType<'db> {
        match comb {
            Combinator::Int(int_type) => HostType::Prim(*int_type),
            Combinator::ConstrainedInt {
                int_type,
                constraint,
            } => {
                // Validate constraint values fit the type
                self.check_int_constraint(constraint, *int_type);
                HostType::Prim(*int_type)
            }
            Combinator::Reference { name, args, span } => {
                self.infer_reference_type(*name, args, *span, local_ctx)
            }
            Combinator::ConstrainedReference {
                name,
                constraint,
                negated: _,
                span,
            } => {
                // Check that the reference is to an enum
                let mut undefined_variants = Vec::new();
                let mut is_enum = false;
                let mut is_non_enum = false;

                if let Some(entry) = self.get_signature_entry(*name) {
                    match entry {
                        SignatureEntry::Enum { variants, .. } => {
                            is_enum = true;
                            // Check that constraint variants exist
                            for variant_ref in constraint {
                                if !variants.contains_key(&variant_ref.name) {
                                    undefined_variants.push((variant_ref.name, variant_ref.span));
                                }
                            }
                        }
                        _ => {
                            is_non_enum = true;
                        }
                    }
                } else {
                    self.emit(
                        CheckDiagnosticKind::UndefinedSymbol(*name),
                        format!("undefined type `{}`", name.as_str(self.db)),
                        *span,
                    );
                    return HostType::Error;
                }

                for (variant_name, variant_span) in undefined_variants {
                    self.emit(
                        CheckDiagnosticKind::UndefinedEnumVariant {
                            variant: variant_name,
                            enum_name: *name,
                        },
                        format!(
                            "undefined variant `{}` for enum `{}`",
                            variant_name.as_str(self.db),
                            name.as_str(self.db)
                        ),
                        variant_span,
                    );
                }

                if is_non_enum {
                    self.emit(
                        CheckDiagnosticKind::ConstraintOnNonEnum,
                        format!(
                            "enum constraint applied to non-enum type `{}`",
                            name.as_str(self.db)
                        ),
                        *span,
                    );
                    HostType::Error
                } else if is_enum {
                    HostType::Enum(*name)
                } else {
                    HostType::Error
                }
            }
            Combinator::Struct(struct_comb) => {
                self.infer_struct_type(struct_comb, local_ctx.clone())
            }
            Combinator::Choice(choice_comb) => self.infer_choice_type(choice_comb, local_ctx),
            Combinator::Array(array_comb) => self.infer_array_type(array_comb, local_ctx),
            Combinator::Vec(vec_comb) => {
                let elem_ty = self.infer_combinator_type(&vec_comb.element, local_ctx);
                HostType::Vec(Box::new(elem_ty))
            }
            Combinator::Option(opt_comb) => {
                let elem_ty = self.infer_combinator_type(&opt_comb.element, local_ctx);
                HostType::Option(Box::new(elem_ty))
            }
            Combinator::Wrap(wrap_comb) => self.infer_wrap_type(wrap_comb, local_ctx),
            Combinator::Tail => HostType::Bytes,
            Combinator::Bind {
                inner,
                target,
                span,
            } => {
                let inner_ty = self.infer_combinator_type(inner, local_ctx);
                // Check that inner produces raw bytes
                if !self.is_raw_type(&inner_ty) {
                    self.emit(
                        CheckDiagnosticKind::BindSourceNotRaw,
                        "bind source must produce raw bytes (bytes or [u8; n])".to_string(),
                        *span,
                    );
                }
                self.infer_combinator_type(target, local_ctx)
            }
            Combinator::Error => HostType::Error,
        }
    }

    /// Check if a type is "raw" (bytes or byte array).
    fn is_raw_type(&self, ty: &HostType<'db>) -> bool {
        match ty {
            HostType::Bytes => true,
            HostType::Array(elem, _) => matches!(elem.as_ref(), HostType::Prim(IntType::U8)),
            _ => false,
        }
    }

    fn infer_reference_type(
        &mut self,
        name: Name<'db>,
        args: &[NameRef<'db>],
        span: Span,
        local_ctx: &LocalContext<'db>,
    ) -> HostType<'db> {
        if let Some(entry) = self.get_signature_entry(name) {
            match entry {
                SignatureEntry::Format { params, result } => {
                    // Check argument count
                    if args.len() != params.len() {
                        self.emit(
                            CheckDiagnosticKind::InvocationArgCountMismatch {
                                expected: params.len(),
                                actual: args.len(),
                            },
                            format!(
                                "expected {} argument(s), found {}",
                                params.len(),
                                args.len()
                            ),
                            span,
                        );
                    }

                    // Check argument types
                    for (i, (arg_ref, (param_name, param_ty))) in
                        args.iter().zip(params.iter()).enumerate()
                    {
                        if let Some(arg_ty) = local_ctx.lookup(arg_ref.name) {
                            if !self.types_compatible(arg_ty, param_ty) {
                                self.emit(
                                    CheckDiagnosticKind::InvocationArgTypeMismatch {
                                        param: *param_name,
                                        expected: self.type_description(param_ty),
                                        actual: self.type_description(arg_ty),
                                    },
                                    format!(
                                        "argument {} type mismatch: expected {}, found {}",
                                        i + 1,
                                        self.type_description(param_ty),
                                        self.type_description(arg_ty)
                                    ),
                                    arg_ref.span,
                                );
                            }
                        } else {
                            self.emit(
                                CheckDiagnosticKind::InvocationArgUndefined(arg_ref.name),
                                format!("undefined argument `@{}`", arg_ref.name.as_str(self.db)),
                                arg_ref.span,
                            );
                        }
                    }

                    result
                }
                SignatureEntry::Enum { .. } => HostType::Enum(name),
                SignatureEntry::Const { ty } => ty,
            }
        } else {
            self.emit(
                CheckDiagnosticKind::UndefinedSymbol(name),
                format!("undefined type `{}`", name.as_str(self.db)),
                span,
            );
            HostType::Error
        }
    }

    fn types_compatible(&self, actual: &HostType<'db>, expected: &HostType<'db>) -> bool {
        match (actual, expected) {
            (HostType::Prim(a), HostType::Prim(b)) => a == b,
            (HostType::Enum(a), HostType::Enum(b)) => a == b,
            (HostType::Bytes, HostType::Bytes) => true,
            (HostType::Array(a_elem, a_len), HostType::Array(b_elem, b_len)) => {
                a_len == b_len && self.types_compatible(a_elem, b_elem)
            }
            (HostType::Vec(a), HostType::Vec(b)) => self.types_compatible(a, b),
            (HostType::Option(a), HostType::Option(b)) => self.types_compatible(a, b),
            _ => false,
        }
    }

    fn infer_struct_type(
        &mut self,
        struct_comb: &StructCombinator<'db>,
        mut local_ctx: LocalContext<'db>,
    ) -> HostType<'db> {
        let mut field_types = Vec::new();
        let mut seen_fields: HashSet<Name<'db>> = HashSet::new();

        for field in &struct_comb.fields {
            // Check for duplicate fields
            if seen_fields.contains(&field.name) {
                self.emit(
                    CheckDiagnosticKind::DuplicateField(field.name),
                    format!("duplicate field `{}`", field.name.as_str(self.db)),
                    field.span,
                );
            } else {
                seen_fields.insert(field.name);
            }

            let field_ty = self.infer_combinator_type(&field.ty, &local_ctx);

            // If this is a const field, validate the constant value
            if field.is_const {
                if let Some(const_val) = &field.const_value {
                    self.check_const_value(const_val, &field_ty, &field.ty, field.span);
                }
            }

            // If dependent, extend local context
            if field.is_dependent {
                local_ctx.extend(field.name, field_ty.clone());
            }

            field_types.push(StructFieldType {
                name: field.name,
                ty: field_ty,
                is_dependent: field.is_dependent,
            });
        }

        HostType::Struct(field_types)
    }

    fn infer_choice_type(
        &mut self,
        choice_comb: &ChoiceCombinator<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> HostType<'db> {
        // Determine discriminant class
        let disc_class = if let Some(disc_ref) = &choice_comb.discriminant {
            if let Some(disc_ty) = local_ctx.lookup(disc_ref.name) {
                match disc_ty {
                    HostType::Prim(int_type) => DiscriminantClass::Prim(*int_type),
                    HostType::Enum(name) => DiscriminantClass::Enum(*name),
                    HostType::Bytes => DiscriminantClass::Bytes,
                    HostType::Array(elem, len) => {
                        if matches!(elem.as_ref(), HostType::Prim(IntType::U8)) {
                            DiscriminantClass::ByteArray(*len)
                        } else {
                            self.emit(
                                CheckDiagnosticKind::ChoiceDiscriminantTypeMismatch {
                                    expected: "integer, enum, or byte array".to_string(),
                                    actual: self.type_description(disc_ty),
                                },
                                format!(
                                    "choice discriminant must be integer, enum, or byte array, found {}",
                                    self.type_description(disc_ty)
                                ),
                                disc_ref.span,
                            );
                            DiscriminantClass::None
                        }
                    }
                    _ => {
                        self.emit(
                            CheckDiagnosticKind::ChoiceDiscriminantTypeMismatch {
                                expected: "integer, enum, or byte array".to_string(),
                                actual: self.type_description(disc_ty),
                            },
                            format!(
                                "choice discriminant must be integer, enum, or byte array, found {}",
                                self.type_description(disc_ty)
                            ),
                            disc_ref.span,
                        );
                        DiscriminantClass::None
                    }
                }
            } else {
                self.emit(
                    CheckDiagnosticKind::ChoiceDiscriminantUndefined(disc_ref.name),
                    format!(
                        "undefined discriminant `@{}`",
                        disc_ref.name.as_str(self.db)
                    ),
                    disc_ref.span,
                );
                DiscriminantClass::None
            }
        } else {
            DiscriminantClass::None
        };

        // Check patterns and collect branch types
        let mut branch_types = Vec::new();
        let mut seen_patterns = Vec::new();
        let mut seen_variants = HashSet::new();
        let mut seen_labels = HashSet::new();
        let mut seen_array_patterns: HashSet<Vec<u64>> = HashSet::new();
        let mut has_wildcard = false;

        for arm in &choice_comb.arms {
            let body_ty = self.infer_combinator_type(&arm.body, local_ctx);
            branch_types.push(body_ty);

            // Validate pattern against discriminant class
            match (&arm.pattern, &disc_class) {
                (ChoicePattern::Wildcard, _) => {
                    if has_wildcard {
                        self.emit(
                            CheckDiagnosticKind::ChoicePatternDuplicate,
                            "duplicate wildcard pattern".to_string(),
                            arm.span,
                        );
                    }
                    has_wildcard = true;
                }
                (ChoicePattern::Variant(variant_ref), DiscriminantClass::Enum(enum_name)) => {
                    // Check variant exists
                    if let Some(SignatureEntry::Enum { variants, .. }) =
                        self.get_signature_entry(*enum_name)
                    {
                        if !variants.contains_key(&variant_ref.name) {
                            self.emit(
                                CheckDiagnosticKind::ChoicePatternUndefinedVariant {
                                    variant: variant_ref.name,
                                    enum_name: *enum_name,
                                },
                                format!(
                                    "undefined variant `{}` for enum `{}`",
                                    variant_ref.name.as_str(self.db),
                                    enum_name.as_str(self.db)
                                ),
                                variant_ref.span,
                            );
                        } else if seen_variants.contains(&variant_ref.name) {
                            self.emit(
                                CheckDiagnosticKind::ChoicePatternDuplicate,
                                format!(
                                    "duplicate pattern for variant `{}`",
                                    variant_ref.name.as_str(self.db)
                                ),
                                variant_ref.span,
                            );
                        } else {
                            seen_variants.insert(variant_ref.name);
                        }
                    }
                }
                (ChoicePattern::Variant(_), DiscriminantClass::Prim(_)) => {
                    self.emit(
                        CheckDiagnosticKind::ChoicePatternTypeMismatch,
                        "enum variant pattern not allowed for integer discriminant".to_string(),
                        arm.span,
                    );
                }
                (ChoicePattern::Constraint(elem), DiscriminantClass::Prim(int_type)) => {
                    self.check_constraint_element(elem, *int_type, &mut seen_patterns);
                }
                (ChoicePattern::Array(values), DiscriminantClass::ByteArray(expected_len)) => {
                    // Check array length
                    if values.len() as u64 != *expected_len {
                        self.emit(
                            CheckDiagnosticKind::ChoicePatternLengthMismatch {
                                expected: *expected_len,
                                actual: values.len() as u64,
                            },
                            format!(
                                "pattern length {} does not match discriminant length {}",
                                values.len(),
                                expected_len
                            ),
                            arm.span,
                        );
                    }
                    // Check values fit u8
                    for val in values {
                        if let ConstValue::Int(v) = val {
                            if *v > u8::MAX as u64 {
                                self.emit(
                                    CheckDiagnosticKind::ConstValueOutOfRange {
                                        value: *v,
                                        int_type: IntType::U8,
                                    },
                                    format!("value {} out of range for u8", v),
                                    arm.span,
                                );
                            }
                        }
                    }

                    let pattern = values
                        .iter()
                        .filter_map(ConstValue::as_int)
                        .collect::<Vec<_>>();
                    if pattern.len() == values.len() && !seen_array_patterns.insert(pattern) {
                        self.emit(
                            CheckDiagnosticKind::ChoicePatternDuplicate,
                            "duplicate byte-array pattern".to_string(),
                            arm.span,
                        );
                    }
                }
                (ChoicePattern::Variant(label_ref), DiscriminantClass::None) => {
                    if !seen_labels.insert(label_ref.name) {
                        self.emit(
                            CheckDiagnosticKind::ChoicePatternDuplicate,
                            format!(
                                "duplicate labeled choice arm `{}`",
                                label_ref.name.as_str(self.db)
                            ),
                            label_ref.span,
                        );
                    }
                }
                _ => {
                    // Pattern/discriminant mismatch
                    self.emit(
                        CheckDiagnosticKind::ChoicePatternTypeMismatch,
                        "pattern type does not match discriminant".to_string(),
                        arm.span,
                    );
                }
            }
        }

        // Check exhaustiveness for enum discriminants
        if let DiscriminantClass::Enum(enum_name) = &disc_class {
            // Collect info without holding borrow
            let (all_variants, is_open) =
                if let Some(SignatureEntry::Enum {
                    variants, is_open, ..
                }) = self.get_signature_entry(*enum_name)
                {
                    (variants.keys().copied().collect::<Vec<_>>(), is_open)
                } else {
                    (Vec::new(), false)
                };

            if !is_open && !has_wildcard {
                // Check that all variants are covered
                let missing: Vec<Name<'db>> = all_variants
                    .iter()
                    .filter(|v| !seen_variants.contains(*v))
                    .copied()
                    .collect();

                if !missing.is_empty() {
                    self.emit(
                        CheckDiagnosticKind::ChoiceNonExhaustive {
                            missing: missing.clone(),
                        },
                        format!(
                            "non-exhaustive choice: missing variant(s) {}",
                            missing
                                .iter()
                                .map(|n| n.as_str(self.db))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        self.choice_anchor_span(choice_comb),
                    );
                }
            }

            // Warn about unnecessary wildcard
            if has_wildcard && !is_open && seen_variants.len() == all_variants.len() {
                self.emit(
                    CheckDiagnosticKind::ChoiceWildcardWithExhaustive,
                    "wildcard pattern unnecessary: all variants are covered".to_string(),
                    self.choice_anchor_span(choice_comb),
                );
            }
        }

        HostType::Choice(disc_class, branch_types)
    }

    fn check_constraint_element(
        &mut self,
        elem: &ConstraintElement<'db>,
        int_type: IntType,
        seen: &mut Vec<ConstraintInterval>,
    ) {
        match elem {
            ConstraintElement::Single {
                value: ConstValue::Int(v),
                span,
            } => {
                if !self.value_fits_type(*v, int_type) {
                    self.emit(
                        CheckDiagnosticKind::IntLiteralOutOfRange {
                            value: *v,
                            int_type,
                        },
                        format!("value {} out of range for {:?}", v, int_type),
                        *span,
                    );
                    return;
                }

                let current = ConstraintInterval::singleton(*v);
                if let Some(previous) = seen.iter().find(|previous| previous.overlaps(current)) {
                    let (kind, message) = if previous.start == *v && previous.end == *v {
                        (
                            CheckDiagnosticKind::ChoicePatternDuplicate,
                            format!("duplicate pattern for value {}", v),
                        )
                    } else {
                        (
                            CheckDiagnosticKind::ChoicePatternOverlap,
                            format!("pattern value {} overlaps with previous pattern", v),
                        )
                    };
                    self.emit(kind, message, *span);
                }
                seen.push(current);
            }
            ConstraintElement::Range { start, end, span } => {
                let start_val = start.as_ref().and_then(|c| c.as_int());
                let end_val = end.as_ref().and_then(|c| c.as_int());

                if start_val.is_none() && end_val.is_none() {
                    self.emit(
                        CheckDiagnosticKind::InvalidIntConstraint,
                        "invalid integer constraint: range must have at least one endpoint"
                            .to_string(),
                        *span,
                    );
                    return;
                }

                if let Some(s) = start_val {
                    if !self.value_fits_type(s, int_type) {
                        self.emit(
                            CheckDiagnosticKind::IntLiteralOutOfRange { value: s, int_type },
                            format!("range start {} out of range for {:?}", s, int_type),
                            *span,
                        );
                    }
                }
                if let Some(e) = end_val {
                    if !self.value_fits_type(e, int_type) {
                        self.emit(
                            CheckDiagnosticKind::IntLiteralOutOfRange { value: e, int_type },
                            format!("range end {} out of range for {:?}", e, int_type),
                            *span,
                        );
                    }
                }

                if let (Some(s), Some(e)) = (start_val, end_val)
                    && s > e
                {
                    self.emit(
                        CheckDiagnosticKind::InvalidIntConstraint,
                        format!(
                            "invalid integer constraint: range start {} exceeds end {}",
                            s, e
                        ),
                        *span,
                    );
                    return;
                }

                if let Some(current) = self.constraint_interval(elem, int_type) {
                    if seen.iter().any(|previous| previous.overlaps(current)) {
                        let display_start =
                            start_val.unwrap_or_else(|| self.int_type_bounds(int_type).start);
                        let display_end =
                            end_val.unwrap_or_else(|| self.int_type_bounds(int_type).end);
                        self.emit(
                            CheckDiagnosticKind::ChoicePatternOverlap,
                            format!(
                                "pattern range {}..{} overlaps with previous pattern",
                                display_start, display_end
                            ),
                            *span,
                        );
                    }
                    seen.push(current);
                }
            }
            _ => {}
        }
    }

    fn infer_array_type(
        &mut self,
        array_comb: &ArrayCombinator<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> HostType<'db> {
        let elem_ty = self.infer_combinator_type(&array_comb.element, local_ctx);

        // Check length expression validity
        self.check_length_expr_validity(&array_comb.length, local_ctx);

        // Try to evaluate to static length
        if let Some(len) = self.eval_length_expr(&array_comb.length, local_ctx) {
            if matches!(elem_ty, HostType::Prim(IntType::U8)) {
                // Byte array with static length
                HostType::Array(Box::new(elem_ty), len)
            } else {
                // Non-byte array with static length
                HostType::Array(Box::new(elem_ty), len)
            }
        } else {
            // Dynamic length
            if matches!(elem_ty, HostType::Prim(IntType::U8)) {
                HostType::Bytes
            } else {
                HostType::Vec(Box::new(elem_ty))
            }
        }
    }

    fn infer_wrap_type(
        &mut self,
        wrap_comb: &WrapCombinator<'db>,
        local_ctx: &LocalContext<'db>,
    ) -> HostType<'db> {
        // Find the combinator argument (payload)
        for arg in &wrap_comb.args {
            if let WrapArg::Combinator(comb) = arg {
                return self.infer_combinator_type(comb, local_ctx);
            }
        }
        HostType::Error
    }

    fn check_int_constraint(&mut self, constraint: &IntConstraint<'db>, int_type: IntType) {
        for elem in &constraint.elements {
            match elem {
                ConstraintElement::Single {
                    value: ConstValue::Int(v),
                    span,
                } => {
                    if !self.value_fits_type(*v, int_type) {
                        self.emit(
                            CheckDiagnosticKind::IntLiteralOutOfRange {
                                value: *v,
                                int_type,
                            },
                            format!("constraint value {} out of range for {:?}", v, int_type),
                            *span,
                        );
                    }
                }
                ConstraintElement::Range { start, end, span } => {
                    let start_val = start.as_ref().and_then(ConstValue::as_int);
                    let end_val = end.as_ref().and_then(ConstValue::as_int);

                    if start_val.is_none() && end_val.is_none() {
                        self.emit(
                            CheckDiagnosticKind::InvalidIntConstraint,
                            "invalid integer constraint: range must have at least one endpoint"
                                .to_string(),
                            *span,
                        );
                        continue;
                    }

                    if let Some(s) = start_val {
                        if !self.value_fits_type(s, int_type) {
                            self.emit(
                                CheckDiagnosticKind::IntLiteralOutOfRange { value: s, int_type },
                                format!(
                                    "constraint range start {} out of range for {:?}",
                                    s, int_type
                                ),
                                *span,
                            );
                        }
                    }
                    if let Some(e) = end_val {
                        if !self.value_fits_type(e, int_type) {
                            self.emit(
                                CheckDiagnosticKind::IntLiteralOutOfRange { value: e, int_type },
                                format!(
                                    "constraint range end {} out of range for {:?}",
                                    e, int_type
                                ),
                                *span,
                            );
                        }
                    }
                    if let (Some(s), Some(e)) = (start_val, end_val)
                        && s > e
                    {
                        self.emit(
                            CheckDiagnosticKind::InvalidIntConstraint,
                            format!(
                                "invalid integer constraint: range start {} exceeds end {}",
                                s, e
                            ),
                            *span,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn check_const_value(
        &mut self,
        const_val: &ConstValue<'db>,
        expected_ty: &HostType<'db>,
        comb: &Combinator<'db>,
        span: Span,
    ) {
        match (const_val, expected_ty) {
            (ConstValue::Int(v), HostType::Prim(int_type)) => {
                if !self.value_fits_type(*v, *int_type) {
                    self.emit(
                        CheckDiagnosticKind::ConstValueOutOfRange {
                            value: *v,
                            int_type: *int_type,
                        },
                        format!("constant value {} out of range for {:?}", v, int_type),
                        span,
                    );
                }
            }
            (ConstValue::Array(values), HostType::Array(elem, expected_len)) => {
                if values.len() as u64 != *expected_len {
                    self.emit(
                        CheckDiagnosticKind::ConstLengthMismatch {
                            expected: *expected_len,
                            actual: values.len() as u64,
                        },
                        format!(
                            "array literal length {} does not match array length {}",
                            values.len(),
                            expected_len
                        ),
                        span,
                    );
                }

                for value in values {
                    match (value, elem.as_ref()) {
                        (ConstValue::Int(v), HostType::Prim(int_type)) => {
                            if !self.value_fits_type(*v, *int_type) {
                                self.emit(
                                    CheckDiagnosticKind::ConstValueOutOfRange {
                                        value: *v,
                                        int_type: *int_type,
                                    },
                                    format!("constant value {} out of range for {:?}", v, int_type),
                                    span,
                                );
                            }
                        }
                        (ConstValue::String(_), HostType::Prim(IntType::U8)) => {
                            self.emit(
                                CheckDiagnosticKind::ConstTypeMismatch,
                                "nested string literal not allowed in array constant".to_string(),
                                span,
                            );
                        }
                        (ConstValue::Array(_), _) => {
                            self.emit(
                                CheckDiagnosticKind::ConstTypeMismatch,
                                "nested array literal not allowed in array constant".to_string(),
                                span,
                            );
                        }
                        _ => {
                            self.emit(
                                CheckDiagnosticKind::ConstTypeMismatch,
                                format!(
                                    "array element type mismatch: expected {}",
                                    self.type_description(elem)
                                ),
                                span,
                            );
                        }
                    }
                }
            }
            (ConstValue::ByteString(s), HostType::Array(elem, expected_len)) => {
                if matches!(elem.as_ref(), HostType::Prim(IntType::U8)) {
                    let str_text = s.name.as_str(self.db);
                    if str_text.len() as u64 != *expected_len {
                        self.emit(
                            CheckDiagnosticKind::ConstLengthMismatch {
                                expected: *expected_len,
                                actual: str_text.len() as u64,
                            },
                            format!(
                                "string literal length {} does not match array length {}",
                                str_text.len(),
                                expected_len
                            ),
                            span,
                        );
                    }
                } else {
                    self.emit(
                        CheckDiagnosticKind::ConstTypeMismatch,
                        "string literal can only be used for byte arrays".to_string(),
                        span,
                    );
                }
            }
            (ConstValue::String(s), HostType::Array(elem, expected_len)) => {
                if matches!(elem.as_ref(), HostType::Prim(IntType::U8)) {
                    // String literal for byte array - check length
                    let str_text = s.name.as_str(self.db);
                    if str_text.len() as u64 != *expected_len {
                        self.emit(
                            CheckDiagnosticKind::ConstLengthMismatch {
                                expected: *expected_len,
                                actual: str_text.len() as u64,
                            },
                            format!(
                                "string literal length {} does not match array length {}",
                                str_text.len(),
                                expected_len
                            ),
                            span,
                        );
                    }
                } else {
                    self.emit(
                        CheckDiagnosticKind::ConstTypeMismatch,
                        "string literal can only be used for byte arrays".to_string(),
                        span,
                    );
                }
            }
            _ => {
                if matches!(comb, Combinator::Array(_)) {
                    self.emit(
                        CheckDiagnosticKind::ConstTypeMismatch,
                        format!(
                            "constant value does not match expected array type {}",
                            self.type_description(expected_ty)
                        ),
                        span,
                    );
                }
            }
        }
    }

    /// Check an enum definition.
    fn check_enum_def(&mut self, name: Name<'db>, enum_def: &EnumDef<'db>, _span: Span) {
        let mut seen_names: HashSet<Name<'db>> = HashSet::new();
        let mut seen_values: HashSet<u64> = HashSet::new();
        let mut type_suffix: Option<IntType> = None;

        for variant in &enum_def.variants {
            // Check for duplicate variant names
            if seen_names.contains(&variant.name) {
                self.emit(
                    CheckDiagnosticKind::DuplicateEnumVariant(variant.name),
                    format!("duplicate enum variant `{}`", variant.name.as_str(self.db)),
                    variant.span,
                );
            } else {
                seen_names.insert(variant.name);
            }

            // Check for duplicate values
            if let ConstValue::Int(v) = &variant.value {
                if seen_values.contains(v) {
                    self.emit(
                        CheckDiagnosticKind::DuplicateEnumValue(*v),
                        format!("duplicate enum value {}", v),
                        variant.span,
                    );
                } else {
                    seen_values.insert(*v);
                }
            }

            if let Some(variant_repr) = variant.repr_type {
                match type_suffix {
                    None => type_suffix = Some(variant_repr),
                    Some(existing) if existing != variant_repr => {
                        self.emit(
                            CheckDiagnosticKind::EnumTypeSuffixMismatch,
                            format!(
                                "enum variant `{}` uses {:?}, but previous variants use {:?}",
                                variant.name.as_str(self.db),
                                variant_repr,
                                existing
                            ),
                            variant.span,
                        );
                    }
                    Some(_) => {}
                }
            }
        }

        let inferred_repr = enum_def.repr_type.unwrap_or_else(|| {
            // Find minimum representation that fits all values
            let max_val = enum_def
                .variants
                .iter()
                .filter_map(|v| v.value.as_int())
                .max()
                .unwrap_or(0);

            if max_val <= u8::MAX as u64 {
                IntType::U8
            } else if max_val <= u16::MAX as u64 {
                IntType::U16
            } else if max_val <= u32::MAX as u64 {
                IntType::U32
            } else {
                IntType::U64
            }
        });

        let repr = type_suffix.unwrap_or(inferred_repr);

        // Check all values fit the chosen representation, and each explicit
        // per-variant suffix if present.
        for variant in &enum_def.variants {
            if let ConstValue::Int(v) = &variant.value {
                if !self.value_fits_type(*v, repr) {
                    self.emit(
                        CheckDiagnosticKind::EnumTypeSuffixOutOfRange {
                            value: *v,
                            int_type: repr,
                        },
                        format!("enum value {} out of range for {:?}", v, repr),
                        variant.span,
                    );
                }
                if let Some(variant_repr) = variant.repr_type
                    && !self.value_fits_type(*v, variant_repr)
                {
                    self.emit(
                        CheckDiagnosticKind::EnumTypeSuffixOutOfRange {
                            value: *v,
                            int_type: variant_repr,
                        },
                        format!("enum value {} out of range for {:?}", v, variant_repr),
                        variant.span,
                    );
                }
            }
        }

        // Build variant map
        let mut variants = HashMap::new();
        for variant in &enum_def.variants {
            if let ConstValue::Int(v) = &variant.value {
                variants.insert(variant.name, *v);
            }
        }

        // Add to signature
        self.signature.entries.insert(
            name,
            SignatureEntry::Enum {
                repr,
                is_open: !enum_def.is_exhaustive,
                variants,
            },
        );
    }

    /// Check a combinator definition.
    fn check_combinator_def(
        &mut self,
        name: Name<'db>,
        params: &[Param<'db>],
        body: &Combinator<'db>,
        _span: Span,
    ) {
        // Build parameter context
        let mut local_ctx = LocalContext::default();
        let mut param_types = Vec::new();
        let mut seen_params: HashSet<Name<'db>> = HashSet::new();

        for param in params {
            // Check for duplicate parameters
            if seen_params.contains(&param.name) {
                self.emit(
                    CheckDiagnosticKind::DuplicateField(param.name),
                    format!("duplicate parameter `@{}`", param.name.as_str(self.db)),
                    param.span,
                );
            } else {
                seen_params.insert(param.name);
            }

            let param_ty = self.infer_combinator_type(&param.ty, &local_ctx);
            local_ctx.extend(param.name, param_ty.clone());
            param_types.push((param.name, param_ty));
        }

        // Infer body type
        let result_ty = self.infer_combinator_type(body, &local_ctx);

        // Add to signature
        self.signature.entries.insert(
            name,
            SignatureEntry::Format {
                params: param_types,
                result: result_ty,
            },
        );
    }

    /// Check a const definition.
    fn check_const_def(
        &mut self,
        name: Name<'db>,
        ty_comb: &Combinator<'db>,
        value: &ConstValue<'db>,
        span: Span,
    ) {
        let local_ctx = LocalContext::default();
        let ty = self.infer_combinator_type(ty_comb, &local_ctx);

        // Validate constant value
        self.check_const_value(value, &ty, ty_comb, span);

        // Add to signature
        self.signature
            .entries
            .insert(name, SignatureEntry::Const { ty });
    }
}

fn definition_map<'db>(hir: &FileHir<'db>) -> HashMap<Name<'db>, Definition<'db>> {
    let mut defs = HashMap::new();
    for def in &hir.definitions {
        defs.entry(def.name).or_insert_with(|| def.clone());
    }
    defs
}

/// Check a lowered HIR file for semantic errors.
pub fn check_hir<'db>(db: &'db dyn Db, hir: &FileHir<'db>) -> Vec<CheckDiagnostic<'db>> {
    let mut ctx = CheckContext::new(db, definition_map(hir));

    // First pass: collect all definitions and check for duplicates
    let mut seen_defs: HashSet<Name<'db>> = HashSet::new();

    for def in &hir.definitions {
        if seen_defs.contains(&def.name) {
            ctx.emit(
                CheckDiagnosticKind::DuplicateDefinition(def.name),
                format!("duplicate definition `{}`", def.name.as_str(db)),
                def.name_span,
            );
        } else {
            seen_defs.insert(def.name);
        }
    }

    // Resolve all definitions lazily so forward references do not depend on
    // source order.
    for def in &hir.definitions {
        ctx.ensure_signature(def.name);
    }

    ctx.diagnostics
}

/// Check a source file for semantic errors through Salsa.
#[salsa::tracked]
pub fn check_file<'db>(db: &'db dyn Db, source: SourceFile) -> Vec<SemanticDiagnostic> {
    let hir = lower_to_hir(db, source);
    let mut diagnostics: Vec<SemanticDiagnostic> = hir
        .diagnostics
        .iter()
        .map(|diag| SemanticDiagnostic {
            message: diag.message.clone(),
            span: diag.span,
        })
        .collect();
    diagnostics.extend(
        check_hir(db, &hir)
            .into_iter()
            .map(|diag| SemanticDiagnostic {
                message: diag.message,
                span: diag.span,
            }),
    );
    diagnostics
}

/// Compute the exact static byte size of a definition if possible.
pub fn compute_static_size<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    def_name: Name<'db>,
) -> Option<u64> {
    let mut ctx = CheckContext::new(db, definition_map(hir));
    ctx.ensure_signature(def_name);

    // Look up definition and compute size
    if let Some(entry) = ctx.get_signature_entry(def_name) {
        match entry {
            SignatureEntry::Format { params, result } => {
                if !params.is_empty() {
                    None
                } else {
                    ctx.compute_static_size(&result)
                }
            }
            SignatureEntry::Enum { repr, .. } => ctx.prim_byte_size(repr),
            SignatureEntry::Const { ty } => ctx.compute_static_size(&ty),
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::lower_to_hir;
    use crate::{Database, Setter, SourceFile};

    /// Check a source string and return the number of diagnostics matching the predicate.
    fn count_diagnostics<F>(source: &str, predicate: F) -> usize
    where
        F: Fn(&CheckDiagnosticKind<'_>) -> bool,
    {
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source.to_string());
        let hir = lower_to_hir(&db, file);
        let diags = check_hir(&db, &hir);
        diags.iter().filter(|d| predicate(&d.kind)).count()
    }

    /// Check a source string and return the number of diagnostics in the full pipeline.
    fn check_count(source: &str) -> usize {
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source.to_string());
        check_file(&db, file).len()
    }

    fn corpus_source(rel_path: &str) -> String {
        let filepath = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("vest_corpus")
            .join(rel_path);
        std::fs::read_to_string(&filepath)
            .unwrap_or_else(|_| panic!("failed to read {}", filepath.display()))
    }

    #[test]
    fn test_duplicate_definition() {
        let source = r#"
foo = u8
foo = u16
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::DuplicateDefinition(_))
        });
        assert!(count > 0, "expected DuplicateDefinition diagnostic");
    }

    #[test]
    fn test_undefined_symbol() {
        let source = r#"
foo = {
    a: bar,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::UndefinedSymbol(_))
        });
        assert!(count > 0, "expected UndefinedSymbol diagnostic");
    }

    #[test]
    fn test_const_out_of_range() {
        let source = r#"
const BAR: u8 = 333
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::ConstValueOutOfRange { .. })
        });
        assert!(count > 0, "expected ConstValueOutOfRange diagnostic");
    }

    #[test]
    fn test_duplicate_field() {
        let source = r#"
fmt = {
    @f1: u8,
    @f1: u16,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::DuplicateField(_))
        });
        assert!(count > 0, "expected DuplicateField diagnostic");
    }

    #[test]
    fn test_enum_duplicate_variant() {
        let source = r#"
t = enum {
    A = 1,
    A = 2,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::DuplicateEnumVariant(_))
        });
        assert!(count > 0, "expected DuplicateEnumVariant diagnostic");
    }

    #[test]
    fn test_choice_non_exhaustive() {
        let source = r#"
t = enum {
    A = 1,
    B = 2,
    C = 3,
}

fmt(@tag: t) = choose(@tag) {
    A => u8,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::ChoiceNonExhaustive { .. })
        });
        assert!(count > 0, "expected ChoiceNonExhaustive diagnostic");
    }

    #[test]
    fn test_choice_undefined_discriminant() {
        let source = r#"
fmt = choose(@tag) {
    1 => u8,
    2 => u16,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::ChoiceDiscriminantUndefined(_))
        });
        assert!(count > 0, "expected ChoiceDiscriminantUndefined diagnostic");
    }

    #[test]
    fn test_invocation_arg_count() {
        let source = r#"
foo(@a: u8) = {
    payload: [u8; @a],
}

bar = {
    @a: u8,
    @b: u8,
    f: foo(@a, @b),
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::InvocationArgCountMismatch { .. })
        });
        assert!(count > 0, "expected InvocationArgCountMismatch diagnostic");
    }

    #[test]
    fn test_length_expr_undefined() {
        let source = r#"
fmt = {
    b: [u8; @a],
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::LengthExprUndefined(_))
        });
        assert!(count > 0, "expected LengthExprUndefined diagnostic");
    }

    #[test]
    fn test_size_expr_undefined() {
        let source = r#"
foo = {
    data: [u8; |undefined_type|],
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::SizeExprUndefined(_))
        });
        assert!(count > 0, "expected SizeExprUndefined diagnostic");
    }

    #[test]
    fn test_size_expr_not_static() {
        let source = r#"
opaque_payload = {
    @len: u16,
    body: [u8; @len],
}

bad = {
    body: [u8; |opaque_payload|],
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::SizeExprNotStatic(_))
        });
        assert!(count > 0, "expected SizeExprNotStatic diagnostic");
    }

    #[test]
    fn test_valid_file() {
        let source = r#"
t = enum {
    A = 1,
    B = 2,
}

msg(@tag: t) = choose(@tag) {
    A => u8,
    B => u16,
}
"#;
        let count = check_count(source);
        assert_eq!(count, 0, "expected no diagnostics");
    }

    #[test]
    fn test_forward_references_are_allowed() {
        let source = corpus_source("elab.vest");
        let count = check_count(&source);
        assert_eq!(count, 0, "expected no diagnostics for forward references");
    }

    #[test]
    fn test_nested_length_access_is_allowed() {
        let source = corpus_source("nested_access.vest");
        let count = check_count(&source);
        assert_eq!(
            count, 0,
            "expected no diagnostics for nested dependent access"
        );
    }

    #[test]
    fn test_forward_reference_static_size_computes() {
        let source = r#"
outer = inner
inner = u8
"#;
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source.to_string());
        let hir = lower_to_hir(&db, file);
        let size = super::compute_static_size(&db, &hir, Name::new(&db, "outer".to_string()));
        assert_eq!(size, Some(1));
    }

    #[test]
    fn test_invalid_int_range_reports_diagnostic() {
        let source = corpus_source("bad/int_constraints_invalid.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::InvalidIntConstraint)
        });
        assert!(count > 0, "expected InvalidIntConstraint diagnostic");
    }

    #[test]
    fn test_empty_int_range_reports_diagnostic() {
        let source = corpus_source("bad/int_constraints_invalid2.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::InvalidIntConstraint)
        });
        assert!(count > 0, "expected InvalidIntConstraint diagnostic");
    }

    #[test]
    fn test_duplicate_byte_array_pattern_reports_diagnostic() {
        let source = corpus_source("bad/choice_arrays_variant_duplicate.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::ChoicePatternDuplicate)
        });
        assert!(count > 0, "expected ChoicePatternDuplicate diagnostic");
    }

    #[test]
    fn test_mixed_enum_suffixes_report_diagnostic() {
        let source = corpus_source("bad/enum_inconsistent_type_suffix.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::EnumTypeSuffixMismatch)
        });
        assert!(count > 0, "expected EnumTypeSuffixMismatch diagnostic");
    }

    #[test]
    fn test_const_byte_array_value_range_reports_diagnostic() {
        let source = corpus_source("bad/const_byte_array_value_range.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::ConstValueOutOfRange { .. })
        });
        assert!(count > 0, "expected ConstValueOutOfRange diagnostic");
    }

    #[test]
    fn test_const_array_len_mismatch_reports_diagnostic() {
        let source = corpus_source("bad/const_array_len_mismatch.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::ConstLengthMismatch { .. })
        });
        assert!(count > 0, "expected ConstLengthMismatch diagnostic");
    }

    #[test]
    fn test_const_array_type_mismatch_reports_diagnostic() {
        let source = corpus_source("bad/const_array_type_mismatch.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::ConstTypeMismatch)
        });
        assert!(count > 0, "expected ConstTypeMismatch diagnostic");
    }

    #[test]
    fn test_large_integer_range_overlap_reports_without_enumeration() {
        let source = r#"
fmt(@tag: u32) = choose(@tag) {
    0..4294967295 => u8,
    42 => u16,
}
"#;
        let count = count_diagnostics(source, |k| {
            matches!(k, CheckDiagnosticKind::ChoicePatternOverlap)
        });
        assert!(count > 0, "expected ChoicePatternOverlap diagnostic");
    }

    #[test]
    fn test_labeled_choice_duplicate_arm_reports_diagnostic() {
        let source = corpus_source("bad/choice_enum_variant_duplicate2.vest");
        let count = count_diagnostics(&source, |k| {
            matches!(k, CheckDiagnosticKind::ChoicePatternDuplicate)
        });
        assert!(count > 0, "expected ChoicePatternDuplicate diagnostic");
    }

    #[test]
    fn test_non_dependent_projected_length_access_reports_diagnostic() {
        let source = corpus_source("bad/nested_non_dependent.vest");
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source);
        let hir = lower_to_hir(&db, file);
        let diag = check_hir(&db, &hir)
            .into_iter()
            .find(|diag| diag.message.contains("is not dependent"))
            .expect("expected non-dependent projection diagnostic");

        assert!(diag.span.start_byte < diag.span.end_byte);
    }

    #[test]
    fn test_invalid_u64_literal_reports_pipeline_diagnostic() {
        let source = corpus_source("bad/const_int_value_range4.vest");
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source);
        let diagnostics = check_file(&db, file);
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.message.contains("does not fit in u64")),
            "expected invalid integer literal diagnostic"
        );
    }

    #[test]
    fn test_invalid_int_constraint_reports_non_empty_span() {
        let source = "fmt = u8 | { .. }\n";
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source.to_string());
        let hir = lower_to_hir(&db, file);
        let diag = check_hir(&db, &hir)
            .into_iter()
            .find(|diag| matches!(diag.kind, CheckDiagnosticKind::InvalidIntConstraint))
            .expect("expected InvalidIntConstraint diagnostic");

        assert!(diag.span.start_byte < diag.span.end_byte);
        assert_eq!(&source[diag.span.start_byte..diag.span.end_byte], "..");
    }

    #[test]
    fn test_bind_source_not_raw_reports_non_empty_span() {
        let source = "fmt = u8 >>= u16\n";
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, source.to_string());
        let hir = lower_to_hir(&db, file);
        let diag = check_hir(&db, &hir)
            .into_iter()
            .find(|diag| matches!(diag.kind, CheckDiagnosticKind::BindSourceNotRaw))
            .expect("expected BindSourceNotRaw diagnostic");

        assert!(diag.span.start_byte < diag.span.end_byte);
        assert!(source[diag.span.start_byte..diag.span.end_byte].contains(">>="));
    }

    #[test]
    fn test_check_file_query_tracks_source_updates() {
        let mut db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 0, "fmt = u8\n".to_string());

        let initial = check_file(&db, file);
        assert!(initial.is_empty());

        file.set_text(&mut db).to("fmt = missing\n".to_string());

        let updated = check_file(&db, file);
        assert!(
            updated
                .iter()
                .any(|diag| diag.message.contains("undefined type")),
            "expected semantic diagnostics after source update"
        );
    }

    /// Test that each file in the bad corpus triggers at least one diagnostic.
    #[test]
    fn test_bad_corpus_files() {
        let corpus_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("vest_corpus/bad");

        let mut failures = Vec::new();
        let mut checked = 0usize;

        for entry in std::fs::read_dir(&corpus_path)
            .unwrap_or_else(|_| panic!("failed to read {}", corpus_path.display()))
        {
            let entry = entry.expect("failed to read bad corpus entry");
            let filepath = entry.path();
            if filepath.extension().and_then(|ext| ext.to_str()) != Some("vest") {
                continue;
            }

            checked += 1;
            let source = std::fs::read_to_string(&filepath)
                .unwrap_or_else(|_| panic!("failed to read {}", filepath.display()));
            let count = check_count(&source);
            if count == 0 {
                failures.push(
                    filepath
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("<unknown>")
                        .to_string(),
                );
            }
        }

        failures.sort();
        assert!(
            failures.is_empty(),
            "expected every bad corpus file to fail; {} file(s) still passed: {}",
            failures.len(),
            failures.join(", ")
        );
        assert!(checked > 0, "expected at least one bad corpus file");
    }
}
