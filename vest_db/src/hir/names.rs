/// Returns the binding name referenced by a dependent identifier like `@len`
/// or `@header.len`.
pub fn dependent_binding_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let binding = text.strip_prefix('@').unwrap_or(text);
    let binding = binding.split('.').next().unwrap_or(binding);
    (!binding.is_empty()).then_some(binding)
}

/// Normalizes a syntax node's text into the symbol name
pub fn reference_name_text<'a>(kind: &str, text: &'a str) -> Option<&'a str> {
    match kind {
        "depend_id" => dependent_binding_name(text),
        "variant_id" if text == "_" => None,
        "variant_id" | "var_id" | "identifier" => (!text.is_empty()).then_some(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependent_binding_name_uses_base_binding() {
        assert_eq!(dependent_binding_name("@header.len"), Some("header"));
        assert_eq!(dependent_binding_name("@len"), Some("len"));
    }

    #[test]
    fn reference_name_text_rejects_wildcard_variants() {
        assert_eq!(reference_name_text("variant_id", "_"), None);
        assert_eq!(reference_name_text("identifier", "packet"), Some("packet"));
    }
}
