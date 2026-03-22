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
        DefinitionKind::Macro(m) => {
            for param in &m.params {
                if param.name.as_str(db) == name_str {
                    return Some(param.span);
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
        DefinitionKind::Macro(m) => m
            .params
            .iter()
            .find(|param| param.span.contains(byte_offset))
            .map(|param| param.span),
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
            refs.extend(args.iter().copied());
        }
        Combinator::ConstrainedReference {
            name, constraint, ..
        } => {
            refs.push(*name);
            refs.extend(constraint.iter().copied());
        }
        Combinator::Struct(s) => {
            for field in &s.fields {
                collect_refs_recursive(&field.ty, refs);
            }
        }
        Combinator::Choice(c) => {
            if let Some(d) = c.discriminant {
                refs.push(d);
            }
            for arm in &c.arms {
                if let ChoicePattern::Variant(v) = &arm.pattern {
                    refs.push(*v);
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
                        refs.push(*ty);
                        refs.push(*variant);
                    }
                    _ => {}
                }
            }
        }
        Combinator::Bind { inner, target } => {
            collect_refs_recursive(inner, refs);
            collect_refs_recursive(target, refs);
        }
        Combinator::MacroInvocation { name, args, .. } => {
            refs.push(*name);
            for arg in args {
                collect_refs_recursive(arg, refs);
            }
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
                LengthAtom::Param(p) => refs.push(*p),
                LengthAtom::SizeOf(SizeTarget::Named(n)) => refs.push(*n),
                LengthAtom::Paren(e) => collect_length_refs(e, refs),
                _ => {}
            }
        }
    }
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
    fn resolve_local_macro_param_returns_param_span() {
        let (db, file) = setup("macro copy!(x) = x\n");
        let name = Name::new(&db, "copy");
        let def = resolve_symbol(&db, file, name).unwrap();

        let param_name = Name::new(&db, "x");
        let span = resolve_local_symbol(&db, &def, param_name).unwrap();
        assert_eq!(span, Span::new(12, 13));
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
    fn collect_references_finds_all_refs() {
        let (db, _) = setup("");
        let name1 = Name::new(&db, "other");
        let name2 = Name::new(&db, "param");

        let comb = Combinator::Reference {
            name: name1,
            args: vec![name2],
            span: Span::empty(),
        };

        let refs = collect_references(&comb);
        assert_eq!(refs.len(), 2);
    }
}
