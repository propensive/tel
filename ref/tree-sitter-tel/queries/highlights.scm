; tree-sitter-tel highlights
;
; Notes: the keyword that opens a compound is the first byte range of the
; compound node and is recoverable from the source text, but it is not a
; named child node, so we cannot pattern-match on it here. Editors that
; want the keyword highlighted will need to use a post-tree pass (or rely
; on the broader @function highlight applied here to the whole compound's
; opening token).

(pragma) @keyword.directive
(shebang) @comment.line

(soft_atom) @string
(hard_atom) @string
(soft_gap) @punctuation.delimiter
(hard_gap) @punctuation.delimiter

(remark) @comment.line
(comment) @comment
(tabulation_line) @attribute
(tabulated_row) @string
(source_atom) @string.special
(literal_atom) @string.special
