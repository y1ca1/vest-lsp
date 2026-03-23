//! Symbol table and resolution queries.

use crate::hir::types::*;
use crate::{Db, SourceFile};

use super::lower::lower_to_hir;

/// Resolve a symbol by name within a file.
///
/// Returns the definition if found, or None if the symbol is not defined.
pub fn resolve_symbol<'db>(
    db: &'db dyn Db,
    source: SourceFile,
    name: Name<'db>,
) -> Option<Definition<'db>> {
    let hir = lower_to_hir(db, source);
    resolve_symbol_in_hir(&hir, name)
}

/// Resolve a symbol by name from an already-lowered HIR.
pub fn resolve_symbol_in_hir<'db>(hir: &FileHir<'db>, name: Name<'db>) -> Option<Definition<'db>> {
    hir.definitions.iter().find(|def| def.name == name).cloned()
}

/// Resolve a local symbol within a combinator's scope.
///
/// This handles dependent variables (@field) and parameter references.
/// Returns the span of the definition if found.
pub fn resolve_local_symbol<'db>(
    db: &'db dyn Db,
    def: &Definition<'db>,
    name: Name<'db>,
) -> Option<Span> {
    let name_str = name.as_str(db);

    match &def.kind {
        DefinitionKind::Combinator { params, body } => {
            // Check parameters first
            for param in params {
                if param.name.as_str(db) == name_str {
                    return Some(param.span);
                }
            }

            // Check struct fields if body is a struct
            if let Combinator::Struct(s) = body {
                for field in &s.fields {
                    if field.name.as_str(db) == name_str {
                        return Some(field.span);
                    }
                }
            }

            None
        }
        DefinitionKind::Enum(e) => {
            for variant in &e.variants {
                if variant.name.as_str(db) == name_str {
                    return Some(variant.span);
                }
            }
            None
        }
        _ => None,
    }
}

/// Find the declaration directly under the cursor within a definition.
pub fn declaration_at_offset<'db>(def: &Definition<'db>, byte_offset: usize) -> Option<Span> {
    if def.name_span.contains(byte_offset) {
        return Some(def.name_span);
    }

    match &def.kind {
        DefinitionKind::Combinator { params, body } => {
            for param in params {
                if param.span.contains(byte_offset) {
                    return Some(param.span);
                }
            }

            if let Combinator::Struct(s) = body {
                for field in &s.fields {
                    if field.span.contains(byte_offset) {
                        return Some(field.span);
                    }
                }
            }

            None
        }
        DefinitionKind::Enum(e) => e
            .variants
            .iter()
            .find(|variant| variant.span.contains(byte_offset))
            .map(|variant| variant.span),
        _ => None,
    }
}

/// Find all definitions in a file.
pub fn file_definitions<'db>(db: &'db dyn Db, source: SourceFile) -> Vec<Definition<'db>> {
    let hir = lower_to_hir(db, source);
    hir.definitions.clone()
}

/// Find the definition at a given byte offset.
pub fn definition_at_offset<'db>(
    db: &'db dyn Db,
    source: SourceFile,
    byte_offset: usize,
) -> Option<Definition<'db>> {
    let hir = lower_to_hir(db, source);
    definition_at_offset_in_hir(&hir, byte_offset)
}

/// Find the enclosing top-level definition at a given byte offset from existing HIR.
pub fn definition_at_offset_in_hir<'db>(
    hir: &FileHir<'db>,
    byte_offset: usize,
) -> Option<Definition<'db>> {
    hir.definitions
        .iter()
        .find(|def| def.span.contains(byte_offset))
        .cloned()
}

/// Resolve the symbol occurrence under a byte offset.
pub fn symbol_at_offset<'db>(
    db: &'db dyn Db,
    source: SourceFile,
    byte_offset: usize,
) -> Option<SymbolId<'db>> {
    let hir = lower_to_hir(db, source);
    symbol_at_offset_in_hir(&hir, byte_offset)
}

/// Resolve the symbol occurrence under a byte offset from existing HIR.
pub fn symbol_at_offset_in_hir<'db>(
    hir: &FileHir<'db>,
    byte_offset: usize,
) -> Option<SymbolId<'db>> {
    collect_symbol_occurrences(hir)
        .into_iter()
        .find(|occurrence| occurrence.span.contains(byte_offset))
        .map(|occurrence| occurrence.symbol)
}

/// Collect all references for a symbol within a file.
pub fn references_for_symbol<'db>(
    db: &'db dyn Db,
    source: SourceFile,
    symbol: SymbolId<'db>,
    include_declaration: bool,
) -> Vec<SymbolOccurrence<'db>> {
    let hir = lower_to_hir(db, source);
    references_for_symbol_in_hir(&hir, symbol, include_declaration)
}

/// Collect all references for a symbol from existing HIR.
pub fn references_for_symbol_in_hir<'db>(
    hir: &FileHir<'db>,
    symbol: SymbolId<'db>,
    include_declaration: bool,
) -> Vec<SymbolOccurrence<'db>> {
    collect_symbol_occurrences(hir)
        .into_iter()
        .filter(|occurrence| occurrence.symbol == symbol)
        .filter(|occurrence| {
            include_declaration || occurrence.kind != SymbolOccurrenceKind::Declaration
        })
        .collect()
}

/// Collect all symbol references used within a combinator.
pub fn collect_references<'db>(comb: &Combinator<'db>) -> Vec<Name<'db>> {
    let mut refs = Vec::new();
    collect_refs_recursive(comb, &mut refs);
    refs
}

fn collect_refs_recursive<'db>(comb: &Combinator<'db>, refs: &mut Vec<Name<'db>>) {
    match comb {
        Combinator::Reference { name, args, .. } => {
            refs.push(*name);
            refs.extend(args.iter().map(|arg| arg.name));
        }
        Combinator::ConstrainedReference {
            name, constraint, ..
        } => {
            refs.push(*name);
            refs.extend(constraint.iter().map(|item| item.name));
        }
        Combinator::Struct(s) => {
            for field in &s.fields {
                collect_refs_recursive(&field.ty, refs);
            }
        }
        Combinator::Choice(c) => {
            if let Some(d) = c.discriminant {
                refs.push(d.name);
            }
            for arm in &c.arms {
                if let ChoicePattern::Variant(v) = &arm.pattern {
                    refs.push(v.name);
                }
                collect_refs_recursive(&arm.body, refs);
            }
        }
        Combinator::Array(a) => {
            collect_refs_recursive(&a.element, refs);
            collect_length_refs(&a.length, refs);
        }
        Combinator::Vec(v) => {
            collect_refs_recursive(&v.element, refs);
        }
        Combinator::Option(o) => {
            collect_refs_recursive(&o.element, refs);
        }
        Combinator::Wrap(w) => {
            for arg in &w.args {
                match arg {
                    WrapArg::Combinator(c) => collect_refs_recursive(c, refs),
                    WrapArg::ConstEnum { ty, variant } => {
                        refs.push(ty.name);
                        refs.push(variant.name);
                    }
                    _ => {}
                }
            }
        }
        Combinator::Bind { inner, target, .. } => {
            collect_refs_recursive(inner, refs);
            collect_refs_recursive(target, refs);
        }
        Combinator::ConstrainedInt { .. }
        | Combinator::Int(_)
        | Combinator::Tail
        | Combinator::Error => {}
    }
}

fn collect_length_refs<'db>(expr: &LengthExpr<'db>, refs: &mut Vec<Name<'db>>) {
    for term in &expr.terms {
        for atom in &term.atoms {
            match atom {
                LengthAtom::Param(p) => refs.push(p.name),
                LengthAtom::ProjectedParam { base, .. } => refs.push(base.name),
                LengthAtom::SizeOf(SizeTarget::Named(n)) => refs.push(n.name),
                LengthAtom::Paren(e) => collect_length_refs(e, refs),
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy)]
struct LocalBinding<'db> {
    symbol: SymbolId<'db>,
    enum_ty: Option<Name<'db>>,
}

fn collect_symbol_occurrences<'db>(hir: &FileHir<'db>) -> Vec<SymbolOccurrence<'db>> {
    let mut occurrences = Vec::new();
    for definition in &hir.definitions {
        collect_definition_occurrences(hir, definition, &mut occurrences);
    }
    occurrences
}

fn collect_definition_occurrences<'db>(
    hir: &FileHir<'db>,
    definition: &Definition<'db>,
    occurrences: &mut Vec<SymbolOccurrence<'db>>,
) {
    let definition_symbol = definition.symbol_id();
    push_occurrence(
        occurrences,
        definition_symbol,
        definition.name_span,
        SymbolOccurrenceKind::Declaration,
    );

    match &definition.kind {
        DefinitionKind::Combinator { params, body } => {
            let mut visible = Vec::new();
            for param in params {
                let symbol = SymbolId::Param {
                    owner: definition.name,
                    name: param.name,
                    declaration: param.span,
                };
                push_occurrence(
                    occurrences,
                    symbol,
                    param.span,
                    SymbolOccurrenceKind::Declaration,
                );
                visible.push(LocalBinding {
                    symbol,
                    enum_ty: enum_type_for_combinator(hir, &param.ty),
                });
            }
            collect_combinator_occurrences(hir, definition.name, body, &mut visible, occurrences);
        }
        DefinitionKind::Enum(enum_def) => {
            for variant in &enum_def.variants {
                let symbol = SymbolId::EnumVariant {
                    owner: definition.name,
                    name: variant.name,
                    declaration: variant.span,
                };
                push_occurrence(
                    occurrences,
                    symbol,
                    variant.span,
                    SymbolOccurrenceKind::Declaration,
                );
            }
        }
        DefinitionKind::Const { ty, value } => {
            let mut visible = Vec::new();
            collect_combinator_occurrences(hir, definition.name, ty, &mut visible, occurrences);
            collect_const_value_occurrences(
                hir,
                Some(value),
                enum_type_for_combinator(hir, ty),
                occurrences,
            );
        }
        DefinitionKind::Endianness(_) => {}
    }
}

fn collect_combinator_occurrences<'db>(
    hir: &FileHir<'db>,
    owner: Name<'db>,
    combinator: &Combinator<'db>,
    visible: &mut Vec<LocalBinding<'db>>,
    occurrences: &mut Vec<SymbolOccurrence<'db>>,
) {
    match combinator {
        Combinator::Reference { name, args, span } => {
            if args.is_empty()
                && let Some(binding) = resolve_local_binding(visible, *name)
            {
                push_occurrence(
                    occurrences,
                    binding.symbol,
                    *span,
                    SymbolOccurrenceKind::Reference,
                );
            } else if let Some(definition) = resolve_symbol_in_hir(hir, *name) {
                push_occurrence(
                    occurrences,
                    definition.symbol_id(),
                    *span,
                    SymbolOccurrenceKind::Reference,
                );
            }

            for arg in args {
                if let Some(binding) = resolve_local_binding(visible, arg.name) {
                    push_occurrence(
                        occurrences,
                        binding.symbol,
                        arg.span,
                        SymbolOccurrenceKind::Reference,
                    );
                }
            }
        }
        Combinator::ConstrainedReference {
            name,
            constraint,
            span,
            ..
        } => {
            let enum_ty = resolve_symbol_in_hir(hir, *name).and_then(|definition| {
                push_occurrence(
                    occurrences,
                    definition.symbol_id(),
                    *span,
                    SymbolOccurrenceKind::Reference,
                );
                matches!(definition.kind, DefinitionKind::Enum(_)).then_some(definition.name)
            });

            if let Some(enum_ty) = enum_ty {
                for variant in constraint {
                    if let Some(symbol) = resolve_enum_variant_symbol(hir, enum_ty, variant.name) {
                        push_occurrence(
                            occurrences,
                            symbol,
                            variant.span,
                            SymbolOccurrenceKind::Reference,
                        );
                    }
                }
            }
        }
        Combinator::Struct(structure) => {
            let mut local_visible = visible.clone();
            for field in &structure.fields {
                let symbol = SymbolId::Field {
                    owner,
                    name: field.name,
                    declaration: field.span,
                    is_dependent: field.is_dependent,
                };
                push_occurrence(
                    occurrences,
                    symbol,
                    field.span,
                    SymbolOccurrenceKind::Declaration,
                );
                collect_combinator_occurrences(
                    hir,
                    owner,
                    &field.ty,
                    &mut local_visible,
                    occurrences,
                );
                collect_const_value_occurrences(
                    hir,
                    field.const_value.as_ref(),
                    enum_type_for_combinator(hir, &field.ty),
                    occurrences,
                );
                local_visible.push(LocalBinding {
                    symbol,
                    enum_ty: enum_type_for_combinator(hir, &field.ty),
                });
            }
        }
        Combinator::Choice(choice) => {
            let discriminant_enum = choice.discriminant.as_ref().and_then(|discriminant| {
                let binding = resolve_local_binding(visible, discriminant.name)?;
                push_occurrence(
                    occurrences,
                    binding.symbol,
                    discriminant.span,
                    SymbolOccurrenceKind::Reference,
                );
                binding.enum_ty
            });

            for arm in &choice.arms {
                if let (ChoicePattern::Variant(variant), Some(enum_ty)) =
                    (&arm.pattern, discriminant_enum)
                    && let Some(symbol) = resolve_enum_variant_symbol(hir, enum_ty, variant.name)
                {
                    push_occurrence(
                        occurrences,
                        symbol,
                        variant.span,
                        SymbolOccurrenceKind::Reference,
                    );
                }
                collect_combinator_occurrences(hir, owner, &arm.body, visible, occurrences);
            }
        }
        Combinator::Array(array) => {
            collect_combinator_occurrences(hir, owner, &array.element, visible, occurrences);
            collect_length_occurrences(hir, &array.length, visible, occurrences);
        }
        Combinator::Vec(vector) => {
            collect_combinator_occurrences(hir, owner, &vector.element, visible, occurrences);
        }
        Combinator::Option(option) => {
            collect_combinator_occurrences(hir, owner, &option.element, visible, occurrences);
        }
        Combinator::Wrap(wrap) => {
            for arg in &wrap.args {
                match arg {
                    WrapArg::Combinator(combinator) => {
                        collect_combinator_occurrences(
                            hir,
                            owner,
                            combinator,
                            visible,
                            occurrences,
                        );
                    }
                    WrapArg::ConstEnum { ty, variant } => {
                        if let Some(definition) = resolve_symbol_in_hir(hir, ty.name) {
                            push_occurrence(
                                occurrences,
                                definition.symbol_id(),
                                ty.span,
                                SymbolOccurrenceKind::Reference,
                            );
                            if matches!(definition.kind, DefinitionKind::Enum(_))
                                && let Some(symbol) =
                                    resolve_enum_variant_symbol(hir, definition.name, variant.name)
                            {
                                push_occurrence(
                                    occurrences,
                                    symbol,
                                    variant.span,
                                    SymbolOccurrenceKind::Reference,
                                );
                            }
                        }
                    }
                    WrapArg::ConstInt { .. } | WrapArg::ConstBytes { .. } => {}
                }
            }
        }
        Combinator::Bind { inner, target, .. } => {
            collect_combinator_occurrences(hir, owner, inner, visible, occurrences);
            collect_combinator_occurrences(hir, owner, target, visible, occurrences);
        }
        Combinator::ConstrainedInt { .. }
        | Combinator::Int(_)
        | Combinator::Tail
        | Combinator::Error => {}
    }
}

fn collect_length_occurrences<'db>(
    hir: &FileHir<'db>,
    expr: &LengthExpr<'db>,
    visible: &[LocalBinding<'db>],
    occurrences: &mut Vec<SymbolOccurrence<'db>>,
) {
    for term in &expr.terms {
        for atom in &term.atoms {
            match atom {
                LengthAtom::Param(param) => {
                    if let Some(binding) = resolve_local_binding(visible, param.name) {
                        push_occurrence(
                            occurrences,
                            binding.symbol,
                            param.span,
                            SymbolOccurrenceKind::Reference,
                        );
                    }
                }
                LengthAtom::ProjectedParam { base, .. } => {
                    if let Some(binding) = resolve_local_binding(visible, base.name) {
                        push_occurrence(
                            occurrences,
                            binding.symbol,
                            base.span,
                            SymbolOccurrenceKind::Reference,
                        );
                    }
                }
                LengthAtom::SizeOf(SizeTarget::Named(name)) => {
                    if let Some(definition) = resolve_symbol_in_hir(hir, name.name) {
                        push_occurrence(
                            occurrences,
                            definition.symbol_id(),
                            name.span,
                            SymbolOccurrenceKind::Reference,
                        );
                    }
                }
                LengthAtom::Paren(inner) => {
                    collect_length_occurrences(hir, inner, visible, occurrences);
                }
                LengthAtom::Const(_) | LengthAtom::SizeOf(SizeTarget::Type(_)) => {}
            }
        }
    }
}

fn collect_const_value_occurrences<'db>(
    hir: &FileHir<'db>,
    value: Option<&ConstValue<'db>>,
    enum_ty: Option<Name<'db>>,
    occurrences: &mut Vec<SymbolOccurrence<'db>>,
) {
    if let (Some(ConstValue::String(variant)), Some(enum_ty)) = (value, enum_ty)
        && let Some(symbol) = resolve_enum_variant_symbol(hir, enum_ty, variant.name)
    {
        push_occurrence(
            occurrences,
            symbol,
            variant.span,
            SymbolOccurrenceKind::Reference,
        );
    }
}

fn resolve_local_binding<'db>(
    visible: &[LocalBinding<'db>],
    name: Name<'db>,
) -> Option<LocalBinding<'db>> {
    visible
        .iter()
        .rev()
        .find(|binding| binding.symbol.name() == name)
        .copied()
}

fn resolve_enum_variant_symbol<'db>(
    hir: &FileHir<'db>,
    enum_name: Name<'db>,
    variant_name: Name<'db>,
) -> Option<SymbolId<'db>> {
    let definition = resolve_symbol_in_hir(hir, enum_name)?;
    let DefinitionKind::Enum(enum_def) = definition.kind else {
        return None;
    };
    enum_def
        .variants
        .iter()
        .find(|variant| variant.name == variant_name)
        .map(|variant| SymbolId::EnumVariant {
            owner: enum_name,
            name: variant.name,
            declaration: variant.span,
        })
}

fn enum_type_for_combinator<'db>(
    hir: &FileHir<'db>,
    combinator: &Combinator<'db>,
) -> Option<Name<'db>> {
    let name = match combinator {
        Combinator::Reference { name, .. } | Combinator::ConstrainedReference { name, .. } => *name,
        _ => return None,
    };

    resolve_symbol_in_hir(hir, name)
        .filter(|definition| matches!(definition.kind, DefinitionKind::Enum(_)))
        .map(|definition| definition.name)
}

fn push_occurrence<'db>(
    occurrences: &mut Vec<SymbolOccurrence<'db>>,
    symbol: SymbolId<'db>,
    span: Span,
    kind: SymbolOccurrenceKind,
) {
    occurrences.push(SymbolOccurrence { symbol, span, kind });
}

#[cfg(test)]
mod tests {
    use crate::Database;

    use super::*;

    fn setup(source: &str) -> (Database, SourceFile) {
        let db = Database::new();
        let file = SourceFile::new(&db, "test.vest".to_string(), 1, source.to_string());
        (db, file)
    }

    fn render_occurrences(occurrences: &[SymbolOccurrence<'_>]) -> String {
        occurrences
            .iter()
            .map(|occurrence| {
                let kind = match occurrence.kind {
                    SymbolOccurrenceKind::Declaration => "declaration",
                    SymbolOccurrenceKind::Reference => "reference",
                };
                format!(
                    "{kind}@{}..{}",
                    occurrence.span.start_byte, occurrence.span.end_byte
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn resolve_existing_symbol() {
        let (db, file) = setup("packet = { field: u8, }\nother = u16\n");
        let name = Name::new(&db, "packet");
        let def = resolve_symbol(&db, file, name);
        assert!(def.is_some());
        assert_eq!(def.unwrap().name.as_str(&db), "packet");
    }

    #[test]
    fn resolve_missing_symbol_returns_none() {
        let (db, file) = setup("packet = { field: u8, }\n");
        let name = Name::new(&db, "missing");
        let def = resolve_symbol(&db, file, name);
        assert!(def.is_none());
    }

    #[test]
    fn resolve_local_param() {
        let (db, file) = setup("msg(@len: u16) = [u8; @len]\n");
        let name = Name::new(&db, "msg");
        let def = resolve_symbol(&db, file, name).unwrap();

        let param_name = Name::new(&db, "len");
        let span = resolve_local_symbol(&db, &def, param_name);
        assert!(span.is_some());
    }

    #[test]
    fn resolve_local_field() {
        let (db, file) = setup("packet = { @len: u16, data: [u8; @len], }\n");
        let name = Name::new(&db, "packet");
        let def = resolve_symbol(&db, file, name).unwrap();

        let field_name = Name::new(&db, "len");
        let span = resolve_local_symbol(&db, &def, field_name);
        assert!(span.is_some());
    }

    #[test]
    fn resolve_local_enum_variant() {
        let (db, file) = setup("my_enum = enum { A = 0, B = 1, }\n");
        let name = Name::new(&db, "my_enum");
        let def = resolve_symbol(&db, file, name).unwrap();

        let variant_name = Name::new(&db, "A");
        let span = resolve_local_symbol(&db, &def, variant_name);
        assert!(span.is_some());
    }

    #[test]
    fn declaration_at_offset_prefers_top_level_name_span() {
        let (db, file) = setup("packet = { packet: u8, }\n");
        let name = Name::new(&db, "packet");
        let def = resolve_symbol(&db, file, name).unwrap();

        let span = declaration_at_offset(&def, 1).unwrap();
        assert_eq!(span, Span::new(0, 6));
    }

    #[test]
    fn declaration_at_offset_prefers_field_name_over_shadowed_param() {
        let (db, file) = setup("msg(@len: u16) = { @len: u8, data: [u8; @len], }\n");
        let name = Name::new(&db, "msg");
        let def = resolve_symbol(&db, file, name).unwrap();

        let span = declaration_at_offset(&def, 20).unwrap();
        assert_eq!(span, Span::new(19, 23));
    }

    #[test]
    fn file_definitions_returns_all() {
        let (db, file) = setup("a = u8\nb = u16\nc = u32\n");
        let defs = file_definitions(&db, file);
        assert_eq!(defs.len(), 3);
    }

    #[test]
    fn definition_at_offset_finds_correct_def() {
        let (db, file) = setup("first = u8\nsecond = u16\n");
        // "second" starts at offset 11
        let def = definition_at_offset(&db, file, 12);
        assert!(def.is_some());
        assert_eq!(def.unwrap().name.as_str(&db), "second");
    }

    #[test]
    fn symbol_at_offset_and_references_for_top_level_symbol() {
        let (db, file) = setup("other = u8\npacket = { field: other, next: other, }\n");
        let hir = lower_to_hir(&db, file);

        let symbol = symbol_at_offset_in_hir(&hir, 30).unwrap();
        let occurrences = references_for_symbol_in_hir(&hir, symbol, true);

        assert_eq!(
            render_occurrences(&occurrences),
            "declaration@0..5\nreference@29..34\nreference@42..47"
        );
    }

    #[test]
    fn symbol_at_offset_resolves_shadowed_field_references() {
        let (db, file) =
            setup("msg(@len: u16) = { @len: u8, data: [u8; @len], rest: [u8; @len], }\n");
        let hir = lower_to_hir(&db, file);

        let symbol = symbol_at_offset_in_hir(&hir, 41).unwrap();
        let occurrences = references_for_symbol_in_hir(&hir, symbol, true);

        assert!(
            symbol
                == SymbolId::Field {
                    owner: Name::new(&db, "msg"),
                    name: Name::new(&db, "len"),
                    declaration: Span::new(19, 23),
                    is_dependent: true,
                }
        );
        assert_eq!(
            render_occurrences(&occurrences),
            "declaration@19..23\nreference@40..44\nreference@58..62"
        );
    }

    #[test]
    fn symbol_references_for_dotted_dependent_id_use_base_binding_span() {
        let (db, file) = setup("msg = { @len: u16, data: [u8; @len.value], }\n");
        let hir = lower_to_hir(&db, file);

        let symbol = symbol_at_offset_in_hir(&hir, 31).unwrap();
        let occurrences = references_for_symbol_in_hir(&hir, symbol, true);

        assert_eq!(
            render_occurrences(&occurrences),
            "declaration@8..12\nreference@30..34"
        );
        assert!(symbol_at_offset_in_hir(&hir, 35).is_none());
    }

    #[test]
    fn collect_references_finds_all_refs() {
        let (db, _) = setup("");
        let name1 = Name::new(&db, "other");
        let name2 = Name::new(&db, "param");

        let comb = Combinator::Reference {
            name: name1,
            args: vec![NameRef::new(name2, Span::new(10, 16))],
            span: Span::empty(),
        };

        let refs = collect_references(&comb);
        assert_eq!(refs.len(), 2);
    }
}
