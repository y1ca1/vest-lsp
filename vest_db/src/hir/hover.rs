//! Hover information and wire-length computation.

use std::collections::{HashMap, HashSet};

use crate::Db;

use super::symbols::symbol_occurrence_at_offset_in_hir;
use super::types::*;

type DefinitionLookup<'db, 'hir> = HashMap<Name<'db>, &'hir Definition<'db>>;

fn definition_lookup<'db, 'hir>(hir: &'hir FileHir<'db>) -> DefinitionLookup<'db, 'hir> {
    hir.definitions
        .iter()
        .map(|definition| (definition.name, definition))
        .collect()
}

fn definition_by_name<'db, 'hir>(
    definitions: &DefinitionLookup<'db, 'hir>,
    name: Name<'db>,
) -> Option<&'hir Definition<'db>> {
    definitions.get(&name).copied()
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WireVar<'db> {
    pub base: Name<'db>,
    pub fields: Vec<Name<'db>>,
}

impl<'db> WireVar<'db> {
    fn render(&self, db: &'db dyn Db) -> String {
        let mut rendered = format!("@{}", self.base.as_str(db));
        for field in &self.fields {
            rendered.push('.');
            rendered.push_str(field.as_str(db));
        }
        rendered
    }
}

impl std::fmt::Debug for WireVar<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WireVar(..)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WireOp {
    Add,
    Sub,
    Mul,
    Div,
}

impl WireOp {
    fn render(self) -> &'static str {
        match self {
            WireOp::Add => "+",
            WireOp::Sub => "-",
            WireOp::Mul => "*",
            WireOp::Div => "/",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            WireOp::Add | WireOp::Sub => 1,
            WireOp::Mul | WireOp::Div => 2,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum WireExpr<'db> {
    Const(u64),
    Var(WireVar<'db>),
    Binary {
        op: WireOp,
        lhs: Box<WireExpr<'db>>,
        rhs: Box<WireExpr<'db>>,
    },
}

impl<'db> WireExpr<'db> {
    fn constant(value: u64) -> Self {
        Self::Const(value)
    }

    fn as_const(&self) -> Option<u64> {
        match self {
            WireExpr::Const(value) => Some(*value),
            WireExpr::Var(_) | WireExpr::Binary { .. } => None,
        }
    }

    fn binary(op: WireOp, lhs: WireExpr<'db>, rhs: WireExpr<'db>) -> Self {
        match (op, lhs.as_const(), rhs.as_const()) {
            (WireOp::Add, Some(0), _) => rhs,
            (WireOp::Add, _, Some(0)) => lhs,
            (WireOp::Mul, Some(0), _) | (WireOp::Mul, _, Some(0)) => WireExpr::Const(0),
            (WireOp::Mul, Some(1), _) => rhs,
            (WireOp::Mul, _, Some(1)) => lhs,
            (WireOp::Sub, _, Some(0)) => lhs,
            (WireOp::Div, _, Some(1)) => lhs,
            (_, Some(left), Some(right)) => match op {
                WireOp::Add => left
                    .checked_add(right)
                    .map(WireExpr::Const)
                    .unwrap_or_else(|| WireExpr::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    }),
                WireOp::Sub => WireExpr::Const(left.saturating_sub(right)),
                WireOp::Mul => left
                    .checked_mul(right)
                    .map(WireExpr::Const)
                    .unwrap_or_else(|| WireExpr::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    }),
                WireOp::Div => {
                    if right == 0 {
                        WireExpr::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        }
                    } else {
                        WireExpr::Const(left / right)
                    }
                }
            },
            _ => WireExpr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        }
    }

    pub fn render(&self, db: &'db dyn Db) -> String {
        self.render_with_precedence(db, 0)
    }

    fn render_with_precedence(&self, db: &'db dyn Db, outer: u8) -> String {
        match self {
            WireExpr::Const(value) => value.to_string(),
            WireExpr::Var(var) => var.render(db),
            WireExpr::Binary { op, lhs, rhs } => {
                let precedence = op.precedence();
                let rendered = format!(
                    "{} {} {}",
                    lhs.render_with_precedence(db, precedence),
                    op.render(),
                    rhs.render_with_precedence(
                        db,
                        precedence + u8::from(matches!(op, WireOp::Sub | WireOp::Div))
                    ),
                );
                if precedence < outer {
                    format!("({rendered})")
                } else {
                    rendered
                }
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WireLength<'db> {
    pub exact: Option<WireExpr<'db>>,
    pub min: u64,
    pub max: Option<u64>,
}

impl<'db> WireLength<'db> {
    fn exact_const(value: u64) -> Self {
        Self {
            exact: Some(WireExpr::Const(value)),
            min: value,
            max: Some(value),
        }
    }

    fn unknown() -> Self {
        Self {
            exact: None,
            min: 0,
            max: None,
        }
    }

    pub fn markdown(&self, db: &'db dyn Db) -> String {
        let mut lines = Vec::new();

        match &self.exact {
            Some(expr) if expr.as_const() == self.max && self.max == Some(self.min) => {
                lines.push(format!("**Wire length:** `{}`", expr.render(db)));
            }
            Some(expr) => {
                lines.push(format!("**Wire length:** `{}`", expr.render(db)));
                if self.max != Some(self.min) {
                    lines.push(format!("**Min wire length:** `{}`", self.min));
                    lines.push(match self.max {
                        Some(max) => format!("**Max wire length:** `{max}`"),
                        None => "**Max wire length:** `unbounded`".to_string(),
                    });
                }
            }
            None => {
                lines.push(format!("**Min wire length:** `{}`", self.min));
                lines.push(match self.max {
                    Some(max) => format!("**Max wire length:** `{max}`"),
                    None => "**Max wire length:** `unbounded`".to_string(),
                });
            }
        }

        lines.join("\n")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoverKind {
    Format,
    Parameter,
    Field,
    Enum,
    EnumVariant,
    Constant,
    Endianness,
}

impl HoverKind {
    pub fn label(self) -> &'static str {
        match self {
            HoverKind::Format => "format",
            HoverKind::Parameter => "parameter",
            HoverKind::Field => "field",
            HoverKind::Enum => "enum",
            HoverKind::EnumVariant => "enum variant",
            HoverKind::Constant => "constant",
            HoverKind::Endianness => "endianness",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HoverInfo<'db> {
    pub range: Span,
    pub snippet_span: Span,
    pub kind: HoverKind,
    pub wire_length: Option<WireLength<'db>>,
}

#[derive(Clone, Copy)]
struct Bounds {
    min: u64,
    max: Option<u64>,
}

impl Bounds {
    fn exact(value: u64) -> Self {
        Self {
            min: value,
            max: Some(value),
        }
    }
}

#[derive(Clone)]
struct ExprSummary<'db> {
    exact: Option<WireExpr<'db>>,
    bounds: Bounds,
}

#[derive(Clone, Copy)]
struct LocalBinding<'db, 'hir> {
    name: Name<'db>,
    combinator: &'hir Combinator<'db>,
}

pub fn compute_wire_length<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    name: Name<'db>,
) -> Option<WireLength<'db>> {
    let definitions = definition_lookup(hir);
    let definition = definition_by_name(&definitions, name)?;
    let mut visiting = HashSet::new();
    wire_length_for_definition(db, hir, &definitions, definition, &mut visiting)
}

pub fn hover_info_in_hir<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    byte_offset: usize,
) -> Option<HoverInfo<'db>> {
    let definitions = definition_lookup(hir);
    let occurrence = symbol_occurrence_at_offset_in_hir(hir, byte_offset)?;
    match occurrence.symbol {
        SymbolId::TopLevel { name, .. } => {
            let definition = definition_by_name(&definitions, name)?;
            let mut visiting = HashSet::new();
            Some(HoverInfo {
                range: occurrence.span,
                snippet_span: definition.span,
                kind: hover_kind_for_definition(definition),
                wire_length: wire_length_for_definition(
                    db,
                    hir,
                    &definitions,
                    definition,
                    &mut visiting,
                ),
            })
        }
        SymbolId::Param {
            owner, declaration, ..
        } => {
            let definition = definition_by_name(&definitions, owner)?;
            let DefinitionKind::Combinator { params, .. } = &definition.kind else {
                return None;
            };
            let index = params.iter().position(|param| param.span == declaration)?;
            let param = &params[index];
            let locals = params
                .iter()
                .take(index)
                .map(|param| LocalBinding {
                    name: param.name,
                    combinator: &param.ty,
                })
                .collect::<Vec<_>>();
            Some(HoverInfo {
                range: occurrence.span,
                snippet_span: param.full_span,
                kind: HoverKind::Parameter,
                wire_length: Some(wire_length_for_combinator(
                    db,
                    hir,
                    &definitions,
                    &param.ty,
                    &locals,
                    &mut HashSet::new(),
                )),
            })
        }
        SymbolId::Field {
            owner, declaration, ..
        } => {
            let definition = definition_by_name(&definitions, owner)?;
            let DefinitionKind::Combinator { params, body } = &definition.kind else {
                return None;
            };
            let Combinator::Struct(structure) = body else {
                return None;
            };
            let index = structure
                .fields
                .iter()
                .position(|field| field.span == declaration)?;
            let field = &structure.fields[index];
            let mut locals = params
                .iter()
                .map(|param| LocalBinding {
                    name: param.name,
                    combinator: &param.ty,
                })
                .collect::<Vec<_>>();
            for previous in structure.fields.iter().take(index) {
                if previous.is_dependent {
                    locals.push(LocalBinding {
                        name: previous.name,
                        combinator: &previous.ty,
                    });
                }
            }
            Some(HoverInfo {
                range: occurrence.span,
                snippet_span: field.full_span,
                kind: HoverKind::Field,
                wire_length: Some(wire_length_for_combinator(
                    db,
                    hir,
                    &definitions,
                    &field.ty,
                    &locals,
                    &mut HashSet::new(),
                )),
            })
        }
        SymbolId::EnumVariant {
            owner, declaration, ..
        } => {
            let definition = definition_by_name(&definitions, owner)?;
            let DefinitionKind::Enum(enum_def) = &definition.kind else {
                return None;
            };
            let variant = enum_def
                .variants
                .iter()
                .find(|variant| variant.span == declaration)?;
            Some(HoverInfo {
                range: occurrence.span,
                snippet_span: variant.full_span,
                kind: HoverKind::EnumVariant,
                wire_length: Some(enum_wire_length(enum_def)),
            })
        }
    }
}

fn hover_kind_for_definition(definition: &Definition<'_>) -> HoverKind {
    match definition.kind {
        DefinitionKind::Combinator { .. } => HoverKind::Format,
        DefinitionKind::Enum(_) => HoverKind::Enum,
        DefinitionKind::Const { .. } => HoverKind::Constant,
        DefinitionKind::Endianness(_) => HoverKind::Endianness,
    }
}

fn wire_length_for_definition<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    definition: &Definition<'db>,
    visiting: &mut HashSet<Name<'db>>,
) -> Option<WireLength<'db>> {
    match &definition.kind {
        DefinitionKind::Combinator { params, body } => {
            let locals = params
                .iter()
                .map(|param| LocalBinding {
                    name: param.name,
                    combinator: &param.ty,
                })
                .collect::<Vec<_>>();
            Some(wire_length_for_combinator(
                db,
                hir,
                definitions,
                body,
                &locals,
                visiting,
            ))
        }
        DefinitionKind::Enum(enum_def) => Some(enum_wire_length(enum_def)),
        DefinitionKind::Const { ty, .. } => Some(wire_length_for_combinator(
            db,
            hir,
            definitions,
            ty,
            &[],
            visiting,
        )),
        DefinitionKind::Endianness(_) => None,
    }
}

fn wire_length_for_combinator<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    combinator: &Combinator<'db>,
    locals: &[LocalBinding<'db, '_>],
    visiting: &mut HashSet<Name<'db>>,
) -> WireLength<'db> {
    match combinator {
        Combinator::Int(int_type) | Combinator::ConstrainedInt { int_type, .. } => {
            prim_byte_size(*int_type)
                .map(WireLength::exact_const)
                .unwrap_or_else(|| WireLength {
                    exact: None,
                    min: 1,
                    max: Some(variable_prim_max(*int_type)),
                })
        }
        Combinator::Reference { name, args, .. } => {
            wire_length_for_reference(db, hir, definitions, *name, args, locals, visiting)
        }
        Combinator::ConstrainedReference { name, .. } => {
            wire_length_for_reference(db, hir, definitions, *name, &[], locals, visiting)
        }
        Combinator::Struct(structure) => {
            let mut exact = Some(WireExpr::constant(0));
            let mut min = 0u64;
            let mut max = Some(0u64);
            let mut local_scope = locals.to_vec();

            for field in &structure.fields {
                let field_length = wire_length_for_combinator(
                    db,
                    hir,
                    definitions,
                    &field.ty,
                    &local_scope,
                    visiting,
                );
                exact = combine_expr(WireOp::Add, exact, field_length.exact);
                min = min.saturating_add(field_length.min);
                max =
                    combine_optional(max, field_length.max, |left, right| left.checked_add(right));

                if field.is_dependent {
                    local_scope.push(LocalBinding {
                        name: field.name,
                        combinator: &field.ty,
                    });
                }
            }

            WireLength { exact, min, max }
        }
        Combinator::Choice(choice) => {
            let mut iter = choice.arms.iter().map(|arm| {
                wire_length_for_combinator(db, hir, definitions, &arm.body, locals, visiting)
            });
            let Some(first) = iter.next() else {
                return WireLength::exact_const(0);
            };

            let mut exact = first.exact.clone();
            let mut min = first.min;
            let mut max = first.max;
            for branch in iter {
                min = min.min(branch.min);
                max = combine_optional_max(max, branch.max);
                if exact != branch.exact {
                    exact = None;
                }
            }

            WireLength { exact, min, max }
        }
        Combinator::Array(array) => {
            let elem =
                wire_length_for_combinator(db, hir, definitions, &array.element, locals, visiting);
            let length = length_expr_summary(db, hir, definitions, &array.length, locals, visiting);
            WireLength {
                exact: combine_expr(WireOp::Mul, length.exact, elem.exact),
                min: multiply_bounds(length.bounds.min, elem.min).unwrap_or(0),
                max: combine_optional(length.bounds.max, elem.max, |left, right| {
                    left.checked_mul(right)
                }),
            }
        }
        Combinator::Vec(_) | Combinator::Tail => WireLength::unknown(),
        Combinator::Option(option) => {
            let elem =
                wire_length_for_combinator(db, hir, definitions, &option.element, locals, visiting);
            WireLength {
                exact: None,
                min: 0,
                max: elem.max,
            }
        }
        Combinator::Wrap(wrap) => {
            let mut exact = Some(WireExpr::constant(0));
            let mut min = 0u64;
            let mut max = Some(0u64);

            for arg in &wrap.args {
                let arg_length = match arg {
                    WrapArg::Combinator(combinator) => wire_length_for_combinator(
                        db,
                        hir,
                        definitions,
                        combinator,
                        locals,
                        visiting,
                    ),
                    WrapArg::ConstInt { ty, .. } => prim_byte_size(*ty)
                        .map(WireLength::exact_const)
                        .unwrap_or_else(WireLength::unknown),
                    WrapArg::ConstEnum { ty, .. } => definition_by_name(definitions, ty.name)
                        .and_then(|definition| match definition.kind {
                            DefinitionKind::Enum(ref enum_def) => Some(enum_wire_length(enum_def)),
                            _ => None,
                        })
                        .unwrap_or_else(WireLength::unknown),
                    WrapArg::ConstBytes { length, .. } => WireLength::exact_const(*length),
                };

                exact = combine_expr(WireOp::Add, exact, arg_length.exact);
                min = min.saturating_add(arg_length.min);
                max = combine_optional(max, arg_length.max, |left, right| left.checked_add(right));
            }

            WireLength { exact, min, max }
        }
        Combinator::Bind { inner, .. } => {
            wire_length_for_combinator(db, hir, definitions, inner, locals, visiting)
        }
        Combinator::Error => WireLength::unknown(),
    }
}

fn wire_length_for_reference<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    name: Name<'db>,
    args: &[NameRef<'db>],
    locals: &[LocalBinding<'db, '_>],
    visiting: &mut HashSet<Name<'db>>,
) -> WireLength<'db> {
    let Some(definition) = definition_by_name(definitions, name) else {
        return WireLength::unknown();
    };

    match &definition.kind {
        DefinitionKind::Enum(enum_def) => enum_wire_length(enum_def),
        DefinitionKind::Const { ty, .. } => {
            wire_length_for_combinator(db, hir, definitions, ty, locals, visiting)
        }
        DefinitionKind::Combinator { params, body } => {
            if !visiting.insert(name) {
                return WireLength::unknown();
            }

            let mut callee_locals = Vec::new();
            for (param, arg) in params.iter().zip(args.iter()) {
                let combinator = resolve_local_binding(locals, arg.name)
                    .map(|binding| binding.combinator)
                    .unwrap_or(&param.ty);
                callee_locals.push(LocalBinding {
                    name: param.name,
                    combinator,
                });
            }

            let result =
                wire_length_for_combinator(db, hir, definitions, body, &callee_locals, visiting);
            visiting.remove(&name);
            result
        }
        DefinitionKind::Endianness(_) => WireLength::unknown(),
    }
}

fn length_expr_summary<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    expr: &LengthExpr<'db>,
    locals: &[LocalBinding<'db, '_>],
    visiting: &mut HashSet<Name<'db>>,
) -> ExprSummary<'db> {
    let mut iter = expr.terms.iter();
    let Some(first) = iter.next() else {
        return ExprSummary {
            exact: Some(WireExpr::constant(0)),
            bounds: Bounds::exact(0),
        };
    };

    let mut summary = length_term_summary(db, hir, definitions, first, locals, visiting);
    for (op, term) in expr.ops.iter().zip(iter) {
        let rhs = length_term_summary(db, hir, definitions, term, locals, visiting);
        summary = combine_summary(summary, *op, rhs);
    }
    summary
}

fn length_term_summary<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    term: &LengthTerm<'db>,
    locals: &[LocalBinding<'db, '_>],
    visiting: &mut HashSet<Name<'db>>,
) -> ExprSummary<'db> {
    let mut iter = term.atoms.iter();
    let Some(first) = iter.next() else {
        return ExprSummary {
            exact: Some(WireExpr::constant(1)),
            bounds: Bounds::exact(1),
        };
    };

    let mut summary = length_atom_summary(db, hir, definitions, first, locals, visiting);
    for (op, atom) in term.ops.iter().zip(iter) {
        let rhs = length_atom_summary(db, hir, definitions, atom, locals, visiting);
        summary = combine_summary(summary, *op, rhs);
    }
    summary
}

fn length_atom_summary<'db>(
    db: &'db dyn Db,
    hir: &FileHir<'db>,
    definitions: &DefinitionLookup<'db, '_>,
    atom: &LengthAtom<'db>,
    locals: &[LocalBinding<'db, '_>],
    visiting: &mut HashSet<Name<'db>>,
) -> ExprSummary<'db> {
    match atom {
        LengthAtom::Const(value) => ExprSummary {
            exact: Some(WireExpr::constant(*value)),
            bounds: Bounds::exact(*value),
        },
        LengthAtom::Param(param) => {
            let var = WireVar {
                base: param.name,
                fields: Vec::new(),
            };
            ExprSummary {
                exact: Some(WireExpr::Var(var)),
                bounds: resolve_value_bounds(hir, locals, param.name, &[]),
            }
        }
        LengthAtom::ProjectedParam { base, fields } => ExprSummary {
            exact: Some(WireExpr::Var(WireVar {
                base: base.name,
                fields: fields.clone(),
            })),
            bounds: resolve_value_bounds(hir, locals, base.name, fields),
        },
        LengthAtom::SizeOf(SizeTarget::Type(int_type)) => prim_byte_size(*int_type)
            .map(|bytes| ExprSummary {
                exact: Some(WireExpr::constant(bytes)),
                bounds: Bounds::exact(bytes),
            })
            .unwrap_or(ExprSummary {
                exact: None,
                bounds: Bounds { min: 0, max: None },
            }),
        LengthAtom::SizeOf(SizeTarget::Named(name)) => {
            let wire = definition_by_name(definitions, name.name)
                .and_then(|definition| {
                    wire_length_for_definition(db, hir, definitions, definition, visiting)
                })
                .unwrap_or_else(WireLength::unknown);
            ExprSummary {
                exact: wire.exact,
                bounds: Bounds {
                    min: wire.min,
                    max: wire.max,
                },
            }
        }
        LengthAtom::Paren(inner) => {
            length_expr_summary(db, hir, definitions, inner, locals, visiting)
        }
    }
}

fn combine_summary<'db>(
    lhs: ExprSummary<'db>,
    op: LengthOp,
    rhs: ExprSummary<'db>,
) -> ExprSummary<'db> {
    let op = match op {
        LengthOp::Add => WireOp::Add,
        LengthOp::Sub => WireOp::Sub,
        LengthOp::Mul => WireOp::Mul,
        LengthOp::Div => WireOp::Div,
    };

    let exact = combine_expr(op, lhs.exact, rhs.exact);
    let bounds = combine_bounds(lhs.bounds, op, rhs.bounds);
    ExprSummary { exact, bounds }
}

fn combine_bounds(lhs: Bounds, op: WireOp, rhs: Bounds) -> Bounds {
    match op {
        WireOp::Add => Bounds {
            min: lhs.min.saturating_add(rhs.min),
            max: combine_optional(lhs.max, rhs.max, |left, right| left.checked_add(right)),
        },
        WireOp::Sub => Bounds {
            min: lhs.min.saturating_sub(rhs.max.unwrap_or(u64::MAX)),
            max: lhs.max.map(|left| left.saturating_sub(rhs.min)),
        },
        WireOp::Mul => Bounds {
            min: multiply_bounds(lhs.min, rhs.min).unwrap_or(0),
            max: combine_optional(lhs.max, rhs.max, |left, right| left.checked_mul(right)),
        },
        WireOp::Div => {
            if rhs.min == 0 {
                Bounds { min: 0, max: None }
            } else {
                Bounds {
                    min: rhs.max.map(|max| lhs.min / max).unwrap_or(0),
                    max: lhs.max.map(|left| left / rhs.min),
                }
            }
        }
    }
}

fn combine_expr<'db>(
    op: WireOp,
    lhs: Option<WireExpr<'db>>,
    rhs: Option<WireExpr<'db>>,
) -> Option<WireExpr<'db>> {
    Some(WireExpr::binary(op, lhs?, rhs?))
}

fn combine_optional(
    lhs: Option<u64>,
    rhs: Option<u64>,
    f: impl FnOnce(u64, u64) -> Option<u64>,
) -> Option<u64> {
    f(lhs?, rhs?)
}

fn combine_optional_max(lhs: Option<u64>, rhs: Option<u64>) -> Option<u64> {
    match (lhs, rhs) {
        (Some(left), Some(right)) => Some(left.max(right)),
        _ => None,
    }
}

fn multiply_bounds(lhs: u64, rhs: u64) -> Option<u64> {
    lhs.checked_mul(rhs)
}

fn resolve_value_bounds<'db>(
    hir: &FileHir<'db>,
    locals: &[LocalBinding<'db, '_>],
    base: Name<'db>,
    fields: &[Name<'db>],
) -> Bounds {
    let Some(binding) = resolve_local_binding(locals, base) else {
        return Bounds { min: 0, max: None };
    };

    let mut current = binding.combinator;
    for field_name in fields {
        let Some((next, is_dependent)) = resolve_struct_field(hir, current, *field_name) else {
            return Bounds { min: 0, max: None };
        };
        if !is_dependent {
            return Bounds { min: 0, max: None };
        }
        current = next;
    }

    value_bounds_for_combinator(hir, current, &mut HashSet::new())
}

fn resolve_local_binding<'db, 'hir>(
    locals: &[LocalBinding<'db, 'hir>],
    name: Name<'db>,
) -> Option<LocalBinding<'db, 'hir>> {
    locals
        .iter()
        .rev()
        .find(|binding| binding.name == name)
        .copied()
}

fn resolve_struct_field<'db, 'hir>(
    hir: &'hir FileHir<'db>,
    combinator: &'hir Combinator<'db>,
    field_name: Name<'db>,
) -> Option<(&'hir Combinator<'db>, bool)> {
    match combinator {
        Combinator::Struct(structure) => structure
            .fields
            .iter()
            .find(|field| field.name == field_name)
            .map(|field| (&field.ty, field.is_dependent)),
        Combinator::Reference { name, args, .. } if args.is_empty() => {
            let definition = definition_ref_in_hir(hir, *name)?;
            let DefinitionKind::Combinator { body, .. } = &definition.kind else {
                return None;
            };
            resolve_struct_field(hir, body, field_name)
        }
        _ => None,
    }
}

fn definition_ref_in_hir<'db, 'hir>(
    hir: &'hir FileHir<'db>,
    name: Name<'db>,
) -> Option<&'hir Definition<'db>> {
    hir.definitions
        .iter()
        .find(|definition| definition.name == name)
}

fn value_bounds_for_combinator<'db>(
    hir: &FileHir<'db>,
    combinator: &Combinator<'db>,
    visiting: &mut HashSet<Name<'db>>,
) -> Bounds {
    match combinator {
        Combinator::Int(int_type) => int_bounds(*int_type),
        Combinator::ConstrainedInt {
            int_type,
            constraint,
        } => bounds_for_constraint(*int_type, constraint),
        Combinator::Reference { name, args, .. } => {
            let Some(definition) = definition_ref_in_hir(hir, *name) else {
                return Bounds { min: 0, max: None };
            };
            match &definition.kind {
                DefinitionKind::Enum(enum_def) => enum_value_bounds(enum_def),
                DefinitionKind::Const { .. } => Bounds { min: 0, max: None },
                DefinitionKind::Combinator { body, .. } => {
                    if !args.is_empty() || !visiting.insert(*name) {
                        return Bounds { min: 0, max: None };
                    }
                    let bounds = value_bounds_for_combinator(hir, body, visiting);
                    visiting.remove(name);
                    bounds
                }
                DefinitionKind::Endianness(_) => Bounds { min: 0, max: None },
            }
        }
        _ => Bounds { min: 0, max: None },
    }
}

fn bounds_for_constraint<'db>(int_type: IntType, constraint: &IntConstraint<'db>) -> Bounds {
    let type_bounds = int_bounds(int_type);
    let mut intervals = constraint
        .elements
        .iter()
        .filter_map(|element| constraint_interval(int_type, element))
        .collect::<Vec<_>>();

    if intervals.is_empty() {
        return type_bounds;
    }

    merge_intervals(&mut intervals);

    if !constraint.negated {
        return Bounds {
            min: intervals
                .first()
                .map(|interval| interval.0)
                .unwrap_or(type_bounds.min),
            max: intervals.last().map(|interval| interval.1),
        };
    }

    let upper = type_bounds.max.unwrap_or(u64::MAX);

    let min = if intervals[0].0 > type_bounds.min {
        type_bounds.min
    } else {
        intervals
            .iter()
            .find_map(|interval| {
                interval
                    .1
                    .checked_add(1)
                    .filter(|candidate| *candidate <= upper)
            })
            .unwrap_or(type_bounds.min)
    };

    let max = if intervals.last().is_some_and(|interval| interval.1 < upper) {
        Some(upper)
    } else {
        intervals
            .iter()
            .rev()
            .find_map(|interval| interval.0.checked_sub(1))
    };

    Bounds { min, max }
}

fn constraint_interval<'db>(
    int_type: IntType,
    element: &ConstraintElement<'db>,
) -> Option<(u64, u64)> {
    let full = int_bounds(int_type);
    match element {
        ConstraintElement::Single {
            value: ConstValue::Int(value),
            ..
        } if *value <= full.max? => Some((*value, *value)),
        ConstraintElement::Range { start, end, .. } => {
            let start = start
                .as_ref()
                .and_then(ConstValue::as_int)
                .unwrap_or(full.min);
            let end = end
                .as_ref()
                .and_then(ConstValue::as_int)
                .unwrap_or(full.max?);
            if start <= end && end <= full.max? {
                Some((start, end))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn merge_intervals(intervals: &mut Vec<(u64, u64)>) {
    intervals.sort_by_key(|interval| interval.0);
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(intervals.len());
    for (start, end) in intervals.drain(..) {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= last_end.saturating_add(1)
        {
            *last_end = (*last_end).max(end);
        } else {
            merged.push((start, end));
        }
    }
    *intervals = merged;
}

fn enum_wire_length<'db>(enum_def: &EnumDef<'db>) -> WireLength<'db> {
    prim_byte_size(enum_repr(enum_def))
        .map(WireLength::exact_const)
        .unwrap_or_else(WireLength::unknown)
}

fn enum_value_bounds(enum_def: &EnumDef<'_>) -> Bounds {
    if enum_def.is_exhaustive {
        let mut values = enum_def
            .variants
            .iter()
            .filter_map(|variant| variant.value.as_int());
        let Some(first) = values.next() else {
            return Bounds::exact(0);
        };
        let (min, max) = values.fold((first, first), |(min, max), value| {
            (min.min(value), max.max(value))
        });
        Bounds {
            min,
            max: Some(max),
        }
    } else {
        int_bounds(enum_repr(enum_def))
    }
}

fn enum_repr(enum_def: &EnumDef<'_>) -> IntType {
    if let Some(repr) = enum_def.repr_type {
        return repr;
    }

    if let Some(repr) = enum_def
        .variants
        .iter()
        .find_map(|variant| variant.repr_type)
    {
        return repr;
    }

    let max_value = enum_def
        .variants
        .iter()
        .filter_map(|variant| variant.value.as_int())
        .max()
        .unwrap_or(0);

    if max_value <= u8::MAX as u64 {
        IntType::U8
    } else if max_value <= u16::MAX as u64 {
        IntType::U16
    } else if max_value <= u32::MAX as u64 {
        IntType::U32
    } else {
        IntType::U64
    }
}

fn int_bounds(int_type: IntType) -> Bounds {
    Bounds {
        min: 0,
        max: Some(int_max(int_type)),
    }
}

fn int_max(int_type: IntType) -> u64 {
    match int_type {
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
    }
}

fn prim_byte_size(int_type: IntType) -> Option<u64> {
    match int_type {
        IntType::U8 | IntType::I8 => Some(1),
        IntType::U16 | IntType::I16 => Some(2),
        IntType::U24 | IntType::I24 => Some(3),
        IntType::U32 | IntType::I32 => Some(4),
        IntType::U64 | IntType::I64 => Some(8),
        IntType::BtcVarint | IntType::Uleb128 => None,
    }
}

fn variable_prim_max(int_type: IntType) -> u64 {
    match int_type {
        IntType::BtcVarint => 9,
        IntType::Uleb128 => 10,
        _ => prim_byte_size(int_type).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use crate::{Database, SourceFile};

    use super::*;
    use crate::hir::lower_to_hir;

    fn with_hir(source: &str, test: impl for<'db> FnOnce(&'db Database, &FileHir<'db>)) {
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 1, source.to_string());
        let hir = lower_to_hir(&db, file);
        test(&db, &hir);
    }

    #[test]
    fn compute_wire_length_for_primitive_definition_is_exact() {
        with_hir("msg = u16\n", |db, hir| {
            let wire = compute_wire_length(db, hir, Name::new(db, "msg".to_string()))
                .expect("wire length should exist");

            assert_eq!(wire.exact, Some(WireExpr::Const(2)));
            assert_eq!(wire.min, 2);
            assert_eq!(wire.max, Some(2));
        });
    }

    #[test]
    fn compute_wire_length_for_dependent_struct_tracks_expr_and_bounds() {
        with_hir(
            "msg = {\n    @len: u8 | 0..0xf0,\n    payload: [u8; @len],\n}\n",
            |db, hir| {
                let wire = compute_wire_length(db, hir, Name::new(db, "msg".to_string()))
                    .expect("wire length should exist");

                assert_eq!(
                    wire.exact
                        .expect("exact expression should exist")
                        .render(db),
                    "1 + @len"
                );
                assert_eq!(wire.min, 1);
                assert_eq!(wire.max, Some(241));
            },
        );
    }

    #[test]
    fn compute_wire_length_for_choice_uses_branch_range() {
        with_hir(
            "msg(@tag: u8) = choose(@tag) { 0 => u8, _ => u16, }\n",
            |db, hir| {
                let wire = compute_wire_length(db, hir, Name::new(db, "msg".to_string()))
                    .expect("wire length should exist");

                assert!(wire.exact.is_none());
                assert_eq!(wire.min, 1);
                assert_eq!(wire.max, Some(2));
            },
        );
    }

    #[test]
    fn division_bounds_with_unbounded_divisor_has_zero_min() {
        let bounds = combine_bounds(
            Bounds {
                min: 10,
                max: Some(100),
            },
            WireOp::Div,
            Bounds { min: 2, max: None },
        );

        assert_eq!(bounds.min, 0);
        assert_eq!(bounds.max, Some(50));
    }

    #[test]
    fn hover_info_for_field_uses_field_definition_not_shadowed_top_level_name() {
        let source = "field = u8\npacket = { field: u16, }\n";
        let offset = source.rfind("field").expect("field label should exist");
        with_hir(source, |db, hir| {
            let hover = hover_info_in_hir(db, hir, offset).expect("hover should exist");

            assert_eq!(hover.kind, HoverKind::Field);
            assert_eq!(
                hover.wire_length.expect("wire length").exact,
                Some(WireExpr::Const(2))
            );
        });
    }

    #[test]
    fn hover_info_for_reference_uses_definition_span() {
        let source = "other = u8\npacket = { field: other, }\n";
        let offset = source.rfind("other").expect("reference should exist");
        with_hir(source, |db, hir| {
            let hover = hover_info_in_hir(db, hir, offset).expect("hover should exist");

            assert_eq!(hover.kind, HoverKind::Format);
            assert_eq!(hover.snippet_span, Span::new(0, 10));
            assert_eq!(
                hover.wire_length.expect("wire length").exact,
                Some(WireExpr::Const(1))
            );
        });
    }
}
