/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

/**
 * Tree-sitter grammar for Vest DSL
 *
 * Vest is a parser and serializer generator for binary data formats.
 * This grammar is based on the pest grammar in vest.pest.
 */
module.exports = grammar({
  name: "vest",

  extras: ($) => [/\s/, $.comment],

  word: ($) => $.identifier,

  conflicts: ($) => [
    // constraint_int_combinator vs combinator_invocation: both start with identifier-like things
    [$.constraint_int_combinator, $.combinator_invocation],
    // combinator_inner rules that may conflict
    [$.struct_combinator, $.combinator_inner],
    // wrap_arg: const_bytes_or_array and array_combinator both start with [type; N]
    [$.const_bytes_or_array, $.array_combinator],
    [$.wrap_arg],
  ],

  rules: {
    source_file: ($) => repeat($.definition),

    definition: ($) =>
      choice(
        $.combinator_defn,
        $.const_combinator_defn,
        $.endianess_defn,
        $.macro_defn
      ),

    // combinator_defn = { var_id ~ param_defn_list? ~ "=" ~ combinator }
    combinator_defn: ($) =>
      seq(
        optional($.modifier),
        field("name", $.var_id),
        optional($.param_defn_list),
        "=",
        field("body", $.combinator)
      ),

    modifier: ($) => choice("secret", "public"),

    param_defn_list: ($) =>
      seq("(", $.param_defn, repeat(seq(",", $.param_defn)), ")"),

    param_defn: ($) => seq($.depend_id, ":", $.combinator_inner),

    // const_combinator_defn = { constant ~ const_id ~ ":" ~ const_combinator }
    const_combinator_defn: ($) =>
      seq($.constant, $.const_id, ":", $.const_combinator),

    // endianess_defn = { "!LITTLE_ENDIAN" | "!BIG_ENDIAN" }
    endianess_defn: ($) => choice("!LITTLE_ENDIAN", "!BIG_ENDIAN"),

    // macro_defn = { "macro" ~ var_id ~ "!" ~ "(" ~ macro_param_list? ~ ")" ~ "=" ~ combinator }
    macro_defn: ($) =>
      seq(
        "macro",
        field("name", $.var_id),
        "!",
        "(",
        optional($.macro_param_list),
        ")",
        "=",
        field("body", $.combinator)
      ),

    macro_param_list: ($) => seq($.var_id, repeat(seq(",", $.var_id))),

    // combinator = { ("(" ~ combinator_inner ~ ")" | combinator_inner) ~ (">>=" ~ combinator)? }
    combinator: ($) =>
      prec.right(
        seq(
          choice(seq("(", $.combinator_inner, ")"), $.combinator_inner),
          optional(seq(">>=", $.combinator))
        )
      ),

    combinator_inner: ($) =>
      choice(
        $.constraint_int_combinator,
        $.constraint_enum_combinator,
        $.macro_invocation,
        $.struct_combinator,
        $.wrap_combinator,
        $.enum_combinator,
        $.choice_combinator,
        $.vec_combinator,
        $.tail_combinator,
        $.array_combinator,
        $.option_combinator,
        $.combinator_invocation
      ),

    // Integer combinator with optional constraints
    constraint_int_combinator: ($) =>
      prec(
        2,
        seq($.int_combinator, optional(seq("|", $.int_constraint)))
      ),

    int_combinator: ($) =>
      choice(
        token(prec(3, seq(choice("u", "i"), choice("8", "16", "24", "32", "64")))),
        $.btc_varint,
        $.uleb128
      ),

    int_width: ($) => choice("8", "16", "24", "32", "64"),

    int_constraint: ($) =>
      choice(
        $.constraint_elem,
        $.constraint_elem_set,
        seq("!", $.int_constraint)
      ),

    constraint_elem_set: ($) =>
      seq(
        "{",
        $.constraint_elem,
        repeat(seq(",", $.constraint_elem)),
        "}"
      ),

    constraint_elem: ($) => choice($.const_int_range, $.const_int),

    const_int_range: ($) =>
      seq(optional($.const_int), "..", optional($.const_int)),

    const_int: ($) => choice($.hex, $.decimal, $.ascii),

    // Enum combinator with constraints
    constraint_enum_combinator: ($) =>
      prec(3, seq($.combinator_invocation, "|", $.enum_constraint)),

    enum_constraint: ($) =>
      choice(
        $.enum_constraint_elem,
        $.enum_constraint_set,
        seq("!", $.enum_constraint)
      ),

    enum_constraint_set: ($) =>
      seq(
        "{",
        $.enum_constraint_elem,
        repeat(seq(",", $.enum_constraint_elem)),
        "}"
      ),

    enum_constraint_elem: ($) => $.variant_id,

    // Struct combinator
    struct_combinator: ($) =>
      seq("{", repeat(seq($.field, ",")), "}"),

    field: ($) =>
      choice(
        seq($.constant, $.var_id, ":", $.const_combinator),
        seq($.depend_id, ":", $.combinator),
        seq($.var_id, ":", $.combinator)
      ),

    // Wrap combinator
    // wrap(prior_consts, main_combinator, post_consts)
    wrap_combinator: ($) =>
      seq(
        "wrap",
        "(",
        $.wrap_args,
        ")"
      ),

    // Arguments inside wrap: comma-separated list of combinators and inline_const_combinators
    // Use prec.dynamic to handle the ambiguity between [type; N] (array) and [type; N] = values (const_bytes)
    wrap_args: ($) =>
      prec.left(seq(
        $.wrap_arg,
        repeat(seq(",", $.wrap_arg))
      )),

    // Wrap arguments can be:
    // - const_int_combinator: u8 = 1
    // - const_enum_combinator: MyEnum = Variant
    // - const_bytes_or_array: [u8; 3] or [u8; 3] = [1,2,3]
    // - combinator: anything else (but not starting with [)
    wrap_arg: ($) =>
      choice(
        $.const_bytes_or_array,  // Unified rule for [type; N] patterns
        $.const_int_combinator,
        $.const_enum_combinator,
        $.combinator
      ),

    // This rule handles both array_combinator and const_bytes_combinator patterns
    // [type; N] optionally followed by = [values]
    // If "= [values]" is present, it's a const_bytes, otherwise it's an array
    const_bytes_or_array: ($) =>
      prec(5, seq(  // Higher precedence to win over combinator -> array_combinator
        "[",
        $.combinator,  // Use combinator to match type (could be int_combinator or user-defined)
        ";",
        $.length_expr,
        "]",
        optional(seq("=", $.const_array))
      )),

    inline_const_combinator: ($) =>
      choice(
        prec(3, $.const_bytes_combinator),  // Highest precedence - has "=" at the end
        prec(2, $.const_int_combinator),    // Higher precedence for int combinators
        $.const_enum_combinator
      ),

    // Enum definition combinator
    enum_combinator: ($) =>
      seq(
        "enum",
        "{",
        choice($.non_exhaustive_enum, $.exhaustive_enum),
        "}"
      ),

    exhaustive_enum: ($) => repeat1($.enum_field),

    non_exhaustive_enum: ($) =>
      seq(repeat1($.enum_field), $.non_exhaustive_marker),

    enum_field: ($) => seq($.variant_id, "=", $.typed_const_int, ","),

    non_exhaustive_marker: ($) => "...",

    // Choice combinator
    choice_combinator: ($) =>
      seq(
        "choose",
        optional(seq("(", $.depend_id, ")")),
        "{",
        repeat1($.choice_arm),
        "}"
      ),

    choice_arm: ($) =>
      seq(
        choice($.variant_id, $.constraint_elem, $.const_array),
        optional("=>"),
        $.combinator,
        ","
      ),

    // Vec combinator
    vec_combinator: ($) => seq($.vec, "<", $.combinator, ">"),

    // Array combinator
    array_combinator: ($) =>
      seq("[", $.combinator, ";", $.length_expr, "]"),

    // Length expressions
    length_expr: ($) =>
      prec.left(
        1,
        seq($.length_term, repeat(seq(choice($.add_op, $.sub_op), $.length_term)))
      ),

    length_term: ($) =>
      prec.left(
        2,
        seq($.length_atom, repeat(seq(choice($.mul_op, $.div_op), $.length_atom)))
      ),

    length_atom: ($) =>
      choice(
        $.size_expr,
        $.const_int,
        $.depend_id,
        seq("(", $.length_expr, ")")
      ),

    size_expr: ($) => seq("|", $.size_target, "|"),

    size_target: ($) =>
      choice(
        $.identifier,
        seq(choice($.unsigned, $.signed), $.int_width),
        $.btc_varint,
        $.uleb128
      ),

    add_op: ($) => "+",
    sub_op: ($) => "-",
    mul_op: ($) => "*",
    div_op: ($) => "/",

    // Option combinator
    option_combinator: ($) => seq("Option", "<", $.combinator, ">"),

    // Tail combinator
    tail_combinator: ($) => $.tail,

    // Combinator invocation
    combinator_invocation: ($) =>
      prec(1, seq($.var_id, optional($.param_list))),

    param_list: ($) => seq("(", $.param, repeat(seq(",", $.param)), ")"),

    param: ($) => $.depend_id,

    // Macro invocation
    macro_invocation: ($) =>
      seq($.var_id, "!", "(", $.macro_arg_list, ")"),

    macro_arg_list: ($) =>
      seq($.combinator_inner, repeat(seq(",", $.combinator_inner))),

    // Const combinators
    const_combinator: ($) => choice($.inline_const_combinator, $.const_id),

    // TODO: const_bytes_combinator inside wrap() may not parse correctly due to
    // lookahead limitations. The pattern "[type; N] = [values]" conflicts with
    // "[type; N]" (array_combinator) until the "=" is seen.
    const_bytes_combinator: ($) =>
      seq("[", $.int_combinator, ";", $.const_int, "]", "=", $.const_array),

    const_array: ($) => choice($.const_char_array, $.const_int_array),

    const_char_array: ($) => $.string,

    const_int_array: ($) =>
      choice($.int_array_expr, $.repeat_int_array_expr),

    int_array_expr: ($) =>
      seq("[", $.const_int, repeat(seq(",", $.const_int)), "]"),

    repeat_int_array_expr: ($) =>
      seq("[", $.const_int, ";", $.const_int, "]"),

    const_int_combinator: ($) => seq($.int_combinator, "=", $.const_int),

    const_enum_combinator: ($) =>
      seq($.combinator_invocation, "=", $.variant_id),

    // Typed const int
    typed_const_int: ($) => seq($.const_int, optional($.int_type_suffix)),

    int_type_suffix: ($) =>
      token(prec(2, seq(choice("u", "i"), choice("8", "16", "24", "32", "64")))),

    // Literals
    decimal: ($) => /[0-9]+/,

    hex: ($) => /0x[0-9a-fA-F]+/,

    ascii: ($) =>
      choice(
        seq("'", /\\x[0-9a-fA-F]{2}/, "'"),
        seq("'", /[^\u0000-\u001F\\']/, "'")
      ),

    string: ($) => /"[^"]*"/,

    // Identifiers
    identifier: ($) => /[a-zA-Z_][a-zA-Z0-9_]*/,

    var_id: ($) => $.identifier,

    const_id: ($) => $.identifier,

    variant_id: ($) => choice("_", $.identifier),

    depend_id: ($) => /@[a-zA-Z_][a-zA-Z0-9_]*(\.[a-zA-Z_][a-zA-Z0-9_]*)*/,

    // Keywords
    constant: ($) => "const",
    signed: ($) => "i",
    unsigned: ($) => "u",
    btc_varint: ($) => "btc_varint",
    uleb128: ($) => "uleb128",
    vec: ($) => "Vec",
    tail: ($) => "Tail",

    // Comment
    comment: ($) => token(seq("//", /.*/)),
  },
});
