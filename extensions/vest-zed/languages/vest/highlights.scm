; Keywords
(constant) @keyword

(vec) @keyword

(tail) @keyword

"enum" @keyword

"choose" @keyword

"wrap" @keyword

"Option" @keyword

; Modifiers
(modifier) @keyword.modifier

; Builtin numeric combinators
(int_combinator) @type.builtin

(btc_varint) @type.builtin

(uleb128) @type.builtin

; Definitions and calls
(combinator_defn
  name: (var_id
    (identifier) @function.definition))

(combinator_invocation
  (var_id
    (identifier) @function.call))

; Struct fields and const definitions
(field
  (var_id
    (identifier) @property))

(const_combinator_defn
  (const_id
    (identifier) @constant))

; Dependent variables
(depend_id) @variable.parameter

; Enum variants
(variant_id
  (identifier) @type.enum.variant)

(variant_id
  "_" @type.enum.variant)

(enum_field
  (variant_id
    (identifier) @type.enum.variant))

; Literals
(decimal) @number

(hex) @number

(typed_const_int) @number

(ascii) @string.special

(string) @string

; Operators
"=" @operator

">>=" @operator

"|" @operator

"=>" @operator

".." @operator

"!" @operator

(add_op) @operator

(sub_op) @operator

(mul_op) @operator

(div_op) @operator

(non_exhaustive_marker) @operator

; Punctuation
[
  "("
  ")"
  "{"
  "}"
  "["
  "]"
  "<"
  ">"
] @punctuation.bracket

[
  ","
  ":"
  ";"
] @punctuation.delimiter

; Comments
(comment) @comment

; Endianness directives
(endianess_defn) @keyword.directive
