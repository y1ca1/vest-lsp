/// Returns the binding name referenced by a dependent identifier like `@len`
/// or `@header.len`.
pub fn dependent_binding_name(text: &str) -> Option<&str> {
    let text = text.trim();
    let binding = text.strip_prefix('@').unwrap_or(text);
    let binding = binding.split('.').next().unwrap_or(binding);
    (!binding.is_empty()).then_some(binding)
}

/// Returns whether a string is a valid Vest identifier.
pub fn is_valid_identifier_text(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) && !is_reserved_word(text)
}

fn is_reserved_word(text: &str) -> bool {
    matches!(
        text,
        "const"
            | "enum"
            | "choose"
            | "wrap"
            | "Option"
            | "Vec"
            | "Tail"
            | "btc_varint"
            | "uleb128"
            | "u8"
            | "u16"
            | "u24"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i24"
            | "i32"
            | "i64"
    )
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
    fn valid_identifier_text_rejects_reserved_words() {
        for reserved in [
            "enum",
            "choose",
            "wrap",
            "Option",
            "Vec",
            "Tail",
            "btc_varint",
            "uleb128",
            "u8",
            "i64",
        ] {
            assert!(
                !is_valid_identifier_text(reserved),
                "{reserved} should be reserved"
            );
        }
        assert!(is_valid_identifier_text("enum_tag"));
        assert!(is_valid_identifier_text("u8_value"));
    }

    #[test]
    fn reference_name_text_rejects_wildcard_variants() {
        assert_eq!(reference_name_text("variant_id", "_"), None);
        assert_eq!(reference_name_text("identifier", "packet"), Some("packet"));
    }
}
