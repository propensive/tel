; tree-sitter-tel highlights

(pragma) @keyword.directive
(shebang) @comment.line

; The keyword that opens a compound is now a named node.
(keyword) @keyword

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
