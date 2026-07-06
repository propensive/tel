; tree-sitter-tel highlights (Zed)

(pragma) @keyword.directive
(shebang) @comment.line

; The keyword that opens a compound.
(keyword) @keyword

; Inline atoms, coloured by their position within the compound so that successive atoms are visually
; distinct. A compound's first child is the `keyword` node, so each pattern anchors from the start
; with `. (keyword) .` and then names one more atom for each further position; the last-named atom is
; the one captured. Every atom is captured at exactly one position (no overlap, so highlight
; precedence is irrelevant); an atom beyond the sixth position falls back to the default text colour.
; (Intermediate positions are spelled out as `[(soft_atom) (hard_atom)]` rather than the `(_)`
; wildcard, because tree-sitter rejects deep anchored wildcard chains as "impossible patterns".)
;
; Note (TEL spacing): a single space separates atoms, but a 2-or-more-space gap starts an atom AND
; locks the rest of the line into hard-space mode, so subsequent single spaces are atom content. Thus
; `key  a b c` (double space) is one atom `a b c`, while `key a b c` (single spaces) is three atoms.
; Either way these patterns colour whichever atom *nodes* the parser produces, by position.
(compound . (keyword) . [(soft_atom) (hard_atom)] @function)
(compound . (keyword) . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] @string)
(compound . (keyword) . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] @number)
(compound . (keyword) . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] @type)
(compound . (keyword) . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] @constant)
(compound . (keyword) . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] . [(soft_atom) (hard_atom)] @property)

(remark) @comment.line
(comment) @comment
(tabulation_line) @attribute
(tabulated_row) @string
(source_atom) @string.special
(literal_atom) @string.special
