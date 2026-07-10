// tree-sitter-tel external scanner.
//
// Implements the indent- and sigil-sensitive lexical rules of TEL that
// cannot be expressed in plain LR rules: line-level structure, dynamic
// sigil discovery via the pragma, indent / dedent tracking, the
// phrase-separation rule (soft- vs. hard-space mode), source-atom
// payloads (greedy +2 indent capture), literal-atom delimiter scanning
// against the raw byte stream, and tabulation-line / tab-row detection.
//
// Spec: ../../../spec/tel.md
//
// State is serialised at every parser checkpoint via memcpy, so the
// struct stays a plain old data type.

#include "tree_sitter/parser.h"
#include <stdint.h>
#include <string.h>
#include <wctype.h>

enum TokenType {
  TOK_SHEBANG,
  TOK_PRAGMA_KEYWORD,
  TOK_PRAGMA_ATOM,
  TOK_KEYWORD,
  TOK_INLINE_ATOM_TEXT,
  TOK_REMARK_TEXT,
  TOK_COMMENT_LINE,
  TOK_TABULATION_LINE,
  TOK_TAB_ROW,
  TOK_SOURCE_ATOM_TEXT,
  TOK_LITERAL_ATOM_OPEN,
  TOK_LITERAL_ATOM_BODY,
  TOK_NEWLINE,
  TOK_BLANK,
  TOK_INDENT,
  TOK_DEDENT,
  TOK_SOFT_GAP_TOKEN,
  TOK_HARD_GAP_TOKEN,
  TOK_ERROR_SENTINEL,
};

#define MAX_INDENT_DEPTH 64
#define MAX_DELIM_LEN     64

typedef struct {
  uint8_t  sigil;              // 0 = unset → effective '#'
  uint8_t  margin;             // leading spaces on first non-blank line
  uint8_t  saw_first_nonblank; // pragma window has closed
  uint8_t  at_line_start;      // next call expects start-of-line processing
  uint8_t  hard_space_mode;    // reset on every _newline
  uint8_t  emitted_keyword;    // reset on every _newline
  uint8_t  after_compound_eol; // last _newline closed a compound line
  uint8_t  this_line_is_compound; // set when _keyword emitted on this line
  uint8_t  in_tabulation;      // we are inside a tabulated block (tabulation_line already emitted)
  uint8_t  tab_indent;         // indent of the active tabulation
  uint8_t  pending_dedents;    // remaining _dedent emissions
  uint8_t  expect_literal_body;
  uint8_t  literal_delim_len;
  uint8_t  literal_delim[MAX_DELIM_LEN];
  uint16_t literal_indent_spaces; // leading spaces of the opening delimiter line
  int8_t   current_indent;     // current open-block indent (units), starts at 0
  uint8_t  indent_depth;
  uint8_t  indent_stack[MAX_INDENT_DEPTH];
  uint8_t  pragma_phrase_idx;  // 0 = expect "tel", 1 = version, 2 = schema, 3 = sigil
} Scanner;

static inline void reset_line_flags(Scanner *s) {
  s->hard_space_mode = 0;
  s->emitted_keyword = 0;
  s->this_line_is_compound = 0;
}

static inline uint8_t effective_sigil(const Scanner *s) {
  return s->sigil ? s->sigil : (uint8_t)'#';
}

static inline bool is_valid_sigil_char(uint8_t c) {
  // ASCII symbolic char: not letter, digit, space, control, paren-like.
  if (c < 0x21 || c > 0x7E) return false;
  if ((c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')) return false;
  switch (c) {
    case '(': case ')': case '[': case ']':
    case '<': case '>': case '{': case '}':
      return false;
    default:
      return true;
  }
}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }
static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

void *tree_sitter_tel_external_scanner_create(void) {
  Scanner *s = (Scanner *)calloc(1, sizeof(Scanner));
  s->at_line_start = 1;
  s->current_indent = 0;
  s->indent_depth = 0;
  return s;
}

void tree_sitter_tel_external_scanner_destroy(void *payload) { free(payload); }

unsigned tree_sitter_tel_external_scanner_serialize(void *payload, char *buffer) {
  memcpy(buffer, payload, sizeof(Scanner));
  return sizeof(Scanner);
}

void tree_sitter_tel_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  Scanner *s = (Scanner *)payload;
  if (length == sizeof(Scanner)) {
    memcpy(s, buffer, sizeof(Scanner));
  } else {
    memset(s, 0, sizeof(Scanner));
    s->at_line_start = 1;
  }
}

// ---------------------------------------------------------------------------
// Pragma keyword peeking: "tel" at start of line followed by space or EOL.
// Called only when scanner is positioned at first non-space of first
// non-blank content line. We have already consumed leading spaces.
// ---------------------------------------------------------------------------
static bool peek_pragma_keyword(TSLexer *lexer) {
  if (lexer->lookahead != 't') return false;
  advance(lexer);
  if (lexer->lookahead != 'e') return false;
  advance(lexer);
  if (lexer->lookahead != 'l') return false;
  advance(lexer);
  // Now look at next char: must be space or LF (or EOF), making "tel" a complete keyword.
  int32_t c = lexer->lookahead;
  return (c == ' ' || c == '\n' || c == 0);
}

// ---------------------------------------------------------------------------
// Source atom payload scan.
// Called at start of a line that's exactly compound+2 indent (relative to
// the indent of the line that introduced the compound). Consumes lines
// greedily until a non-blank line at indent < source indent or EOF.
// Emits the whole captured payload as a single _source_atom_text token.
// We DO NOT advance past the terminator line — we leave the cursor at
// its start, so normal line-start processing reopens it.
// ---------------------------------------------------------------------------
static bool scan_source_atom(Scanner *s, TSLexer *lexer, uint32_t leading_spaces) {
  // Entry: cursor is at the first non-space character of the source-atom's
  // opening line (leading spaces already consumed by the at_line_start path).
  //
  // We capture content until a non-blank line is encountered whose indent is
  // strictly less than `leading_spaces`, or until EOF. Blank lines are part
  // of the source atom per §14 but DO NOT terminate it on their own; we must
  // continue scanning past them to find the next non-blank line.
  for (;;) {
    // Consume current line content up to (but not past) the LF.
    while (lexer->lookahead != 0 && lexer->lookahead != '\n') advance(lexer);
    if (lexer->lookahead == 0) {
      lexer->mark_end(lexer);
      break;
    }
    advance(lexer);              // consume LF
    lexer->mark_end(lexer);

    // Skip any number of blank lines, then check the next non-blank line's
    // leading whitespace.
    for (;;) {
      uint32_t spaces = 0;
      while (lexer->lookahead == ' ' && spaces < leading_spaces) {
        advance(lexer);
        spaces++;
      }
      if (lexer->lookahead == 0) {
        // EOF after blank/short lines: source atom ends.
        lexer->mark_end(lexer);
        s->at_line_start = 1;
        lexer->result_symbol = TOK_SOURCE_ATOM_TEXT;
        return true;
      }
      if (lexer->lookahead == '\n') {
        // Blank (or space-only) line. Consume LF and check next.
        advance(lexer);
        lexer->mark_end(lexer);
        continue;
      }
      if (spaces < leading_spaces) {
        // Non-blank line at strictly lesser indent → source atom ends.
        lexer->result_symbol = TOK_SOURCE_ATOM_TEXT;
        s->at_line_start = 1;
        return true;
      }
      // Non-blank line at sufficient indent: stay inside the source atom.
      // Break to the outer loop, which consumes this line's remaining content.
      break;
    }
  }
  s->at_line_start = 1;
  lexer->result_symbol = TOK_SOURCE_ATOM_TEXT;
  return true;
}

// ---------------------------------------------------------------------------
// Literal atom OPENING: capture the delimiter (everything after the
// leading SPACE run on the opening line, up to but not including LF).
// ---------------------------------------------------------------------------
static bool scan_literal_open(Scanner *s, TSLexer *lexer, uint32_t leading_spaces) {
  s->literal_indent_spaces = (uint16_t)leading_spaces;
  s->literal_delim_len = 0;
  while (lexer->lookahead != 0 && lexer->lookahead != '\n') {
    if (s->literal_delim_len < MAX_DELIM_LEN) {
      s->literal_delim[s->literal_delim_len++] = (uint8_t)lexer->lookahead;
    }
    advance(lexer);
  }
  if (s->literal_delim_len == 0) {
    // Empty delimiter → not a literal atom (treated as blank per §15).
    return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = TOK_LITERAL_ATOM_OPEN;
  s->expect_literal_body = 1;
  return true;
}

// ---------------------------------------------------------------------------
// Literal atom BODY: scan raw bytes for `LF + closing delimiter line + LF`,
// where the closing delimiter line is byte-identical to the opening one:
// its leading spaces followed by the delimiter (§15). We include everything
// from the current position up to and including the closing LF in the token.
// ---------------------------------------------------------------------------
static bool scan_literal_body(Scanner *s, TSLexer *lexer) {
  // The cursor is positioned right after the LF that ended the opening
  // delimiter line. Scan bytes until we match the closing line or hit EOF.
  bool at_line_start = true;
  for (;;) {
    int32_t c = lexer->lookahead;
    if (c == 0) {
      // EOF before closing → error sentinel for the body; we still emit it
      // so the LR layer can wrap an ERROR around the compound.
      lexer->mark_end(lexer);
      lexer->result_symbol = TOK_LITERAL_ATOM_BODY;
      s->expect_literal_body = 0;
      // After a runaway literal, we treat the document as ended.
      s->at_line_start = 1;
      return true;
    }
    if (at_line_start) {
      // Try matching the closing delimiter line against this line: the
      // opening indentation, then the delimiter, then LF.
      uint32_t sp = 0;
      while (sp < s->literal_indent_spaces && lexer->lookahead == ' ') {
        advance(lexer);
        sp++;
      }
      if (sp == s->literal_indent_spaces) {
        uint32_t i = 0;
        while (i < s->literal_delim_len && lexer->lookahead == s->literal_delim[i]) {
          advance(lexer);
          i++;
        }
        if (i == s->literal_delim_len && lexer->lookahead == '\n') {
          // Closing match: consume the trailing LF and mark end.
          advance(lexer);
          lexer->mark_end(lexer);
          lexer->result_symbol = TOK_LITERAL_ATOM_BODY;
          s->expect_literal_body = 0;
          s->at_line_start = 1;
          return true;
        }
      }
      // Partial or non-match: the chars consumed so far are body content.
      at_line_start = false;
      continue;
    }
    advance(lexer);
    if (c == '\n') at_line_start = true;
  }
}

// ---------------------------------------------------------------------------
// Look-ahead within the current line to decide: tabulation-line or
// comment-line? A line whose first non-space char is the sigil is a
// tabulation line iff at least one further sigil occurs preceded by a
// hard-space run. We must check this without committing characters to a
// token — we use advance() to peek and rely on mark_end() to set the
// actual token end after we've decided.
//
// On entry: lexer is positioned at the leading sigil. We advance over
// the rest of the line, recording whether we see "hard-space + sigil".
// We then call mark_end at the LF and emit either tabulation_line or
// comment_line. Either way, the same line content is consumed.
// ---------------------------------------------------------------------------
static bool scan_sigil_keyword_line(Scanner *s, TSLexer *lexer, const bool *valid) {
  uint8_t sigil = effective_sigil(s);
  bool is_tabulation = false;
  uint32_t consec_spaces = 0;

  // First char is the sigil; consume it.
  advance(lexer);
  for (;;) {
    int32_t c = lexer->lookahead;
    if (c == 0 || c == '\n') break;
    if (c == ' ') {
      consec_spaces++;
      advance(lexer);
      continue;
    }
    if (c == sigil && consec_spaces >= 2) {
      is_tabulation = true;
    }
    consec_spaces = 0;
    advance(lexer);
  }
  lexer->mark_end(lexer);
  if (is_tabulation) {
    if (!valid[TOK_TABULATION_LINE]) return false;
    lexer->result_symbol = TOK_TABULATION_LINE;
    s->in_tabulation = 1;
    s->tab_indent = (uint8_t)s->current_indent;
  } else {
    if (!valid[TOK_COMMENT_LINE]) return false;
    lexer->result_symbol = TOK_COMMENT_LINE;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Emit a _keyword token: consume non-space chars until a space or LF.
// Spec §10.1: keyword may be any Unicode except SPACE and LF.
// ---------------------------------------------------------------------------
static bool scan_keyword(Scanner *s, TSLexer *lexer) {
  while (lexer->lookahead != 0 && lexer->lookahead != '\n' && lexer->lookahead != ' ') {
    advance(lexer);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = TOK_KEYWORD;
  s->emitted_keyword = 1;
  s->this_line_is_compound = 1;
  return true;
}

// ---------------------------------------------------------------------------
// Emit a _pragma_atom token: pragma uses simple soft-space separation
// (no hard-space mode). Consume non-space, non-LF chars.
// Capture the sigil byte if this is a single ASCII symbolic char.
// ---------------------------------------------------------------------------
static bool scan_pragma_atom(Scanner *s, TSLexer *lexer) {
  uint32_t len = 0;
  uint8_t first_byte = 0;
  while (lexer->lookahead != 0 && lexer->lookahead != '\n' && lexer->lookahead != ' ') {
    if (len == 0) first_byte = (uint8_t)lexer->lookahead;
    len++;
    advance(lexer);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = TOK_PRAGMA_ATOM;
  s->pragma_phrase_idx++;
  if (len == 1 && is_valid_sigil_char(first_byte)) {
    s->sigil = first_byte;
  }
  return true;
}

// ---------------------------------------------------------------------------
// Emit an _inline_atom token. Phrase-separation rule (§10.3): before the
// first hard-space run, a single space ends a phrase. After the first
// hard-space run, only hard-space runs end phrases — soft spaces become
// part of the phrase. Also detect a remark at a phrase boundary:
// sigil + soft space → switch to remark mode.
// ---------------------------------------------------------------------------
static bool scan_inline_atom_or_remark(Scanner *s, TSLexer *lexer, const bool *valid) {
  // The mid-line caller emits a soft_gap / hard_gap token on the call before
  // this one, so when we are entered the cursor sits at the first non-space
  // character of a new phrase.
  uint8_t sigil = effective_sigil(s);

  // Check for remark: sigil + soft space + non-space-non-LF content.
  if (lexer->lookahead == sigil) {
    advance(lexer);
    lexer->mark_end(lexer);  // tentative: atom is just the sigil
    if (lexer->lookahead == ' ') {
      advance(lexer);
      if (lexer->lookahead != ' ' && lexer->lookahead != '\n' && lexer->lookahead != 0) {
        // Remark. Consume the rest of the line.
        while (lexer->lookahead != 0 && lexer->lookahead != '\n') {
          advance(lexer);
        }
        lexer->mark_end(lexer);
        if (!valid[TOK_REMARK_TEXT]) return false;
        lexer->result_symbol = TOK_REMARK_TEXT;
        return true;
      }
      // sigil + 1 space + (hard space or EOL/EOF): NOT a remark.
      // The atom is just "<sigil>"; the trailing space starts the next gap.
      // mark_end was set after the sigil, so the token ends there; the next
      // scan resumes at the space, where the gap-emission path takes over.
      if (!valid[TOK_INLINE_ATOM_TEXT]) return false;
      lexer->result_symbol = TOK_INLINE_ATOM_TEXT;
      return true;
    }
    // sigil + (EOL or non-space): atom starts with sigil; continue scanning.
    // mark_end will be overwritten as we consume more content.
  }

  // Scan the rest of the phrase.
  if (s->hard_space_mode) {
    // In hard-space mode the phrase ends only at a hard-space run or EOL.
    // Soft spaces are content. We mark_end before each candidate space so
    // we can "rewind" to it if the space turns out to be the start of a
    // hard gap.
    for (;;) {
      int32_t c = lexer->lookahead;
      if (c == 0 || c == '\n') {
        lexer->mark_end(lexer);
        break;
      }
      if (c == ' ') {
        lexer->mark_end(lexer);   // tentatively end before this space
        advance(lexer);
        if (lexer->lookahead == ' ' || lexer->lookahead == '\n' || lexer->lookahead == 0) {
          // Hard gap or trailing/EOL — phrase ended at the marked position.
          break;
        }
        // Soft space (content). Continue; subsequent stops will re-mark.
        continue;
      }
      advance(lexer);
    }
  } else {
    // Initial mode: any space ends the phrase.
    while (lexer->lookahead != 0 && lexer->lookahead != '\n' && lexer->lookahead != ' ') {
      advance(lexer);
    }
    lexer->mark_end(lexer);
  }
  if (!valid[TOK_INLINE_ATOM_TEXT]) return false;
  lexer->result_symbol = TOK_INLINE_ATOM_TEXT;
  return true;
}

// ---------------------------------------------------------------------------
// Scan a tabulated row: consume the whole row content (similar to a
// compound line but treated as opaque to the LR layer). Sigil-and-space
// look-ahead for remarks would be nice but for v1 we take everything to
// end of line.
// ---------------------------------------------------------------------------
static bool scan_tab_row(Scanner *s, TSLexer *lexer) {
  while (lexer->lookahead != 0 && lexer->lookahead != '\n') {
    advance(lexer);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = TOK_TAB_ROW;
  return true;
}

// ---------------------------------------------------------------------------
// Main scan dispatch.
// ---------------------------------------------------------------------------
bool tree_sitter_tel_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
  Scanner *s = (Scanner *)payload;

  // 1. Emit pending dedents.
  if (s->pending_dedents > 0 && valid_symbols[TOK_DEDENT]) {
    s->pending_dedents--;
    if (s->indent_depth > 0) {
      s->current_indent = (int8_t)s->indent_stack[--s->indent_depth];
    } else {
      s->current_indent = 0;
    }
    lexer->result_symbol = TOK_DEDENT;
    return true;
  }

  // 2. Emit the literal atom body if we just emitted its opener.
  if (s->expect_literal_body && valid_symbols[TOK_LITERAL_ATOM_BODY]) {
    return scan_literal_body(s, lexer);
  }

  // 3. Start-of-line indent processing.
  if (s->at_line_start) {
    // Count leading spaces. Start from the current column to account for
    // bytes the source-atom / literal-atom captures may have already
    // consumed on this line.
    uint32_t spaces = lexer->get_column(lexer);
    while (lexer->lookahead == ' ') {
      skip(lexer);
      spaces++;
    }
    // EOF or blank line?
    if (lexer->lookahead == 0) {
      // Flush dedents back to depth 0, then nothing.
      if (s->indent_depth > 0 && valid_symbols[TOK_DEDENT]) {
        s->pending_dedents = s->indent_depth - 1;
        s->indent_depth--;
        s->current_indent = s->indent_depth > 0 ? (int8_t)s->indent_stack[s->indent_depth - 1] : 0;
        lexer->result_symbol = TOK_DEDENT;
        return true;
      }
      return false;
    }
    if (lexer->lookahead == '\n') {
      // Blank line.
      advance(lexer);
      lexer->mark_end(lexer);
      if (!valid_symbols[TOK_BLANK]) return false;
      lexer->result_symbol = TOK_BLANK;
      s->after_compound_eol = 0;
      s->in_tabulation = 0;  // blank line terminates tabulated block
      reset_line_flags(s);
      return true;
    }

    // Establish margin on the first non-blank line.
    int32_t indent;
    if (!s->saw_first_nonblank) {
      s->margin = (uint8_t)spaces;
      indent = 0;
    } else {
      if (spaces < s->margin) {
        // E106: less than margin. Treat as indent 0 for recovery.
        indent = 0;
      } else {
        uint32_t excess = spaces - s->margin;
        // Odd indentation → round down for recovery. (E107 is reported
        // semantically by downstream tooling; the tree-sitter layer just
        // produces a parseable tree.)
        indent = (int32_t)(excess / 2);
      }
    }

    // If we were in a tabulated block, leaving the tabulation's indent
    // terminates it.
    if (s->in_tabulation && indent != s->tab_indent) {
      s->in_tabulation = 0;
    }

    int32_t diff = indent - s->current_indent;

    // Source / literal atom triggers (only valid right after a compound's _newline).
    if (s->after_compound_eol && diff == 2 && valid_symbols[TOK_SOURCE_ATOM_TEXT]) {
      // Source atom: consume payload.
      s->at_line_start = 0;
      s->after_compound_eol = 0;
      reset_line_flags(s);
      return scan_source_atom(s, lexer, (uint32_t)s->margin + (uint32_t)indent * 2);
    }
    if (s->after_compound_eol && diff == 3 && valid_symbols[TOK_LITERAL_ATOM_OPEN]) {
      // Literal atom opening.
      s->at_line_start = 0;
      s->after_compound_eol = 0;
      reset_line_flags(s);
      return scan_literal_open(s, lexer, spaces);
    }

    if (diff == 1 && valid_symbols[TOK_INDENT]) {
      // Child block opens.
      if (s->indent_depth < MAX_INDENT_DEPTH) {
        s->indent_stack[s->indent_depth++] = (uint8_t)s->current_indent;
      }
      s->current_indent = (int8_t)indent;
      lexer->result_symbol = TOK_INDENT;
      s->after_compound_eol = 0;
      // at_line_start stays true; the next scan call emits content tokens.
      return true;
    }
    if (diff < 0 && valid_symbols[TOK_DEDENT]) {
      // Dedent.
      int needed = -diff;
      if (needed > s->indent_depth) needed = s->indent_depth;
      s->pending_dedents = (uint8_t)(needed - 1);
      if (s->indent_depth > 0) {
        s->current_indent = (int8_t)s->indent_stack[--s->indent_depth];
      } else {
        s->current_indent = 0;
      }
      lexer->result_symbol = TOK_DEDENT;
      s->after_compound_eol = 0;
      return true;
    }
    // diff == 0 (peer) — drop through to content tokenisation.
    s->at_line_start = 0;
    s->after_compound_eol = 0;
    reset_line_flags(s);
  }

  // 4. Mid-line dispatch.
  //
  // The cursor is positioned somewhere after the line's leading indent
  // (step 3 handled that). What we do next depends on whether we have
  // already emitted the line's first content token (`emitted_keyword`)
  // and whether we are mid-pragma (where gaps are insignificant).
  //
  // For mid-compound positions we want the gap between phrases to be a
  // visible part of the tree — so we `advance()` over spaces (which makes
  // them part of the next emitted token) and emit a dedicated
  // `_soft_gap_token` or `_hard_gap_token`. Before the first content of a
  // line, or anywhere inside the pragma, we `skip()` instead because the
  // gap there is structurally insignificant.
  bool in_pragma = s->pragma_phrase_idx > 0;
  bool emit_gap_here = s->emitted_keyword && !in_pragma;
  uint32_t gap = 0;
  while (lexer->lookahead == ' ') {
    if (emit_gap_here) {
      advance(lexer);
    } else {
      skip(lexer);
    }
    gap++;
  }

  // 4a. End of line / EOF take precedence over gap emission: a "trailing"
  //     gap with no content after it isn't a phrase separator. The
  //     advanced spaces get absorbed into the _newline token.
  if (lexer->lookahead == '\n') {
    advance(lexer);
    lexer->mark_end(lexer);
    if (!valid_symbols[TOK_NEWLINE]) return false;
    lexer->result_symbol = TOK_NEWLINE;
    s->at_line_start = 1;
    s->saw_first_nonblank = 1;
    s->after_compound_eol = s->this_line_is_compound ? 1 : 0;
    s->pragma_phrase_idx = 0;
    reset_line_flags(s);
    return true;
  }
  if (lexer->lookahead == 0) {
    if (s->emitted_keyword && valid_symbols[TOK_NEWLINE]) {
      lexer->mark_end(lexer);
      lexer->result_symbol = TOK_NEWLINE;
      s->at_line_start = 1;
      s->saw_first_nonblank = 1;
      s->after_compound_eol = s->this_line_is_compound ? 1 : 0;
      s->pragma_phrase_idx = 0;
      reset_line_flags(s);
      return true;
    }
    return false;
  }

  // 4b. Mid-compound gap emission: spaces preceded a non-space content
  //     character, so emit the gap as its own token.
  if (emit_gap_here && gap > 0) {
    lexer->mark_end(lexer);
    if (gap >= 2) {
      s->hard_space_mode = 1;
      if (!valid_symbols[TOK_HARD_GAP_TOKEN]) return false;
      lexer->result_symbol = TOK_HARD_GAP_TOKEN;
      return true;
    }
    // gap == 1
    if (s->hard_space_mode) {
      // In hard-space mode a single space at a phrase boundary is malformed
      // (a real hard gap would have ≥2 spaces). Emit as a hard_gap for
      // recovery so the LR layer can still attach the next atom.
      if (!valid_symbols[TOK_HARD_GAP_TOKEN]) return false;
      lexer->result_symbol = TOK_HARD_GAP_TOKEN;
      return true;
    }
    if (!valid_symbols[TOK_SOFT_GAP_TOKEN]) return false;
    lexer->result_symbol = TOK_SOFT_GAP_TOKEN;
    return true;
  }

  // First non-blank line of document: check for shebang and pragma.
  if (!s->saw_first_nonblank && !s->emitted_keyword) {
    // Shebang: literal '#' '!' at byte 0 — only if margin is 0. The
    // shebang check uses literal '#', NOT the dynamic sigil, per §7.
    if (lexer->lookahead == '#' && s->margin == 0) {
      // We must advance past '#' to peek the next char. If it isn't '!',
      // we still want to handle the line as comment / tabulation (sigil
      // is '#' until pragma overrides it). We've already committed to
      // consuming '#', so we scan the rest of the line ourselves rather
      // than re-entering scan_sigil_keyword_line (which expects the
      // cursor to be at the sigil).
      advance(lexer);
      if (lexer->lookahead == '!' && valid_symbols[TOK_SHEBANG]) {
        while (lexer->lookahead != 0 && lexer->lookahead != '\n') advance(lexer);
        lexer->mark_end(lexer);
        lexer->result_symbol = TOK_SHEBANG;
        return true;
      }
      // Not a shebang. The leading '#' is the sigil → this line is a
      // comment or tabulation. Walk the rest of the line and decide.
      bool is_tab = false;
      uint32_t consec = 0;
      while (lexer->lookahead != 0 && lexer->lookahead != '\n') {
        if (lexer->lookahead == ' ') {
          consec++;
        } else {
          if (lexer->lookahead == '#' && consec >= 2) is_tab = true;
          consec = 0;
        }
        advance(lexer);
      }
      lexer->mark_end(lexer);
      if (is_tab && valid_symbols[TOK_TABULATION_LINE]) {
        lexer->result_symbol = TOK_TABULATION_LINE;
        s->in_tabulation = 1;
        s->tab_indent = (uint8_t)s->current_indent;
      } else if (valid_symbols[TOK_COMMENT_LINE]) {
        lexer->result_symbol = TOK_COMMENT_LINE;
      } else {
        return false;
      }
      s->emitted_keyword = 1;
      s->this_line_is_compound = 0;
      return true;
    }
    // Pragma keyword: "tel" followed by space or LF.
    if (valid_symbols[TOK_PRAGMA_KEYWORD] && peek_pragma_keyword(lexer)) {
      lexer->mark_end(lexer);
      lexer->result_symbol = TOK_PRAGMA_KEYWORD;
      s->pragma_phrase_idx = 1;
      s->emitted_keyword = 1;
      s->this_line_is_compound = 0;
      return true;
    }
    // Otherwise fall through to ordinary tokenisation. We may have
    // partially advanced via peek_pragma_keyword; if so, those chars are
    // committed as part of a keyword.
    if (valid_symbols[TOK_KEYWORD]) {
      while (lexer->lookahead != 0 && lexer->lookahead != '\n' && lexer->lookahead != ' ') {
        advance(lexer);
      }
      lexer->mark_end(lexer);
      lexer->result_symbol = TOK_KEYWORD;
      s->emitted_keyword = 1;
      s->this_line_is_compound = 1;
      return true;
    }
  }

  // Pragma atom continuation (we've already emitted _pragma_keyword on this line).
  if (s->pragma_phrase_idx > 0 && s->pragma_phrase_idx <= 3 && valid_symbols[TOK_PRAGMA_ATOM]) {
    return scan_pragma_atom(s, lexer);
  }

  // In a tabulated block: emit a _tab_row.
  if (s->in_tabulation && !s->emitted_keyword && valid_symbols[TOK_TAB_ROW]) {
    s->emitted_keyword = 1;
    s->this_line_is_compound = 0;
    return scan_tab_row(s, lexer);
  }

  // First content token of an ordinary line: comment / tabulation / keyword.
  if (!s->emitted_keyword) {
    uint8_t sigil = effective_sigil(s);
    if ((uint8_t)lexer->lookahead == sigil) {
      // comment or tabulation
      s->emitted_keyword = 1;
      s->this_line_is_compound = 0;
      return scan_sigil_keyword_line(s, lexer, valid_symbols);
    }
    if (valid_symbols[TOK_KEYWORD]) {
      return scan_keyword(s, lexer);
    }
    return false;
  }

  // Subsequent tokens on the line: inline atoms or remark.
  if (valid_symbols[TOK_REMARK_TEXT] || valid_symbols[TOK_INLINE_ATOM_TEXT]) {
    return scan_inline_atom_or_remark(s, lexer, valid_symbols);
  }

  return false;
}
