; tree-sitter-tel language injections.
;
; Mark source-atom and literal-atom payloads so downstream tools may
; inject embedded-language highlighting. The injection language is a
; placeholder; richer queries can dispatch on the enclosing compound's
; keyword to choose a real grammar (json, shell, markdown, etc.).

((source_atom) @injection.content
 (#set! injection.language "tel.embedded"))

((literal_atom) @injection.content
 (#set! injection.language "tel.embedded"))
