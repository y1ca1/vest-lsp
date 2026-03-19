(struct_combinator) @local.scope
(choice_combinator) @local.scope
(combinator_defn) @local.scope
(macro_defn) @local.scope

(field (var_id (identifier) @local.definition))
(field (depend_id) @local.definition)
(param_defn (depend_id) @local.definition)

(depend_id) @local.reference
(combinator_invocation (var_id (identifier) @local.reference))
