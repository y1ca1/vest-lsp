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
    pub span: Span,
}

impl<'db> Definition<'db> {
    pub fn name_str(&self, db: &'db dyn Db) -> &'db str {
        self.name.as_str(db)
    }
}

/// The kind of a top-level definition.
#[derive(Clone, PartialEq, Eq)]
pub enum DefinitionKind<'db> {
    Combinator {
        params: Vec<Param<'db>>,
        body: Combinator<'db>,
    },
    Enum(EnumDef<'db>),
    Macro(MacroDef<'db>),
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
        args: Vec<Name<'db>>,
        span: Span,
    },

    /// Reference with enum constraint (MyEnum | { A, B })
    ConstrainedReference {
        name: Name<'db>,
        constraint: Vec<Name<'db>>,
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
    },

    /// Macro invocation name!(args)
    MacroInvocation {
        name: Name<'db>,
        args: Vec<Combinator<'db>>,
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
    Single(ConstValue<'db>),
    Range {
        start: Option<ConstValue<'db>>,
        end: Option<ConstValue<'db>>,
    },
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
    pub discriminant: Option<Name<'db>>,
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
    Variant(Name<'db>),
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
        ty: Name<'db>,
        variant: Name<'db>,
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
    Param(Name<'db>),
    SizeOf(SizeTarget<'db>),
    Paren(Box<LengthExpr<'db>>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum SizeTarget<'db> {
    Type(IntType),
    Named(Name<'db>),
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
    pub span: Span,
}

/// Macro definition.
#[derive(Clone, PartialEq, Eq)]
pub struct MacroDef<'db> {
    pub params: Vec<MacroParam<'db>>,
    pub body: Combinator<'db>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MacroParam<'db> {
    pub name: Name<'db>,
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
    String(Name<'db>),
}

impl<'db> ConstValue<'db> {
    pub fn as_int(&self) -> Option<u64> {
        match self {
            ConstValue::Int(v) => Some(*v),
            ConstValue::String(_) => None,
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
