//! CST to HIR lowering.
//!
//! Traverses the Tree-sitter parse tree and constructs HIR nodes.

use tree_sitter::Node;

use super::names::dependent_binding_name;
use crate::hir::types::*;
use crate::{Db, SourceFile};

/// Lower a source file to HIR.
pub fn lower_to_hir<'db>(db: &'db dyn Db, source: SourceFile) -> FileHir<'db> {
    let text = source.text(db);
    let parse = vest_syntax::parse(text);
    lower_to_hir_with_parse(db, source, &parse)
}

/// Lower a source file to HIR using an existing parse tree.
pub fn lower_to_hir_with_parse<'db>(
    db: &'db dyn Db,
    source: SourceFile,
    parse: &vest_syntax::Parse,
) -> FileHir<'db> {
    let text = source.text(db);
    lower_to_hir_with_root(db, text, parse.root_node())
}

fn lower_to_hir_with_root<'db>(db: &'db dyn Db, text: &str, root: Node<'_>) -> FileHir<'db> {
    let mut ctx = LoweringContext::new(db, text);
    ctx.lower_source_file(root);

    FileHir {
        definitions: ctx.definitions,
        diagnostics: ctx.diagnostics,
    }
}

struct LoweringContext<'db, 'src> {
    db: &'db dyn Db,
    source: &'src str,
    definitions: Vec<Definition<'db>>,
    diagnostics: Vec<HirDiagnostic<'db>>,
}

enum LoweredDefinitionBody<'db> {
    Combinator(Combinator<'db>),
    Enum(EnumDef<'db>),
}

impl<'db, 'src> LoweringContext<'db, 'src> {
    fn new(db: &'db dyn Db, source: &'src str) -> Self {
        Self {
            db,
            source,
            definitions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn intern(&self, text: &str) -> Name<'db> {
        Name::new(self.db, text.to_string())
    }

    fn node_text(&self, node: Node<'_>) -> &'src str {
        node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    fn lower_source_file(&mut self, root: Node<'_>) {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "definition" {
                self.lower_definition(child);
            }
        }
    }

    fn lower_definition(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator_defn" => self.lower_combinator_defn(child),
                "const_combinator_defn" => self.lower_const_defn(child),
                "endianess_defn" => self.lower_endianness_defn(child),
                "macro_defn" => self.lower_macro_defn(child),
                _ => {}
            }
        }
    }

    fn lower_combinator_defn(&mut self, node: Node<'_>) {
        let mut visibility = Visibility::Default;
        let mut name_node = None;
        let mut params = Vec::new();
        let mut body_node = None;
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "modifier" => {
                    visibility = match self.node_text(child) {
                        "public" => Visibility::Public,
                        "secret" => Visibility::Secret,
                        _ => Visibility::Default,
                    };
                }
                "var_id" => {
                    name_node = Some(child);
                }
                "param_defn_list" => {
                    params = self.lower_param_defn_list(child);
                }
                "combinator" => {
                    body_node = Some(child);
                }
                _ => {}
            }
        }

        if let Some(name_node) = name_node {
            let name = self.intern(self.node_text(name_node));
            let name_span = Span::from_node(&name_node);
            let kind = if let Some(body_node) = body_node {
                match self.lower_definition_body(body_node) {
                    LoweredDefinitionBody::Combinator(body) => {
                        DefinitionKind::Combinator { params, body }
                    }
                    LoweredDefinitionBody::Enum(enum_def) => DefinitionKind::Enum(enum_def),
                }
            } else {
                DefinitionKind::Combinator {
                    params,
                    body: Combinator::Error,
                }
            };

            self.definitions.push(Definition {
                name,
                visibility,
                kind,
                name_span,
                span,
            });
        }
    }

    fn lower_definition_body(&mut self, node: Node<'_>) -> LoweredDefinitionBody<'db> {
        if let Some(enum_def) = self.lower_top_level_enum(node) {
            LoweredDefinitionBody::Enum(enum_def)
        } else {
            LoweredDefinitionBody::Combinator(self.lower_combinator(node))
        }
    }

    fn lower_top_level_enum(&mut self, node: Node<'_>) -> Option<EnumDef<'db>> {
        let mut has_bind_target = false;
        let mut enum_node = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator" => has_bind_target = true,
                "combinator_inner" => {
                    let mut inner_cursor = child.walk();
                    for inner in child.children(&mut inner_cursor) {
                        if inner.kind() == "enum_combinator" {
                            enum_node = Some(inner);
                        }
                    }
                }
                _ => {}
            }
        }

        if has_bind_target {
            None
        } else {
            enum_node.map(|enum_node| self.lower_enum_combinator(enum_node))
        }
    }

    fn lower_const_defn(&mut self, node: Node<'_>) {
        let mut name = None;
        let mut name_span = None;
        let mut ty = None;
        let mut value = None;
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_id" => {
                    name = Some(self.intern(self.node_text(child)));
                    name_span = Some(Span::from_node(&child));
                }
                "const_combinator" => {
                    let (t, v) = self.lower_const_combinator(child);
                    ty = t;
                    value = v;
                }
                _ => {}
            }
        }

        if let (Some(name), Some(name_span), Some(ty), Some(value)) = (name, name_span, ty, value) {
            self.definitions.push(Definition {
                name,
                visibility: Visibility::Default,
                kind: DefinitionKind::Const { ty, value },
                name_span,
                span,
            });
        }
    }

    fn lower_endianness_defn(&mut self, node: Node<'_>) {
        let text = self.node_text(node);
        let endianness = if text.contains("LITTLE") {
            Endianness::Little
        } else {
            Endianness::Big
        };

        let name = self.intern(text);
        self.definitions.push(Definition {
            name,
            visibility: Visibility::Default,
            kind: DefinitionKind::Endianness(endianness),
            name_span: Span::from_node(&node),
            span: Span::from_node(&node),
        });
    }

    fn lower_macro_defn(&mut self, node: Node<'_>) {
        let mut name = None;
        let mut name_span = None;
        let mut params = Vec::new();
        let mut body = None;
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "var_id" => {
                    name = Some(self.intern(self.node_text(child)));
                    name_span = Some(Span::from_node(&child));
                }
                "macro_param_list" => {
                    params = self.lower_macro_param_list(child);
                }
                "combinator" => {
                    body = Some(self.lower_combinator(child));
                }
                _ => {}
            }
        }

        if let (Some(name), Some(name_span), Some(body)) = (name, name_span, body) {
            self.definitions.push(Definition {
                name,
                visibility: Visibility::Default,
                kind: DefinitionKind::Macro(MacroDef { params, body }),
                name_span,
                span,
            });
        }
    }

    fn lower_param_defn_list(&mut self, node: Node<'_>) -> Vec<Param<'db>> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "param_defn"
                && let Some(param) = self.lower_param_defn(child)
            {
                params.push(param);
            }
        }
        params
    }

    fn lower_param_defn(&mut self, node: Node<'_>) -> Option<Param<'db>> {
        let mut name = None;
        let mut span = None;
        let mut ty = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "depend_id" => {
                    if let Some(binding) = dependent_binding_name(self.node_text(child)) {
                        name = Some(self.intern(binding));
                        span = Some(Span::from_node(&child));
                    }
                }
                "combinator_inner" => {
                    ty = Some(self.lower_combinator_inner(child));
                }
                _ => {}
            }
        }

        match (name, span, ty) {
            (Some(name), Some(span), Some(ty)) => Some(Param { name, ty, span }),
            _ => None,
        }
    }

    fn lower_macro_param_list(&mut self, node: Node<'_>) -> Vec<MacroParam<'db>> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "var_id" {
                params.push(MacroParam {
                    name: self.intern(self.node_text(child)),
                    span: Span::from_node(&child),
                });
            }
        }
        params
    }

    fn lower_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut inner = None;
        let mut bind_target = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator_inner" => {
                    inner = Some(self.lower_combinator_inner(child));
                }
                "combinator" => {
                    bind_target = Some(self.lower_combinator(child));
                }
                _ => {}
            }
        }

        match (inner, bind_target) {
            (Some(inner), Some(target)) => Combinator::Bind {
                inner: Box::new(inner),
                target: Box::new(target),
            },
            (Some(inner), None) => inner,
            _ => Combinator::Error,
        }
    }

    fn lower_combinator_inner(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "constraint_int_combinator" => return self.lower_constraint_int_combinator(child),
                "constraint_enum_combinator" => {
                    return self.lower_constraint_enum_combinator(child);
                }
                "struct_combinator" => return self.lower_struct_combinator(child),
                "choice_combinator" => return self.lower_choice_combinator(child),
                "array_combinator" => return self.lower_array_combinator(child),
                "vec_combinator" => return self.lower_vec_combinator(child),
                "option_combinator" => return self.lower_option_combinator(child),
                "wrap_combinator" => return self.lower_wrap_combinator(child),
                "tail_combinator" => return Combinator::Tail,
                "combinator_invocation" => return self.lower_combinator_invocation(child),
                "macro_invocation" => return self.lower_macro_invocation(child),
                "enum_combinator" => {
                    return self.lower_enum_combinator_as_combinator(child);
                }
                _ => {}
            }
        }
        Combinator::Error
    }

    fn lower_constraint_int_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut int_type = None;
        let mut constraint = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "int_combinator" => {
                    int_type = IntType::from_keyword(self.node_text(child));
                }
                "int_constraint" => {
                    constraint = Some(self.lower_int_constraint(child));
                }
                _ => {}
            }
        }

        match (int_type, constraint) {
            (Some(t), Some(c)) => Combinator::ConstrainedInt {
                int_type: t,
                constraint: c,
            },
            (Some(t), None) => Combinator::Int(t),
            _ => Combinator::Error,
        }
    }

    fn lower_int_constraint(&mut self, node: Node<'_>) -> IntConstraint<'db> {
        let mut elements = Vec::new();
        let mut negated = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "!" => negated = true,
                "int_constraint" => {
                    let inner = self.lower_int_constraint(child);
                    return IntConstraint {
                        elements: inner.elements,
                        negated: !inner.negated,
                    };
                }
                "constraint_elem" => {
                    if let Some(elem) = self.lower_constraint_elem(child) {
                        elements.push(elem);
                    }
                }
                "constraint_elem_set" => {
                    elements.extend(self.lower_constraint_elem_set(child));
                }
                _ => {}
            }
        }

        IntConstraint { elements, negated }
    }

    fn lower_constraint_elem_set(&mut self, node: Node<'_>) -> Vec<ConstraintElement<'db>> {
        let mut elements = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "constraint_elem"
                && let Some(elem) = self.lower_constraint_elem(child)
            {
                elements.push(elem);
            }
        }
        elements
    }

    fn lower_constraint_elem(&mut self, node: Node<'_>) -> Option<ConstraintElement<'db>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int_range" => return Some(self.lower_const_int_range(child)),
                "const_int" => {
                    return Some(ConstraintElement::Single(self.lower_const_int(child)));
                }
                _ => {}
            }
        }
        None
    }

    fn lower_const_int_range(&mut self, node: Node<'_>) -> ConstraintElement<'db> {
        let mut start = None;
        let mut end = None;
        let mut seen_dots = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "const_int" {
                if seen_dots {
                    end = Some(self.lower_const_int(child));
                } else {
                    start = Some(self.lower_const_int(child));
                }
            } else if child.kind() == ".." || self.node_text(child) == ".." {
                seen_dots = true;
            }
        }

        ConstraintElement::Range { start, end }
    }

    fn lower_const_int(&self, node: Node<'_>) -> ConstValue<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "decimal" => {
                    let text = self.node_text(child);
                    if let Ok(val) = text.parse::<u64>() {
                        return ConstValue::Int(val);
                    }
                }
                "hex" => {
                    let text = self.node_text(child);
                    let hex_str = text.strip_prefix("0x").unwrap_or(text);
                    if let Ok(val) = u64::from_str_radix(hex_str, 16) {
                        return ConstValue::Int(val);
                    }
                }
                "ascii" => {
                    let text = self.node_text(child);
                    if text.starts_with("'\\x") {
                        let hex_str = text
                            .strip_prefix("'\\x")
                            .and_then(|s| s.strip_suffix('\''))
                            .unwrap_or("");
                        if let Ok(val) = u64::from_str_radix(hex_str, 16) {
                            return ConstValue::Int(val);
                        }
                    } else if text.len() == 3 && text.starts_with('\'') && text.ends_with('\'') {
                        let c = text.chars().nth(1).unwrap_or('\0');
                        return ConstValue::Int(c as u64);
                    }
                }
                _ => {}
            }
        }
        ConstValue::Int(0)
    }

    fn lower_constraint_enum_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut name = None;
        let mut constraint = Vec::new();
        let mut negated = false;
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator_invocation" => {
                    if let Some(n) = self.extract_var_id(child) {
                        name = Some(n);
                    }
                }
                "enum_constraint" => {
                    let (c, n) = self.lower_enum_constraint(child);
                    constraint = c;
                    negated = n;
                }
                _ => {}
            }
        }

        if let Some(name) = name {
            Combinator::ConstrainedReference {
                name,
                constraint,
                negated,
                span,
            }
        } else {
            Combinator::Error
        }
    }

    fn lower_enum_constraint(&mut self, node: Node<'_>) -> (Vec<Name<'db>>, bool) {
        let mut elements = Vec::new();
        let mut negated = false;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "!" => negated = true,
                "enum_constraint" => {
                    let (inner, inner_negated) = self.lower_enum_constraint(child);
                    return (inner, !inner_negated);
                }
                "enum_constraint_elem" => {
                    elements.push(self.lower_enum_constraint_elem(child));
                }
                "enum_constraint_set" => {
                    elements.extend(self.lower_enum_constraint_set(child));
                }
                _ => {}
            }
        }

        (elements, negated)
    }

    fn lower_enum_constraint_elem(&mut self, node: Node<'_>) -> Name<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variant_id" {
                return self.intern(self.node_text(child));
            }
        }
        self.intern(self.node_text(node))
    }

    fn lower_enum_constraint_set(&mut self, node: Node<'_>) -> Vec<Name<'db>> {
        let mut elements = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_constraint_elem" {
                elements.push(self.lower_enum_constraint_elem(child));
            }
        }
        elements
    }

    fn lower_struct_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut fields = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "field"
                && let Some(field) = self.lower_field(child)
            {
                fields.push(field);
            }
        }
        Combinator::Struct(StructCombinator { fields })
    }

    fn lower_field(&mut self, node: Node<'_>) -> Option<Field<'db>> {
        let mut is_const = false;
        let mut is_dependent = false;
        let mut name = None;
        let mut span = None;
        let mut ty = None;
        let mut const_value = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "constant" => is_const = true,
                "depend_id" => {
                    is_dependent = true;
                    if let Some(binding) = dependent_binding_name(self.node_text(child)) {
                        name = Some(self.intern(binding));
                        span = Some(Span::from_node(&child));
                    }
                }
                "var_id" => {
                    name = Some(self.intern(self.node_text(child)));
                    span = Some(Span::from_node(&child));
                }
                "combinator" => {
                    ty = Some(self.lower_combinator(child));
                }
                "const_combinator" => {
                    let (t, v) = self.lower_const_combinator(child);
                    ty = t;
                    const_value = v;
                }
                _ => {}
            }
        }

        match (name, span, ty) {
            (Some(name), Some(span), Some(ty)) => Some(Field {
                name,
                is_dependent,
                is_const,
                ty,
                const_value,
                span,
            }),
            _ => None,
        }
    }

    fn lower_const_combinator(
        &mut self,
        node: Node<'_>,
    ) -> (Option<Combinator<'db>>, Option<ConstValue<'db>>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "inline_const_combinator" => {
                    return self.lower_inline_const_combinator(child);
                }
                "const_id" => {
                    let name = self.intern(self.node_text(child));
                    return (
                        Some(Combinator::Reference {
                            name,
                            args: Vec::new(),
                            span: Span::from_node(&child),
                        }),
                        None,
                    );
                }
                _ => {}
            }
        }
        (None, None)
    }

    fn lower_inline_const_combinator(
        &mut self,
        node: Node<'_>,
    ) -> (Option<Combinator<'db>>, Option<ConstValue<'db>>) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int_combinator" => {
                    return self.lower_const_int_combinator(child);
                }
                "const_enum_combinator" => {
                    return self.lower_const_enum_combinator(child);
                }
                "const_bytes_combinator" => {
                    return self.lower_const_bytes_combinator(child);
                }
                _ => {}
            }
        }
        (None, None)
    }

    fn lower_const_int_combinator(
        &mut self,
        node: Node<'_>,
    ) -> (Option<Combinator<'db>>, Option<ConstValue<'db>>) {
        let mut int_type = None;
        let mut value = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "int_combinator" => {
                    int_type = IntType::from_keyword(self.node_text(child));
                }
                "const_int" => {
                    value = Some(self.lower_const_int(child));
                }
                _ => {}
            }
        }

        match (int_type, value) {
            (Some(t), Some(v)) => (Some(Combinator::Int(t)), Some(v)),
            _ => (None, None),
        }
    }

    fn lower_const_enum_combinator(
        &mut self,
        node: Node<'_>,
    ) -> (Option<Combinator<'db>>, Option<ConstValue<'db>>) {
        let mut ty_name = None;
        let mut variant = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator_invocation" => {
                    ty_name = self.extract_var_id(child);
                }
                "variant_id" => {
                    variant = Some(self.intern(self.node_text(child)));
                }
                _ => {}
            }
        }

        match (ty_name, variant) {
            (Some(ty), Some(v)) => {
                let span = Span::from_node(&node);
                (
                    Some(Combinator::Reference {
                        name: ty,
                        args: Vec::new(),
                        span,
                    }),
                    Some(ConstValue::String(v)),
                )
            }
            _ => (None, None),
        }
    }

    fn lower_const_bytes_combinator(
        &mut self,
        node: Node<'_>,
    ) -> (Option<Combinator<'db>>, Option<ConstValue<'db>>) {
        // For const bytes, we return an array type - the values are stored in const_value
        let mut int_type = None;
        let mut length = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "int_combinator" => {
                    int_type = IntType::from_keyword(self.node_text(child));
                }
                "const_int" => {
                    length = Some(self.lower_const_int(child));
                }
                _ => {}
            }
        }

        match (int_type, length) {
            (Some(t), Some(l)) => {
                let len_val = l.as_int().unwrap_or(0);
                let arr = ArrayCombinator {
                    element: Box::new(Combinator::Int(t)),
                    length: LengthExpr {
                        terms: vec![LengthTerm {
                            atoms: vec![LengthAtom::Const(len_val)],
                            ops: Vec::new(),
                        }],
                        ops: Vec::new(),
                    },
                };
                (Some(Combinator::Array(arr)), Some(l))
            }
            _ => (None, None),
        }
    }

    fn lower_choice_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut discriminant = None;
        let mut arms = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "depend_id" => {
                    if let Some(binding) = dependent_binding_name(self.node_text(child)) {
                        discriminant = Some(self.intern(binding));
                    }
                }
                "choice_arm" => {
                    if let Some(arm) = self.lower_choice_arm(child) {
                        arms.push(arm);
                    }
                }
                _ => {}
            }
        }

        Combinator::Choice(ChoiceCombinator { discriminant, arms })
    }

    fn lower_choice_arm(&mut self, node: Node<'_>) -> Option<ChoiceArm<'db>> {
        let mut pattern = None;
        let mut body = None;
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "variant_id" => {
                    let text = self.node_text(child);
                    pattern = Some(if text == "_" {
                        ChoicePattern::Wildcard
                    } else {
                        ChoicePattern::Variant(self.intern(text))
                    });
                }
                "constraint_elem" => {
                    if let Some(elem) = self.lower_constraint_elem(child) {
                        pattern = Some(ChoicePattern::Constraint(elem));
                    }
                }
                "const_array" => {
                    pattern = Some(self.lower_const_array_pattern(child));
                }
                "combinator" => {
                    body = Some(self.lower_combinator(child));
                }
                _ => {}
            }
        }

        match (pattern, body) {
            (Some(pattern), Some(body)) => Some(ChoiceArm {
                pattern,
                body,
                span,
            }),
            _ => None,
        }
    }

    fn lower_const_array_pattern(&mut self, node: Node<'_>) -> ChoicePattern<'db> {
        let mut values = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int_array" | "int_array_expr" => {
                    values.extend(self.lower_const_int_array_values(child));
                }
                "repeat_int_array_expr" => {
                    values.extend(self.lower_repeat_int_array_values(child));
                }
                _ => {}
            }
        }
        ChoicePattern::Array(values)
    }

    fn lower_const_int_array_values(&mut self, node: Node<'_>) -> Vec<ConstValue<'db>> {
        let mut values = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int" => {
                    values.push(self.lower_const_int(child));
                }
                "int_array_expr" => {
                    values.extend(self.lower_const_int_array_values(child));
                }
                "repeat_int_array_expr" => {
                    values.extend(self.lower_repeat_int_array_values(child));
                }
                _ => {}
            }
        }
        values
    }

    fn lower_repeat_int_array_values(&mut self, node: Node<'_>) -> Vec<ConstValue<'db>> {
        let mut values = Vec::new();
        let mut value = None;
        let mut count = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "const_int" {
                if value.is_none() {
                    value = Some(self.lower_const_int(child));
                } else {
                    count = Some(self.lower_const_int(child).as_int().unwrap_or(0));
                }
            }
        }

        if let (Some(v), Some(c)) = (value, count) {
            for _ in 0..c {
                values.push(v.clone());
            }
        }

        values
    }

    fn lower_array_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut element = None;
        let mut length = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator" => {
                    element = Some(self.lower_combinator(child));
                }
                "length_expr" => {
                    length = Some(self.lower_length_expr(child));
                }
                _ => {}
            }
        }

        match (element, length) {
            (Some(element), Some(length)) => Combinator::Array(ArrayCombinator {
                element: Box::new(element),
                length,
            }),
            _ => Combinator::Error,
        }
    }

    fn lower_length_expr(&mut self, node: Node<'_>) -> LengthExpr<'db> {
        let mut terms = Vec::new();
        let mut ops = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "length_term" => {
                    terms.push(self.lower_length_term(child));
                }
                "add_op" => ops.push(LengthOp::Add),
                "sub_op" => ops.push(LengthOp::Sub),
                _ => {}
            }
        }

        LengthExpr { terms, ops }
    }

    fn lower_length_term(&mut self, node: Node<'_>) -> LengthTerm<'db> {
        let mut atoms = Vec::new();
        let mut ops = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "length_atom" => {
                    atoms.push(self.lower_length_atom(child));
                }
                "mul_op" => ops.push(LengthOp::Mul),
                "div_op" => ops.push(LengthOp::Div),
                _ => {}
            }
        }

        LengthTerm { atoms, ops }
    }

    fn lower_length_atom(&mut self, node: Node<'_>) -> LengthAtom<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int" => {
                    let val = self.lower_const_int(child).as_int().unwrap_or(0);
                    return LengthAtom::Const(val);
                }
                "depend_id" => {
                    if let Some(binding) = dependent_binding_name(self.node_text(child)) {
                        return LengthAtom::Param(self.intern(binding));
                    }
                }
                "size_expr" => {
                    return self.lower_size_expr(child);
                }
                "length_expr" => {
                    return LengthAtom::Paren(Box::new(self.lower_length_expr(child)));
                }
                _ => {}
            }
        }
        LengthAtom::Const(0)
    }

    fn lower_size_expr(&mut self, node: Node<'_>) -> LengthAtom<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "size_target" {
                return self.lower_size_target(child);
            }
        }
        LengthAtom::Const(0)
    }

    fn lower_size_target(&mut self, node: Node<'_>) -> LengthAtom<'db> {
        let text = self.node_text(node).trim();

        // Check for primitive types
        if let Some(int_type) = IntType::from_keyword(text) {
            return LengthAtom::SizeOf(SizeTarget::Type(int_type));
        }

        // Check for combined primitives (e.g., "u 8", "i 16")
        let combined: String = text.split_whitespace().collect();
        if let Some(int_type) = IntType::from_keyword(&combined) {
            return LengthAtom::SizeOf(SizeTarget::Type(int_type));
        }

        // Named type reference
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" {
                return LengthAtom::SizeOf(SizeTarget::Named(self.intern(self.node_text(child))));
            }
        }

        // Fall back to treating the whole text as a named reference
        LengthAtom::SizeOf(SizeTarget::Named(self.intern(text)))
    }

    fn lower_vec_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "combinator" {
                return Combinator::Vec(VecCombinator {
                    element: Box::new(self.lower_combinator(child)),
                });
            }
        }
        Combinator::Error
    }

    fn lower_option_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "combinator" {
                return Combinator::Option(OptionCombinator {
                    element: Box::new(self.lower_combinator(child)),
                });
            }
        }
        Combinator::Error
    }

    fn lower_wrap_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut args = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "wrap_args" {
                args = self.lower_wrap_args(child);
            }
        }

        Combinator::Wrap(WrapCombinator { args })
    }

    fn lower_wrap_args(&mut self, node: Node<'_>) -> Vec<WrapArg<'db>> {
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "wrap_arg"
                && let Some(arg) = self.lower_wrap_arg(child)
            {
                args.push(arg);
            }
        }
        args
    }

    fn lower_wrap_arg(&mut self, node: Node<'_>) -> Option<WrapArg<'db>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int_combinator" => {
                    if let (Some(Combinator::Int(ty)), Some(value)) =
                        self.lower_const_int_combinator(child)
                    {
                        return Some(WrapArg::ConstInt { ty, value });
                    }
                }
                "const_enum_combinator" => {
                    if let (Some(Combinator::Reference { name, .. }), Some(ConstValue::String(v))) =
                        self.lower_const_enum_combinator(child)
                    {
                        return Some(WrapArg::ConstEnum {
                            ty: name,
                            variant: v,
                        });
                    }
                }
                "const_bytes_or_array" => {
                    return self.lower_const_bytes_or_array(child);
                }
                "combinator" => {
                    return Some(WrapArg::Combinator(self.lower_combinator(child)));
                }
                _ => {}
            }
        }
        None
    }

    fn lower_const_bytes_or_array(&mut self, node: Node<'_>) -> Option<WrapArg<'db>> {
        let mut element_type = None;
        let mut length = None;
        let mut values = Vec::new();

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "combinator" => {
                    let comb = self.lower_combinator(child);
                    if let Combinator::Int(t) = comb {
                        element_type = Some(t);
                    }
                }
                "length_expr" => {
                    length = Some(self.lower_length_expr(child));
                }
                "const_array" => {
                    values = self.lower_const_array_values(child);
                }
                _ => {}
            }
        }

        match (element_type, length) {
            (Some(ty), Some(len)) => {
                if values.is_empty() {
                    // Just an array, no const values
                    Some(WrapArg::Combinator(Combinator::Array(ArrayCombinator {
                        element: Box::new(Combinator::Int(ty)),
                        length: len,
                    })))
                } else {
                    // Const bytes
                    let len_val = len
                        .terms
                        .first()
                        .and_then(|t| t.atoms.first())
                        .and_then(|a| match a {
                            LengthAtom::Const(v) => Some(*v),
                            _ => None,
                        })
                        .unwrap_or(values.len() as u64);
                    Some(WrapArg::ConstBytes {
                        element_ty: ty,
                        length: len_val,
                        values,
                    })
                }
            }
            _ => None,
        }
    }

    fn lower_const_array_values(&mut self, node: Node<'_>) -> Vec<ConstValue<'db>> {
        let mut values = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int_array" => {
                    values.extend(self.lower_const_int_array_values(child));
                }
                "const_char_array" => {
                    let text = self.node_text(child);
                    let content = text.trim_matches('"');
                    for c in content.chars() {
                        values.push(ConstValue::Int(c as u64));
                    }
                }
                _ => {}
            }
        }
        values
    }

    fn lower_combinator_invocation(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut name = None;
        let mut args = Vec::new();
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "var_id" => {
                    name = Some(self.intern(self.node_text(child)));
                }
                "param_list" => {
                    args = self.lower_param_list(child);
                }
                _ => {}
            }
        }

        if let Some(name) = name {
            Combinator::Reference { name, args, span }
        } else {
            Combinator::Error
        }
    }

    fn lower_param_list(&mut self, node: Node<'_>) -> Vec<Name<'db>> {
        let mut params = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "param"
                && let Some(p) = self.lower_param(child)
            {
                params.push(p);
            }
        }
        params
    }

    fn lower_param(&mut self, node: Node<'_>) -> Option<Name<'db>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "depend_id"
                && let Some(binding) = dependent_binding_name(self.node_text(child))
            {
                return Some(self.intern(binding));
            }
        }
        None
    }

    fn lower_macro_invocation(&mut self, node: Node<'_>) -> Combinator<'db> {
        let mut name = None;
        let mut args = Vec::new();
        let span = Span::from_node(&node);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "var_id" => {
                    name = Some(self.intern(self.node_text(child)));
                }
                "macro_arg_list" => {
                    args = self.lower_macro_arg_list(child);
                }
                _ => {}
            }
        }

        if let Some(name) = name {
            Combinator::MacroInvocation { name, args, span }
        } else {
            Combinator::Error
        }
    }

    fn lower_macro_arg_list(&mut self, node: Node<'_>) -> Vec<Combinator<'db>> {
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "combinator_inner" {
                args.push(self.lower_combinator_inner(child));
            }
        }
        args
    }

    fn lower_enum_combinator_as_combinator(&mut self, node: Node<'_>) -> Combinator<'db> {
        // When enum appears as a combinator body, we need to capture it differently.
        // This creates a struct-like representation with the enum variants.
        let enum_def = self.lower_enum_combinator(node);

        // For inline enum definitions, we create an anonymous enum type
        // This will be handled specially by later analysis
        Combinator::Struct(StructCombinator {
            fields: enum_def
                .variants
                .into_iter()
                .map(|v| Field {
                    name: v.name,
                    is_dependent: false,
                    is_const: true,
                    ty: Combinator::Int(enum_def.repr_type.unwrap_or(IntType::U8)),
                    const_value: Some(v.value),
                    span: v.span,
                })
                .collect(),
        })
    }

    fn lower_enum_combinator(&mut self, node: Node<'_>) -> EnumDef<'db> {
        let mut variants = Vec::new();
        let mut is_exhaustive = true;
        let mut repr_type = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "exhaustive_enum" => {
                    is_exhaustive = true;
                    let (v, r) = self.lower_enum_variants(child);
                    variants = v;
                    repr_type = r;
                }
                "non_exhaustive_enum" => {
                    is_exhaustive = false;
                    let (v, r) = self.lower_enum_variants(child);
                    variants = v;
                    repr_type = r;
                }
                _ => {}
            }
        }

        EnumDef {
            variants,
            is_exhaustive,
            repr_type,
        }
    }

    fn lower_enum_variants(&mut self, node: Node<'_>) -> (Vec<EnumVariant<'db>>, Option<IntType>) {
        let mut variants = Vec::new();
        let mut repr_type = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "enum_field"
                && let Some((variant, inferred_type)) = self.lower_enum_field(child)
            {
                variants.push(variant);
                if repr_type.is_none() {
                    repr_type = inferred_type;
                }
            }
        }

        (variants, repr_type)
    }

    fn lower_enum_field(&mut self, node: Node<'_>) -> Option<(EnumVariant<'db>, Option<IntType>)> {
        let mut name = None;
        let mut span = None;
        let mut value = None;
        let mut repr_type = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "variant_id" => {
                    name = Some(self.intern(self.node_text(child)));
                    span = Some(Span::from_node(&child));
                }
                "typed_const_int" => {
                    let (v, t) = self.lower_typed_const_int(child);
                    value = Some(v);
                    repr_type = t;
                }
                _ => {}
            }
        }

        match (name, span, value) {
            (Some(name), Some(span), Some(value)) => {
                Some((EnumVariant { name, value, span }, repr_type))
            }
            _ => None,
        }
    }

    fn lower_typed_const_int(&mut self, node: Node<'_>) -> (ConstValue<'db>, Option<IntType>) {
        let mut value = ConstValue::Int(0);
        let mut int_type = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_int" => {
                    value = self.lower_const_int(child);
                }
                "int_type_suffix" => {
                    int_type = IntType::from_keyword(self.node_text(child));
                }
                _ => {}
            }
        }

        (value, int_type)
    }

    fn extract_var_id(&self, node: Node<'_>) -> Option<Name<'db>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "var_id" {
                return Some(self.intern(self.node_text(child)));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use tree_sitter::{InputEdit, Point};

    use crate::{Database, Setter, SourceFile};

    use super::*;

    fn check(source: &str, expected: expect_test::Expect) {
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 1, source.to_string());
        let hir = lower_to_hir(&db, file);
        let result = format_definitions(&db, &hir);
        expected.assert_eq(&result);
    }

    #[test]
    fn lower_to_hir_with_parse_matches_fresh_lowering_after_incremental_edit() {
        let mut db = Database::new();
        let file = SourceFile::new(
            &db,
            "test.vest".to_string(),
            1,
            "packet = {\n    field: u8,\n}\n".to_string(),
        );
        let initial = vest_syntax::parse(file.text(&db));
        let updated = "packet = {\n    field: u16,\n}\n";
        let parse = vest_syntax::parse_with_edits(
            updated,
            Some(&initial),
            &[InputEdit {
                start_byte: 22,
                old_end_byte: 24,
                new_end_byte: 25,
                start_position: Point::new(1, 11),
                old_end_position: Point::new(1, 13),
                new_end_position: Point::new(1, 14),
            }],
        );
        file.set_text(&mut db).to(updated.to_string());

        let incremental = lower_to_hir_with_parse(&db, file, &parse);
        let fresh = lower_to_hir(&db, file);

        assert!(incremental == fresh);
    }

    fn format_definitions<'db>(db: &'db Database, hir: &FileHir<'db>) -> String {
        hir.definitions
            .iter()
            .map(|def| format_definition(db, def))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_definition<'db>(db: &'db dyn Db, def: &Definition<'db>) -> String {
        let name = def.name.as_str(db);
        let vis = match def.visibility {
            Visibility::Public => "public ",
            Visibility::Secret => "secret ",
            Visibility::Default => "",
        };

        match &def.kind {
            DefinitionKind::Combinator { params, body } => {
                let params_str = if params.is_empty() {
                    String::new()
                } else {
                    format!(
                        "({})",
                        params
                            .iter()
                            .map(|p| p.name.as_str(db))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!("{vis}{name}{params_str} = {}", format_combinator(db, body))
            }
            DefinitionKind::Enum(e) => {
                let exhaustive = if e.is_exhaustive {
                    ""
                } else {
                    " (non-exhaustive)"
                };
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        format!(
                            "{} = {}",
                            v.name.as_str(db),
                            format_const_value(db, &v.value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{vis}{name} = enum{exhaustive} {{ {variants} }}")
            }
            DefinitionKind::Macro(m) => {
                let params = m
                    .params
                    .iter()
                    .map(|p| p.name.as_str(db))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "{vis}macro {name}!({params}) = {}",
                    format_combinator(db, &m.body)
                )
            }
            DefinitionKind::Const { ty, value } => {
                format!(
                    "{vis}const {name}: {} = {}",
                    format_combinator(db, ty),
                    format_const_value(db, value)
                )
            }
            DefinitionKind::Endianness(e) => {
                let end = match e {
                    Endianness::Little => "LITTLE",
                    Endianness::Big => "BIG",
                };
                format!("!{end}_ENDIAN")
            }
        }
    }

    fn format_combinator<'db>(db: &'db dyn Db, comb: &Combinator<'db>) -> String {
        match comb {
            Combinator::Int(t) => format!("{t:?}").to_lowercase(),
            Combinator::ConstrainedInt { int_type, .. } => {
                format!("{int_type:?} | ...").to_lowercase()
            }
            Combinator::Reference { name, args, .. } => {
                let name_str = name.as_str(db);
                if args.is_empty() {
                    name_str.to_string()
                } else {
                    let args_str = args
                        .iter()
                        .map(|a| format!("@{}", a.as_str(db)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{name_str}({args_str})")
                }
            }
            Combinator::ConstrainedReference { name, .. } => {
                format!("{} | ...", name.as_str(db))
            }
            Combinator::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .map(|f| {
                        let prefix = if f.is_const {
                            "const "
                        } else if f.is_dependent {
                            "@"
                        } else {
                            ""
                        };
                        format!(
                            "{prefix}{}: {}",
                            f.name.as_str(db),
                            format_combinator(db, &f.ty)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {fields} }}")
            }
            Combinator::Choice(c) => {
                let disc = c
                    .discriminant
                    .as_ref()
                    .map(|d| format!("(@{})", d.as_str(db)))
                    .unwrap_or_default();
                let arms = c
                    .arms
                    .iter()
                    .map(|a| format!("{} => ...", format_pattern(db, &a.pattern)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("choose{disc} {{ {arms} }}")
            }
            Combinator::Array(a) => {
                format!(
                    "[{}; {}]",
                    format_combinator(db, &a.element),
                    format_length_expr(db, &a.length)
                )
            }
            Combinator::Vec(v) => {
                format!("Vec<{}>", format_combinator(db, &v.element))
            }
            Combinator::Option(o) => {
                format!("Option<{}>", format_combinator(db, &o.element))
            }
            Combinator::Wrap(w) => {
                let args = w
                    .args
                    .iter()
                    .map(|a| match a {
                        WrapArg::Combinator(c) => format_combinator(db, c),
                        WrapArg::ConstInt { ty, value } => {
                            format!("{ty:?} = {}", format_const_value(db, value)).to_lowercase()
                        }
                        WrapArg::ConstEnum { ty, variant } => {
                            format!("{} = {}", ty.as_str(db), variant.as_str(db))
                        }
                        WrapArg::ConstBytes {
                            element_ty, length, ..
                        } => format!("[{element_ty:?}; {length}] = [...]").to_lowercase(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("wrap({args})")
            }
            Combinator::Tail => "Tail".to_string(),
            Combinator::Bind { inner, target } => {
                format!(
                    "{} >>= {}",
                    format_combinator(db, inner),
                    format_combinator(db, target)
                )
            }
            Combinator::MacroInvocation { name, args, .. } => {
                let args_str = args
                    .iter()
                    .map(|a| format_combinator(db, a))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}!({args_str})", name.as_str(db))
            }
            Combinator::Error => "ERROR".to_string(),
        }
    }

    fn format_pattern<'db>(db: &'db dyn Db, pattern: &ChoicePattern<'db>) -> String {
        match pattern {
            ChoicePattern::Variant(v) => v.as_str(db).to_string(),
            ChoicePattern::Wildcard => "_".to_string(),
            ChoicePattern::Constraint(_) => "constraint".to_string(),
            ChoicePattern::Array(vals) => {
                let v = vals
                    .iter()
                    .map(|c| format_const_value(db, c))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{v}]")
            }
        }
    }

    fn format_length_expr<'db>(db: &'db dyn Db, expr: &LengthExpr<'db>) -> String {
        let mut result = String::new();
        for (i, term) in expr.terms.iter().enumerate() {
            if i > 0 {
                let op = match expr.ops.get(i - 1) {
                    Some(LengthOp::Add) => " + ",
                    Some(LengthOp::Sub) => " - ",
                    _ => " ? ",
                };
                result.push_str(op);
            }
            result.push_str(&format_length_term(db, term));
        }
        result
    }

    fn format_length_term<'db>(db: &'db dyn Db, term: &LengthTerm<'db>) -> String {
        let mut result = String::new();
        for (i, atom) in term.atoms.iter().enumerate() {
            if i > 0 {
                let op = match term.ops.get(i - 1) {
                    Some(LengthOp::Mul) => " * ",
                    Some(LengthOp::Div) => " / ",
                    _ => " ? ",
                };
                result.push_str(op);
            }
            result.push_str(&format_length_atom(db, atom));
        }
        result
    }

    fn format_length_atom<'db>(db: &'db dyn Db, atom: &LengthAtom<'db>) -> String {
        match atom {
            LengthAtom::Const(v) => v.to_string(),
            LengthAtom::Param(p) => format!("@{}", p.as_str(db)),
            LengthAtom::SizeOf(target) => match target {
                SizeTarget::Type(t) => format!("|{t:?}|").to_lowercase(),
                SizeTarget::Named(n) => format!("|{}|", n.as_str(db)),
            },
            LengthAtom::Paren(e) => format!("({})", format_length_expr(db, e)),
        }
    }

    fn format_const_value<'db>(db: &'db dyn Db, val: &ConstValue<'db>) -> String {
        match val {
            ConstValue::Int(v) => v.to_string(),
            ConstValue::String(s) => format!("\"{}\"", s.as_str(db)),
        }
    }

    #[test]
    fn lower_simple_struct() {
        check(
            "packet = { field: u8, }\n",
            expect![[r#"packet = { field: u8 }"#]],
        );
    }

    #[test]
    fn lower_struct_with_const_field() {
        check(
            "two = { const a: u8 = 255, b: u16, }\n",
            expect![[r#"two = { const a: u8, b: u16 }"#]],
        );
    }

    #[test]
    fn lower_struct_with_dependent_field() {
        check(
            "msg = { @len: u16, data: [u8; @len], }\n",
            expect![[r#"msg = { @len: u16, data: [u8; @len] }"#]],
        );
    }

    #[test]
    fn lower_dotted_dependent_reference_uses_binding_name() {
        check(
            "msg = { @len: u16, data: [u8; @len.value], }\n",
            expect![[r#"msg = { @len: u16, data: [u8; @len] }"#]],
        );
    }

    #[test]
    fn lower_enum_definition() {
        check(
            "my_enum = enum { A = 0, B = 1, }\n",
            expect![[r#"my_enum = enum { A = 0, B = 1 }"#]],
        );
    }

    #[test]
    fn lower_choice_combinator() {
        check(
            "choice(@tag: u8) = choose(@tag) { 0 => u16, _ => u32, }\n",
            expect![[r#"choice(tag) = choose(@tag) { constraint => ..., _ => ... }"#]],
        );
    }

    #[test]
    fn lower_array_with_size_expr() {
        check("bytes = [u8; |u16|]\n", expect![[r#"bytes = [u8; |u16|]"#]]);
    }

    #[test]
    fn lower_bind_combinator() {
        check(
            "reinterp = [u8; 4] >>= header\n",
            expect![[r#"reinterp = [u8; 4] >>= header"#]],
        );
    }

    #[test]
    fn lower_vec_combinator() {
        check("items = Vec<u32>\n", expect![[r#"items = Vec<u32>"#]]);
    }

    #[test]
    fn lower_option_combinator() {
        check("opt = Option<u16>\n", expect![[r#"opt = Option<u16>"#]]);
    }

    #[test]
    fn lower_wrap_combinator() {
        check(
            "wrapped = wrap(u8 = 1, u16)\n",
            expect![[r#"wrapped = wrap(u8 = 1, u16)"#]],
        );
    }

    #[test]
    fn lower_visibility_modifiers() {
        check(
            "public pub_def = u8\nsecret sec_def = u16\n",
            expect![[r#"
                public pub_def = u8
                secret sec_def = u16"#]],
        );
    }

    #[test]
    fn lower_macro_definition() {
        check(
            "macro wrap_it!(item) = wrap(u8 = 1, item)\n",
            expect![[r#"macro wrap_it!(item) = wrap(u8 = 1, item)"#]],
        );
    }

    #[test]
    fn lower_macro_invocation() {
        check(
            "wrapped = wrap_it!(u32)\n",
            expect![[r#"wrapped = wrap_it!(u32)"#]],
        );
    }

    #[test]
    fn lower_constrained_int() {
        check(
            "bounded = u16 | { 1..100, 200..300 }\n",
            expect![[r#"bounded = u16 | ..."#]],
        );
    }

    #[test]
    fn lower_combinator_with_params() {
        check(
            "param_comb(@len: u16 | { 4..0xffff }) = [u8; @len]\n",
            expect![[r#"param_comb(len) = [u8; @len]"#]],
        );
    }

    #[test]
    fn lower_length_expr_with_arithmetic() {
        check(
            "arith(@a: u16, @b: u8) = [u8; @a - @b + 4]\n",
            expect![[r#"arith(a, b) = [u8; @a - @b + 4]"#]],
        );
    }

    #[test]
    fn lower_size_of_named() {
        check(
            "sized = [u8; |header|]\n",
            expect![[r#"sized = [u8; |header|]"#]],
        );
    }

    #[test]
    fn lower_choice_with_array_pattern() {
        check(
            "choice_arr = choose { [0, 1] => u8, [1, 0] => u16, }\n",
            expect![[r#"choice_arr = choose { [0, 1] => ..., [1, 0] => ... }"#]],
        );
    }

    #[test]
    fn lower_tail() {
        check("rest = Tail\n", expect![[r#"rest = Tail"#]]);
    }

    #[test]
    fn lower_reference_with_args() {
        check(
            "call = other(@foo, @bar)\n",
            expect![[r#"call = other(@foo, @bar)"#]],
        );
    }
}
