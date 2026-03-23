//! HIR type definitions with Salsa interning.

use crate::Db;

/// Interned identifier to avoid repeated string allocations.
#[salsa::interned]
pub struct Name<'db> {
    #[returns(ref)]
    pub text: String,
}

impl<'db> Name<'db> {
    pub fn as_str(&self, db: &'db dyn Db) -> &'db str {
        self.text(db).as_str()
    }
}

/// A resolved or unresolved name occurrence with an exact source span.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NameRef<'db> {
    pub name: Name<'db>,
    pub span: Span,
}

impl<'db> NameRef<'db> {
    pub fn new(name: Name<'db>, span: Span) -> Self {
        Self { name, span }
    }

    pub fn as_str(&self, db: &'db dyn Db) -> &'db str {
        self.name.as_str(db)
    }
}

/// Source span for error reporting and navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
}

impl Span {
    pub fn new(start_byte: usize, end_byte: usize) -> Self {
        Self {
            start_byte,
            end_byte,
        }
    }

    pub fn from_node(node: &tree_sitter::Node) -> Self {
        Self::new(node.start_byte(), node.end_byte())
    }

    pub fn empty() -> Self {
        Self::new(0, 0)
    }

    pub fn contains(self, byte_offset: usize) -> bool {
        self.start_byte <= byte_offset && byte_offset < self.end_byte
    }
}

/// Visibility modifier for definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Default,
    Public,
    Secret,
}

/// Complete HIR for a single file.
#[derive(Clone, PartialEq, Eq)]
pub struct FileHir<'db> {
    pub definitions: Vec<Definition<'db>>,
    pub diagnostics: Vec<HirDiagnostic<'db>>,
}

/// A top-level definition in a Vest file.
#[derive(Clone, PartialEq, Eq)]
pub struct Definition<'db> {
    pub name: Name<'db>,
    pub visibility: Visibility,
    pub kind: DefinitionKind<'db>,
    pub name_span: Span,
    pub span: Span,
}

impl<'db> Definition<'db> {
    pub fn name_str(&self, db: &'db dyn Db) -> &'db str {
        self.name.as_str(db)
    }

    pub fn symbol_id(&self) -> SymbolId<'db> {
        SymbolId::TopLevel {
            name: self.name,
            declaration: self.name_span,
        }
    }
}

/// A symbol identity scoped to the current file.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolId<'db> {
    TopLevel {
        name: Name<'db>,
        declaration: Span,
    },
    Param {
        owner: Name<'db>,
        name: Name<'db>,
        declaration: Span,
    },
    Field {
        owner: Name<'db>,
        name: Name<'db>,
        declaration: Span,
        is_dependent: bool,
    },
    EnumVariant {
        owner: Name<'db>,
        name: Name<'db>,
        declaration: Span,
    },
}

impl<'db> SymbolId<'db> {
    pub fn is_sigiled(self) -> bool {
        matches!(
            self,
            SymbolId::Param { .. }
                | SymbolId::Field {
                    is_dependent: true,
                    ..
                }
        )
    }

    pub fn declaration_span(self) -> Span {
        match self {
            SymbolId::TopLevel { declaration, .. }
            | SymbolId::Param { declaration, .. }
            | SymbolId::Field { declaration, .. }
            | SymbolId::EnumVariant { declaration, .. } => declaration,
        }
    }

    pub fn name(self) -> Name<'db> {
        match self {
            SymbolId::TopLevel { name, .. }
            | SymbolId::Param { name, .. }
            | SymbolId::Field { name, .. }
            | SymbolId::EnumVariant { name, .. } => name,
        }
    }

    pub fn prepare_rename_span(self, occurrence: Span) -> Span {
        if self.is_sigiled() && occurrence.start_byte < occurrence.end_byte {
            Span::new(occurrence.start_byte + 1, occurrence.end_byte)
        } else {
            occurrence
        }
    }

    pub fn normalize_rename_input<'a>(self, new_name: &'a str) -> &'a str {
        if self.is_sigiled() {
            new_name.strip_prefix('@').unwrap_or(new_name)
        } else {
            new_name
        }
    }

    pub fn rename_text(self, new_name: &str) -> String {
        match self {
            SymbolId::Param { .. } => format!("@{new_name}"),
            SymbolId::Field { is_dependent, .. } if is_dependent => format!("@{new_name}"),
            _ => new_name.to_string(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolOccurrenceKind {
    Declaration,
    Reference,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolOccurrence<'db> {
    pub symbol: SymbolId<'db>,
    pub span: Span,
    pub kind: SymbolOccurrenceKind,
}

/// The kind of a top-level definition.
#[derive(Clone, PartialEq, Eq)]
pub enum DefinitionKind<'db> {
    Combinator {
        params: Vec<Param<'db>>,
        body: Combinator<'db>,
    },
    Enum(EnumDef<'db>),
    Const {
        ty: Combinator<'db>,
        value: ConstValue<'db>,
    },
    Endianness(Endianness),
}

/// A combinator type (the main building block of Vest formats).
#[derive(Clone, PartialEq, Eq)]
pub enum Combinator<'db> {
    /// Primitive integer type (u8, i16, etc.)
    Int(IntType),

    /// Integer type with constraint (u8 | 0..255)
    ConstrainedInt {
        int_type: IntType,
        constraint: IntConstraint<'db>,
    },

    /// Reference to another combinator by name
    Reference {
        name: Name<'db>,
        args: Vec<NameRef<'db>>,
        span: Span,
    },

    /// Reference with enum constraint (MyEnum | { A, B })
    ConstrainedReference {
        name: Name<'db>,
        constraint: Vec<NameRef<'db>>,
        negated: bool,
        span: Span,
    },

    /// Struct combinator { field1: Type1, field2: Type2 }
    Struct(StructCombinator<'db>),

    /// Choice combinator choose(@tag) { ... }
    Choice(ChoiceCombinator<'db>),

    /// Fixed-size array [Type; length]
    Array(ArrayCombinator<'db>),

    /// Variable-size vector Vec<Type>
    Vec(VecCombinator<'db>),

    /// Optional value Option<Type>
    Option(OptionCombinator<'db>),

    /// Wrap combinator wrap(...)
    Wrap(WrapCombinator<'db>),

    /// Variable-length tail bytes
    Tail,

    /// Bind operator (>>=)
    Bind {
        inner: Box<Combinator<'db>>,
        target: Box<Combinator<'db>>,
        span: Span,
    },

    /// Lowering error placeholder
    Error,
}

/// Integer type specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntType {
    U8,
    U16,
    U24,
    U32,
    U64,
    I8,
    I16,
    I24,
    I32,
    I64,
    BtcVarint,
    Uleb128,
}

impl IntType {
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "u8" => Some(Self::U8),
            "u16" => Some(Self::U16),
            "u24" => Some(Self::U24),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "i8" => Some(Self::I8),
            "i16" => Some(Self::I16),
            "i24" => Some(Self::I24),
            "i32" => Some(Self::I32),
            "i64" => Some(Self::I64),
            "btc_varint" => Some(Self::BtcVarint),
            "uleb128" => Some(Self::Uleb128),
            _ => None,
        }
    }

    pub fn bit_width(&self) -> Option<u32> {
        match self {
            Self::U8 | Self::I8 => Some(8),
            Self::U16 | Self::I16 => Some(16),
            Self::U24 | Self::I24 => Some(24),
            Self::U32 | Self::I32 => Some(32),
            Self::U64 | Self::I64 => Some(64),
            Self::BtcVarint | Self::Uleb128 => None,
        }
    }
}

/// Integer constraint expression.
#[derive(Clone, PartialEq, Eq)]
pub struct IntConstraint<'db> {
    pub elements: Vec<ConstraintElement<'db>>,
    pub negated: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ConstraintElement<'db> {
    Single {
        value: ConstValue<'db>,
        span: Span,
    },
    Range {
        start: Option<ConstValue<'db>>,
        end: Option<ConstValue<'db>>,
        span: Span,
    },
}

impl<'db> ConstraintElement<'db> {
    pub fn span(&self) -> Span {
        match self {
            ConstraintElement::Single { span, .. } | ConstraintElement::Range { span, .. } => *span,
        }
    }
}

/// A struct combinator with ordered fields.
#[derive(Clone, PartialEq, Eq)]
pub struct StructCombinator<'db> {
    pub fields: Vec<Field<'db>>,
}

/// A field in a struct combinator.
#[derive(Clone, PartialEq, Eq)]
pub struct Field<'db> {
    pub name: Name<'db>,
    pub is_dependent: bool,
    pub is_const: bool,
    pub ty: Combinator<'db>,
    pub const_value: Option<ConstValue<'db>>,
    pub span: Span,
}

/// A choice combinator with pattern matching arms.
#[derive(Clone, PartialEq, Eq)]
pub struct ChoiceCombinator<'db> {
    pub discriminant: Option<NameRef<'db>>,
    pub arms: Vec<ChoiceArm<'db>>,
}

/// A single arm in a choice combinator.
#[derive(Clone, PartialEq, Eq)]
pub struct ChoiceArm<'db> {
    pub pattern: ChoicePattern<'db>,
    pub body: Combinator<'db>,
    pub span: Span,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ChoicePattern<'db> {
    Variant(NameRef<'db>),
    Wildcard,
    Constraint(ConstraintElement<'db>),
    Array(Vec<ConstValue<'db>>),
}

/// Fixed-size array combinator.
#[derive(Clone, PartialEq, Eq)]
pub struct ArrayCombinator<'db> {
    pub element: Box<Combinator<'db>>,
    pub length: LengthExpr<'db>,
}

/// Variable-size vector combinator.
#[derive(Clone, PartialEq, Eq)]
pub struct VecCombinator<'db> {
    pub element: Box<Combinator<'db>>,
}

/// Optional value combinator.
#[derive(Clone, PartialEq, Eq)]
pub struct OptionCombinator<'db> {
    pub element: Box<Combinator<'db>>,
}

/// Wrap combinator for prefixing/suffixing with constants.
#[derive(Clone, PartialEq, Eq)]
pub struct WrapCombinator<'db> {
    pub args: Vec<WrapArg<'db>>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum WrapArg<'db> {
    Combinator(Combinator<'db>),
    ConstInt {
        ty: IntType,
        value: ConstValue<'db>,
    },
    ConstEnum {
        ty: NameRef<'db>,
        variant: NameRef<'db>,
    },
    ConstBytes {
        element_ty: IntType,
        length: u64,
        values: Vec<ConstValue<'db>>,
    },
}

/// Length expression for arrays.
#[derive(Clone, PartialEq, Eq)]
pub struct LengthExpr<'db> {
    pub terms: Vec<LengthTerm<'db>>,
    pub ops: Vec<LengthOp>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct LengthTerm<'db> {
    pub atoms: Vec<LengthAtom<'db>>,
    pub ops: Vec<LengthOp>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum LengthAtom<'db> {
    Const(u64),
    Param(NameRef<'db>),
    ProjectedParam {
        base: NameRef<'db>,
        fields: Vec<Name<'db>>,
    },
    SizeOf(SizeTarget<'db>),
    Paren(Box<LengthExpr<'db>>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum SizeTarget<'db> {
    Type(IntType),
    Named(NameRef<'db>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Enum definition.
#[derive(Clone, PartialEq, Eq)]
pub struct EnumDef<'db> {
    pub variants: Vec<EnumVariant<'db>>,
    pub is_exhaustive: bool,
    pub repr_type: Option<IntType>,
}

/// A single variant in an enum definition.
#[derive(Clone, PartialEq, Eq)]
pub struct EnumVariant<'db> {
    pub name: Name<'db>,
    pub value: ConstValue<'db>,
    pub repr_type: Option<IntType>,
    pub span: Span,
}

/// Parameter definition for combinators.
#[derive(Clone, PartialEq, Eq)]
pub struct Param<'db> {
    pub name: Name<'db>,
    pub ty: Combinator<'db>,
    pub span: Span,
}

/// Constant value (integer or string).
#[derive(Clone, PartialEq, Eq)]
pub enum ConstValue<'db> {
    Int(u64),
    String(NameRef<'db>),
    ByteString(NameRef<'db>),
    Array(Vec<ConstValue<'db>>),
}

impl<'db> ConstValue<'db> {
    pub fn as_int(&self) -> Option<u64> {
        match self {
            ConstValue::Int(v) => Some(*v),
            ConstValue::String(_) | ConstValue::ByteString(_) | ConstValue::Array(_) => None,
        }
    }
}

/// Endianness directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endianness {
    Little,
    Big,
}

/// Semantic diagnostic from HIR lowering.
#[derive(Clone, PartialEq, Eq)]
pub struct HirDiagnostic<'db> {
    pub message: String,
    pub span: Span,
    pub kind: HirDiagnosticKind<'db>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum HirDiagnosticKind<'db> {
    UnresolvedSymbol(Name<'db>),
    DuplicateDefinition(Name<'db>),
    InvalidConstant,
    UnsupportedSyntax,
}
