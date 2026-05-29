/**
 * tree-sitter-tel
 *
 * Grammar for TEL (Typed Element Language). Recognises the presentation
 * model — pragma, blocks, compounds, atoms, comments, remarks,
 * tabulations, source atoms, literal atoms. Most lexical work happens in
 * src/scanner.c because TEL is indentation- and sigil-sensitive.
 *
 * Spec: ../../spec/tel.md
 */

module.exports = grammar({
  name: 'tel',

  externals: $ => [
    $._shebang,
    $._pragma_keyword,
    $._pragma_atom,
    $._keyword,
    $._inline_atom_text,
    $._remark_text,
    $._comment_line,
    $._tabulation_line,
    $._tab_row,
    $._source_atom_text,
    $._literal_atom_open,
    $._literal_atom_body,
    $._newline,
    $._blank,
    $._indent,
    $._dedent,
    $._soft_gap_token,
    $._hard_gap_token,
    $._error_sentinel,
  ],

  extras: _ => [],

  conflicts: _ => [],

  rules: {
    document: $ => seq(
      optional($.shebang),
      optional($.pragma),
      repeat($._line),
    ),

    shebang: $ => seq($._shebang, $._newline),

    pragma: $ => seq(
      $._pragma_keyword,
      field('version', $._pragma_atom),
      repeat(field('atom', $._pragma_atom)),
      $._newline,
    ),

    _line: $ => choice(
      $.blank_line,
      $.comment,
      $.tabulation_block,
      $.compound,
    ),

    blank_line: $ => $._blank,

    comment: $ => seq($._comment_line, $._newline),

    tabulation_block: $ => seq(
      $.tabulation_line,
      repeat1($.tabulated_row),
    ),

    tabulation_line: $ => seq($._tabulation_line, $._newline),
    tabulated_row: $ => seq($._tab_row, $._newline),

    compound: $ => seq(
      field('keyword', $._keyword),
      repeat($._atom),
      optional(field('remark', $.remark)),
      $._newline,
      optional(choice($.source_atom, $.literal_atom)),
      optional($.children),
    ),

    // Each atom carries an explicit preceding-gap node — soft_atom for a
    // 1-space separator (§10.3 initial mode), hard_atom for a 2-or-more-space
    // separator (which also locks the rest of the line into hard-space mode,
    // making subsequent soft spaces part of atom content). The atom's
    // content is the byte range from the gap's end to the atom's end.
    _atom: $ => choice($.soft_atom, $.hard_atom),

    soft_atom: $ => seq($.soft_gap, $._inline_atom_text),
    hard_atom: $ => seq($.hard_gap, $._inline_atom_text),

    soft_gap: $ => $._soft_gap_token,
    hard_gap: $ => $._hard_gap_token,

    remark: $ => seq(
      choice($.soft_gap, $.hard_gap),
      $._remark_text,
    ),

    source_atom: $ => $._source_atom_text,

    literal_atom: $ => seq(
      field('delimiter', $._literal_atom_open),
      $._newline,
      field('body', $._literal_atom_body),
    ),

    children: $ => seq($._indent, repeat1($._line), $._dedent),
  },
});
