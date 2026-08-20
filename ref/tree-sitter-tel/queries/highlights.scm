; tree-sitter-tel highlights

; The pragma is a directive line, not code, so it takes `preproc` rather than a `keyword`
; sub-scope. Capture names resolve by stripping trailing components, so the previous
; `keyword.directive` fell back to `keyword` in themes that do not define it — rendering the
; pragma identically to every compound keyword.
(pragma) @preproc
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
