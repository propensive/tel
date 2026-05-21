# TEL Specification Draft

## Abstract

TEL is a line-oriented, tree-structured, typed data language designed for data that is read, written
and maintained by _humans_, intelligent _agents_ or deterministic _processors_.

TEL defines a **presentation model** that preserves comments, document structure and user data
through programmatic round-trips, while permitting minor normalizations such as collapsing
space-only blank lines to empty lines. A schema-driven **semantic model** ascribes types to every
node in the tree. The two models are connected by a deterministic type-assignment algorithm. A
companion specification, [BinTEL](bintel-spec.md), defines a compact binary encoding that provides
an unambiguous serialization of the semantic model.

The design of TEL is motivated by the following goals:

- **Formatting preservation.** Machine edits should not disturb formatting, comments or whitespace
  on lines they do not semantically change, so that line-based version control produces minimal,
  meaningful diffs.
- **Minimal escaping.** Syntax conflicts between content and structure should be rare; literal and
  source atoms allow arbitrary content with no character escaping.
- **Strict, recoverable parsing.** Parsing is unambiguous and every error condition has a defined
  recovery strategy, so that a single mistake does not shadow subsequent errors.
- **Schema-driven typing.** Every node is typed by a schema. Validation, including string-level
  constraints, is an integral part of the format rather than an external layer.
- **Layered extensibility.** Schemas support append-only layering, enabling forwards-compatible
  extensions with clear compatibility semantics.
- **Human and machine editors.** The format is designed for direct human authorship, IDE tooling
  with immediate feedback, programmatic transformation, and AI-assisted editing alike.

## 1. Status

This document is a draft specification of TEL.

Where this draft contains `FIXME` notes, the corresponding behavior is not yet fully specified and
MUST NOT be considered stable.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Overview

TEL is a Unicode, character-based language for ordered, tree-structured data represented as strings,
and typed according to a schema.

TEL presents data as an _ordered_ tree, however an application consuming TEL MAY choose to assign
meaning to sibling order, or MAY treat it as insignificant. In this respect, TEL is similar to XML.

TEL distinguishes between:

- a **presentation model**, which preserves comments, interpreter directives, pragma metadata, atom
  presentation form, most whitespace and document structure sufficiently for faithful
  reserialization, and
- a **semantic model**, which is derived from the presentation model using a schema.

This document specifies TEL source, its parsing into the presentation model, the definition of
schemas and translation between presentation model and semantic model by means of a schema.

## 4. Character Encoding

TEL is defined over Unicode code points.

When written to a file, a TEL document MUST be encoded as UTF-8.

Line endings in a TEL document are governed by the following rules (literal atom payloads, defined
in §16, are exempt from all of them):

1. The line-ending style is uniform across the entire document: either every line ends with `LF`, or
   every line ends with `CR LF`.
2. The **line-ending mode** is determined by the first `CR` or `LF` character in the document: if it
   is `CR`, the mode is **CRLF mode**; otherwise the mode is **LF mode**.
3. In CRLF mode, `CR` and `LF` may only appear as part of a `CR LF` line ending, except within
   literal atoms (**E123**).
4. In LF mode, `CR` may not appear anywhere in the document, except within literal atoms (**E123**).

LF mode is the default and RECOMMENDED line-ending style. Human authors SHOULD use LF endings but
MAY use CRLF endings. Agents and processors MUST use LF endings when creating new documents. When
modifying an existing document, agents and processors SHOULD NOT change its line-ending mode.

No Unicode normalization is required or implied. TEL is defined over the exact Unicode code points
that appear in the serialized text.

A UTF-8 byte order mark MUST NOT appear in a TEL document (**E101**).

Visually misleading code points, such as zero-width characters, SHOULD be avoided. Control-heavy
content SHOULD be avoided except where required. TEL is not intended primarily as a binary-data
format, even though it can represent content containing non-printing code points.

## 5. Significant Characters and Terms

The following three characters have syntactic significance in TEL:

- `U+000A` LINE FEED (`LF`)
- `U+0020` SPACE
- one other symbolic character designated as the **sigil**

The following definitions apply:

A **line** is a contiguous, potentially empty sequence of non-linefeed characters delimited by `LF`
characters or by the start or end of the file. In CRLF mode (§4), the `CR` immediately preceding
each delimiting `LF` is part of the line terminator and is not part of the line's content.

A **soft space** is exactly one `U+0020` SPACE character.

A **hard space** is two or more consecutive `U+0020` SPACE characters.

A **blank line** is a line containing only `U+0020` SPACE characters, or no characters at all.

A **parenthetical symbol** is one of the six bracket characters: `(`, `)`, `[`, `]`, `<`, `>`, `{`,
`}`.

A **phrase** is a maximal contiguous sequence of non-linefeed, non-separator characters on a line,
where separators are determined by the phrase-separation rules. A phrase MAY contain soft spaces;
see §10.3.

The **beginning** of a non-blank line is the first non-space character on the line.

An **ordinary line** is any non-blank line that is not a comment line (§11.1), a tabulation line
(§11.2), or a payload line of a source atom (§15) or literal atom (§16).

## 6. Root Structure

A parsed TEL document has the following root structure:

```typescript
interface Document {
  directive: string | null;
  pragma: Pragma | null;
  lineEndings: "LF" | "CRLF";
  children: Block[];
}

interface Pragma {
  version: [number, number];
  schema: string | null;
  sigil: Sigil | null;
}

type Sigil =
  | "!"
  | '"'
  | "#"
  | "$"
  | "%"
  | "&"
  | "'"
  | "*"
  | "+"
  | ","
  | "-"
  | "."
  | "/"
  | ":"
  | ";"
  | "="
  | "?"
  | "@"
  | "\\"
  | "^"
  | "_"
  | "`"
  | "|"
  | "~";
```

## 7. Interpreter Directive

If the first two characters of the document are `#!` (`NUMBER SIGN`, `EXCLAMATION MARK`), then the
first line of the document is an interpreter directive line. If not, the interpreter directive is
absent.

The interpreter directive payload is the content of the first line after the leading `#!`, up to but
excluding the line terminator.

If a document has an interpreter directive and also has a pragma, then the pragma MUST appear after
the interpreter directive.

An interpreter directive line is not part of the `children` sequence. It is not part of the semantic
content of the document, but should be invariant under reserialization.

## 8. Pragma

If present, the pragma MUST be the first non-blank line after any interpreter directive line, and is
parsed as an ordinary line (**E102**).

If present, the entire pragma line MUST be fully contained within the first 4096 bytes of the
document (**E103**).

The first phrase on the pragma line (its keyword, as defined in §10) MUST be `tel`. The keyword
`tel` is reserved: it MUST NOT appear as a `Field.keyword` or `Variant.keyword` in any `Struct`
within a schema (**E211**).

The pragma line MUST contain at most three phrases after `tel` (version, schema identifier, and
sigil). Any additional phrases are invalid (**E125**). A pattern of the form `<sigil> <text>` that
would otherwise be treated as an inline comment does not apply on the pragma line; any such content
is invalid (**E125**).

The positional form of the pragma is:

```text
tel 1.0 schema-id #
```

The parameters are interpreted in order as follows:

1. TEL version
2. schema identifier
3. sigil

The version parameter MUST have the form `x.y`, where `x` and `y` are non-negative integers
(**E104**). `x` is the major version and `y` is the minor version.

The following rules govern how the version number changes across revisions of this specification:

- A revision that rejects a document that would have been accepted by the previous revision MUST
  increment the major version.
- A revision that accepts a previously accepted document but assigns it a different interpretation
  in its presentation or semantic model MUST increment the major version.
- A revision that accepts documents which would not have been accepted by an earlier revision, but
  does not reject or reinterpret any previously valid document, MUST keep the same major version and
  increment the minor version.

The schema identifier parameter is optional.

The sigil parameter is optional.

The sigil MUST be a single ASCII symbolic character. It MUST NOT be SPACE, LINEFEED, CARRIAGE
RETURN, a letter, a control character, a digit, or a parenthetical symbol (§5) (**E106**).

The default sigil is `#`, used unless the pragma or the document schema specifies a different one.

### 8.1 Schema Identifier

The schema identifier, if present, MUST be one of:

- an HTTP or HTTPS URL, optionally with a fragment (the `#` separator and everything after it) that
  is the BASE64-URL-encoded (no padding) SHA-256 hash of the [BinTEL](bintel-spec.md) representation
  of the schema
- a bare BASE64-URL-encoded (no padding) SHA-256 hash of the [BinTEL](bintel-spec.md) representation
  of the schema

A schema identifier that does not match either of these forms is invalid (**E124**).

The `#` used in the URL form is the standard URI fragment separator (RFC 3986 §3.5). A bare hash is
distinguished from a URL by the absence of a `://` substring. Because the BASE64-URL alphabet
contains no space characters, a schema identifier always occupies a single phrase.

A **schema signature** is a deterministic byte string derived from the SHA-256 hashes of the
schema's components (base schema and any layers). It uniquely identifies a composed schema and
enables verification of schema identity and compatibility. The full construction and decoding
algorithm for schema signatures is defined in the [BinTEL Specification](bintel-spec.md).

### 8.2 Schema Resolution

A schema may be supplied in two independent ways when parsing a TEL document:

- an **invocation schema**, supplied directly to the parser by the calling application
- a **document schema**, identified by the schema parameter in the pragma

The following table defines the outcome for each combination:

| Invocation schema | Document schema       | Outcome                                                      |
| ----------------- | --------------------- | ------------------------------------------------------------ |
| absent            | absent                | Untyped document; only the presentation model is available   |
| absent            | present               | Semantic model available, but types are not statically known |
| present           | absent                | Semantic model available with statically known types         |
| present           | present, matching     | Same as invocation-only; types are statically known          |
| present           | present, compatible   | Parsed with invocation schema; types are statically known    |
| present           | present, incompatible | Error                                                        |

Types are **statically known** when the schema is available at compile time (or equivalent) in the
host language, enabling type-safe access through generated types, type providers, or similar
mechanisms. When types are not statically known, the semantic model is still available but must be
accessed through a dynamic or generic interface.

Two schema identifiers **match** if:

- both carry a hash, and the hashes are identical; or
- neither carries a hash, and the URLs are identical

A schema identifier that carries a hash takes precedence for matching purposes: a URL-only
identifier and a URL-with-hash identifier for the same URL do not automatically match (the hash is
authoritative).

A schema with signature A is **compatible** with a schema with signature B if the decoded hash
sequence of A is a subsequence of the decoded hash sequence of B. That is, A's components (base and
layers) appear in B in the same order, but B may include additional layers between or after them.
Compatibility is directional: if A is compatible with B, a reader expecting schema B can read a
document written against schema A, because B's composed type structure is a superset of A's (layers
are append-only). The converse does not hold: a reader expecting A cannot necessarily read a
document written against B, since B may contain members unknown to A.

If a schema URL is specified but the schema cannot be retrieved, it is a runtime error.

### 8.3 Sigil Resolution

The sigil is determined in the following order of increasing precedence:

1. The default sigil (`#`)
2. The sigil declared by the resolved schema, if any
3. The sigil specified in the pragma, if present

The sigil MUST be determined before parsing any content after the pragma line. If the effective
sigil requires the schema (i.e., the pragma does not specify a sigil and the schema may declare
one), the parser MUST resolve the schema before continuing.

The sigil declared by a schema is given by the `sigil` field of the `Schema` type, whose structure
is defined in the Schema Language section.

The pragma line is not included in the `Document.children` sequence. It is recorded only in the
`Document.pragma` field.

## 9. Lines, Margin, and Indentation

A TEL document MAY begin with zero or more blank lines.

A document containing no non-blank lines (other than an interpreter directive or pragma) is valid
and has an empty `children` list.

If the document begins with an interpreter directive, the **margin** is zero. Otherwise, if the
document contains at least one non-blank content line, the **margin** is the sequence of leading
spaces on the first such line. If the document contains no non-blank content lines, the margin is
zero.

Every non-blank line in the document MUST begin with the margin, optionally followed by additional
spaces. A non-blank line which does not begin with the margin is invalid (**E108**).

For each non-blank line, the number of spaces following the margin MUST be even (**E109**). The
**indent** is defined as one half of the number of spaces between the margin and the first non-space
character.

Therefore, after removing the margin, indentation is measured in units of two spaces, and the first
non-blank line necessarily has indent `0`. Blank lines have no defined indent.

Trailing spaces on a non-blank ordinary line are not permitted (**E110**).

Blank lines have no structural effect, except as explicitly noted in the sections defining tabulated
blocks, source atoms, and literal atoms.

## 10. Keywords and Inline Atoms

Each non-blank ordinary line is parsed into one or more phrases by the phrase-separation rule
(defined in §10.3 below).

The first phrase on a non-blank ordinary line is the **keyword**.

Each subsequent phrase on that line is an **inline atom**.

### 10.1 Keyword Characters

A keyword may contain any Unicode code point except `U+0020` SPACE and `U+000A` LINE FEED.
Non-printing code points are NOT RECOMMENDED in keywords but are not forbidden. Although non-ASCII
keywords are permitted, ASCII keywords are generally RECOMMENDED for interoperability and
readability.

### 10.2 Inline Atom Characters

An inline atom may contain any Unicode code point other than `LF`, subject to the phrase-separation
rules defined in §10.3 below.

### 10.3 Phrase-Separation Rule

After the keyword, the line is parsed left-to-right.

Initially, a single space is sufficient to terminate a phrase and begin the next phrase.

If a hard space is encountered anywhere on the same line after the keyword, then from that point
onward only hard spaces terminate phrases. Soft spaces after that point become part of the current
phrase.

Accordingly:

- before the first hard space on a line, either a soft space or a hard space terminates a phrase
- after the first hard space on a line, only a hard space terminates a phrase
- after the first hard space on a line, a soft space becomes content within the current phrase
- each new line resets this rule

Consequently, after the first hard space on a line, a phrase may contain soft spaces but may not
contain hard spaces.

## 11. Comments, Tabulations, and Remarks

TEL distinguishes between:

- a **comment**, which occupies an entire line and is represented as a line-level presentation node,
- a **tabulation**, which occupies an entire line and represents the start of a tabulated block, and
- a **remark**, which is attached to a compound line and is not an ordinary child node.

The document's **sigil** — the character that introduces comments, tabulations, and remarks — is
determined by the resolution rules in §8.3.

### 11.1 Comment

A line is a comment line if, after its leading indentation, its keyword is exactly equal to the
sigil, and the line does not qualify as a tabulation line. A line qualifies as a tabulation line if
at least one further occurrence of the sigil appears on the line preceded by a hard space; in that
case the line is a tabulation line and not a comment line, regardless of any other content.

If the sigil is followed immediately by the end of line, the comment payload is the empty string.

If the sigil is followed by a soft space, the comment payload begins at the first character after
that soft space and continues unchanged to the end of the line.

A phrase such as `#foo` (the sigil concatenated with other characters) is not a comment keyword.
This makes it possible to use the sigil as part of a word.

The payload of a comment is not further parsed. Spaces inside the payload are preserved exactly.

Comments participate in indentation and structural ordering as line-level nodes. Comments cannot
have children.

A comment line MUST be immediately preceded by one of the following: a blank line, another comment
line, the start of the document (i.e., a comment may be the very first non-blank line), or a line at
a lesser indent (i.e., a comment may appear after a compound if it is indented one level deeper than
that compound) (**E111**). Because a blank line terminates any active tabulated block, this rule
ensures that comments cannot appear inside tabulated blocks.

A comment is **attached** to the immediately following node if there is no blank line between the
comment and that node. The following node may be a compound node, or a tabulation line (in which
case the comment is attached to the tabulated block that the tabulation line introduces). The
attached node MUST be at the same indentation level as the comment. A comment that is followed by a
blank line, by end of input, or by a line at a shallower indentation level is **free-standing**.

Comment attachment is a semantic property recorded in the presentation model. It is significant
during programmatic editing: when a node is moved or deleted, its attached comments travel with it
or are removed with it.

### 11.2 Tabulation

A line is a tabulation line if, after its leading indentation, its first non-space character is the
sigil, and at least one further occurrence of the sigil appears on the line immediately preceded by
a hard space.

Each marker occurrence on a tabulation line is identified as follows: the first non-space character
(M_0) is always a marker; any subsequent occurrence of the sigil that is immediately preceded by a
hard space is also a marker (M_1, M_2, …).

A tabulation line is represented as a distinct presentation node. Remarks are not applicable to
tabulation lines; any content on a tabulation line that would otherwise form a remark is instead
part of the heading text for the final column.

The markers on a tabulation line are ordered by position. Let their character offsets from the start
of the line, after removing the document margin, be M_0 < M_1 < ... < M_n, where n ≥ 1. The first
marker (at M_0) is the line's keyword and carries no column semantics. Each subsequent marker
defines a column of the tabulated block: marker at M_i (for i = 1, ..., n) defines the start of
**column i**. Columns are numbered from 1.

For each non-final column i (where 1 ≤ i < n), its maximum content width is M\_{i+1} − M_i − 2 code
points. The final column (i = n) is unbounded.

**Column headings.** Each marker M_i is followed by a **column heading**, parsed as follows:

- If M_i is immediately followed by end of line, the heading is the empty string.
- If M_i is immediately followed by exactly one space (a soft space), the heading is the text
  beginning after that space and ending immediately before the first hard space encountered, or at
  end of line if no hard space follows. If the heading ends at a hard space, the character
  immediately after that hard space MUST be the sigil (i.e., M\_{i+1}) (**E122**). The heading text
  MUST NOT itself contain the sigil (**E122**).
- If M_i is immediately followed by two or more spaces (a hard space), the character immediately
  after those spaces MUST be the sigil (i.e., M\_{i+1}), and the heading for M_i is the empty string
  (**E122** if not).
- Any other character immediately following M_i (including a non-space character) is invalid
  (**E122**).

The column heading for M_0 labels the keyword and pre-column area of rows. The column heading for
M_i (i ≥ 1) labels column i and is positioned within column i's span on the tabulation line.

Column headings are preserved in the `Tabulation` node as an ordered list parallel to
`markerOffsets`. An empty string heading is permitted.

Examples:

- `# ID  # Name  # Age` — three markers; headings `["ID", "Name", "Age"]`
- `#  # Name  # Age` — M_0 followed by hard space then M_1; headings `["", "Name", "Age"]`
- `# ID  #  # Age` — M_1 followed by hard space then M_2; headings `["ID", "", "Age"]`
- `# foo  # # bar` — invalid (**E122**): heading for M_1 would contain the marker
- `# foo  #  bar  # baz` — invalid (**E122**): M_1 followed by hard space not immediately preceding
  a marker

### 11.3 Remark

A remark is attached to the compound defined by its line.

A remark begins when the sigil appears at the start of a phrase and is immediately followed by
exactly one soft space. Whether a given occurrence of the sigil is at the start of a phrase depends
on the phrase-separation mode in effect at that point (§10.3).

Accordingly:

- the sigil followed by end of line is an ordinary phrase, not a remark introducer
- the sigil followed by a hard space is an ordinary phrase, not a remark introducer
- the sigil not preceded by a phrase boundary in the current mode is ordinary content within the
  current phrase
- the sigil at a phrase boundary followed by a soft space introduces a remark

The remark payload begins at the first character after that soft space and continues unchanged to
the end of the line.

The sigil itself is not part of the remark payload.

A remark payload is not further parsed.

A compound may have at most one remark.

Remarks do not terminate or split a tabulated block.

## 12. Presentation Nodes

The presentation-layer node types are:

```typescript
interface Compound {
  keyword: string;
  atoms: Atom[];
  remark: string | null;
  children: Block[];
}

interface Block {
  comments: Comment[];
  tabulation: Tabulation | null;
  compounds: Compound[];
  trailingBlankLines: number;
}

interface Comment {
  text: string;
}

interface Tabulation {
  markerOffsets: number[];
  headings: string[];
}

type Atom = Inline | Source | Literal;

interface Inline {
  text: string;
  precedingSpaces: number;
}

interface Source {
  lines: string[];
}

interface Literal {
  delimiter: string;
  text: string;
}
```

These distinctions are presentation-only.

In the semantic model, all atom presentation forms are just atoms, and the distinction between atom
and compound disappears in favor of typed nodes with types.

A `Block` is the primary structural grouping within a compound's children. It consists of, in order:

- zero or more **comments** (contiguous, with no blank lines between them)
- an optional **tabulation** line, which applies to all compounds in the block
- zero or more **compound children** (rows if the tabulation is present, ordinary children
  otherwise)
- a count of **trailing blank lines**: the number of blank lines that follow the last compound (or,
  if compounds is empty, the last comment) in the block

A block has at most one tabulation. If a tabulation is present, it MUST appear after any attached
comments and before the first compound child. A block with a tabulation MUST have at least one
compound child (row).

A block whose `compounds` list is empty and whose `comments` list is non-empty represents a
free-standing comment group (a comment or comments not immediately followed by any compound at the
same level). Such a block has no tabulation.

Each `Atom.Inline` records the number of spaces immediately preceding it on its source line. Each
`Atom.Literal` records the delimiter string used to open and close it, in addition to the payload
text.

## 13. Compound Tree Structure

Each non-comment non-tabulation ordinary line defines a `Compound` node whose keyword is the line
keyword.

Each subsequent inline atom after the keyword defines an `Atom.Inline` attached to that compound,
unless superseded by the remark rule.

After its inline atoms, a compound may have zero or more child blocks (§12), determined by
indentation and blank-line structure.

## 14. Parent, Child, and Peer Relations

For each non-blank line after the first non-blank line, excluding lines consumed by source atoms or
literal atoms, let the **previous compound line** be the most recent preceding non-blank compound
line (i.e., excluding comment lines and tabulation lines):

- if its indent is exactly one greater than that of the previous compound line, it is a child of the
  previous compound line
- if its indent is equal to that of the previous compound line, it is a peer of the previous
  compound line
- if its indent is less than that of the previous compound line, it closes one or more open
  compounds and becomes a peer of the nearest preceding compound line with the same indent; if no
  preceding compound line has the same indent, the document is invalid (**E112**)

A line may not have indent greater than one plus the indent of the previous compound line, except
where the source-atom or literal-atom rules apply (**E113**).

Comments and tabulations follow the same indentation and peer/child rules as compounds during
parsing, except that comments and tabulations cannot have children. A line that would become a child
of a comment or tabulation is invalid (**E114**). In the resulting presentation model, comments and
tabulations are absorbed into `Block` nodes and do not appear as standalone siblings of compounds.

## 15. Source Atoms

If a line immediately follows a compound line with no intervening blank line, and its indent is
exactly two greater than that compound line's indent, then it begins a source atom, provided:

- the preceding compound does not already have a source atom or literal atom

A source atom is represented in the presentation model as `Atom.Source(lines)` and is appended to
the end of the atom sequence of the immediately preceding compound.

A compound may have at most one source atom. Introducing a source atom when the preceding compound
already has a source or literal atom is invalid (**E115**).

The source atom begins on the double-indented line and includes that line together with each
subsequent line until either:

- the end of the document is reached, or
- a non-blank line is encountered whose indent is less than the indent of the first source-atom line

Blank lines are permitted within a source atom.

Each captured line becomes one element of the `lines` array, in order. The array therefore contains
one entry per line, including blank lines.

For each non-blank captured line, exactly the indentation of the first source-atom line is stripped
from the start of the line. Any surplus leading spaces are preserved.

For each captured line, trailing spaces are stripped. (Source-atom lines are not ordinary lines, so
E110 does not apply to them; trailing spaces are silently removed rather than being an error.)

A blank line within a source atom is represented as an empty string in the `lines` array, regardless
of how many spaces it physically contains.

Line content is otherwise captured literally. In particular, the sigil has no special meaning inside
a source atom.

Source-atom lines are subject to the normal line rules (§5): in CRLF mode, the `CR` preceding each
`LF` is part of the line terminator and is not part of the line content. Consequently, each element
of `lines` contains only the line content, with no line-ending characters.

Source-atom lines are not compounds and are never members of a tabulated block. A source atom always
terminates any surrounding tabulated block.

After a source atom ends, parsing resumes normally. The next non-source-atom line is evaluated for
indentation relative to the compound that introduced the source atom, as if the source atom lines
were not present.

## 16. Literal Atoms

If a line immediately follows a compound line with no intervening blank line, and its indent is
exactly three greater than that compound line's indent, then it begins a literal atom, provided:

- the preceding compound does not already have a source atom or literal atom

A literal atom is represented in the presentation model as `Atom.Literal(text)` and is appended to
the end of the atom sequence of the immediately preceding compound.

A compound may have at most one literal atom. Introducing a literal atom when the preceding compound
already has a source or literal atom is invalid (**E116**).

The opening literal-atom line is not part of the payload.

The remainder of that opening line, from its beginning up to but excluding the line terminator, is
the delimiter.

The delimiter MUST consist only of ASCII characters other than whitespace (spaces, linefeeds,
carriage returns, tabs, and other ASCII control characters).

If the delimiter is empty, the line does not begin a literal atom.

The literal payload begins immediately after the `LF` that terminates the delimiter line.

The closing delimiter is identified by scanning for a `LF` immediately followed by the exact
delimiter characters and then immediately followed by another `LF`. This scan uses bare `LF`
characters regardless of the document's line-ending mode; the `LF` characters that structurally
delimit the literal atom (the opening `LF`, the `LF` before the closing delimiter, and the `LF`
after the closing delimiter) are exempt from the CRLF mode requirement. The closing delimiter match
is performed against the raw byte stream, without any margin stripping or indentation processing.
The payload is everything between the opening `LF` (exclusive) and the closing `LF` before the
delimiter (exclusive). The `LF` after the closing delimiter terminates the literal atom.

Accordingly, an empty literal payload (a `LF` immediately followed by the delimiter and a `LF`) is
permitted.

The literal payload preserves leading spaces, trailing spaces, internal spaces, and all other
content exactly.

If the end of file is reached before a closing delimiter is encountered, the document is invalid
(**E117**).

The sigil has no special meaning inside a literal atom.

The line-ending mode rules of §4 do not apply inside a literal atom payload or to the structural
`LF` characters that bound it. `CR` characters within the payload are preserved exactly as-is and
carry no special meaning; only bare `LF` is recognised as a line separator for the purpose of
identifying the closing delimiter. In particular, a `CR` immediately before a `LF` inside the
payload is payload content, not a line terminator.

Literal atom payload content is raw: it is not subject to any TEL parsing rules. Indentation,
trailing spaces, and all other content are preserved exactly. The only termination condition is a
`LF` immediately followed by the delimiter and another `LF`.

Literal-atom lines are not compounds and are never members of a tabulated block. A literal atom
always terminates any surrounding tabulated block.

After the closing delimiter line and its line terminator, parsing resumes normally. The next
non-literal-atom line is evaluated for indentation relative to the compound that introduced the
literal atom, as if the literal atom lines were not present.

## 17. Tabulated Blocks

A **tabulated block** begins immediately after a tabulation line and continues through each
subsequent non-blank line until a blank line is encountered or the end of the document is reached.
Lines within a tabulated block (other than the tabulation line itself) are called **rows**.

In the presentation model, a tabulated block is represented as a `Block` whose `tabulation` field
holds the tabulation line and whose `compounds` list holds the rows.

A second tabulation line appearing within a continuous run of rows (without an intervening blank
line) terminates the current `Block` and begins a new `Block` with the new tabulation. The new
block's `trailingBlankLines` on the preceding block is zero, indicating that no blank lines separate
the two tabulated sub-blocks.

Every non-blank, non-comment row MUST be an ordinary compound line. Every row MUST have the same
indent as the tabulation line (**E118**). Rows MUST NOT have child line-nodes (**E114**).

**Row structure.** Each row consists of a keyword and zero or more **pre-column atoms**, followed by
zero or more **column values**. The keyword and pre-column atoms are parsed using the same
phrase-separation rules as ordinary lines (§10.3). Column values are introduced by the column
positions defined by the tabulation line.

**Spacing constraints.** The following two rules govern the spacing on every row:

1. Every contiguous run of two or more space characters (a hard space) MUST end at position M_i − 1
   for some column i that is present on the row (**E119**).
2. No two consecutive space characters may appear at any other position on the row (that is, within
   the keyword, within pre-column atoms, or within a column value) (**E120**).

These rules together imply:

- the keyword and pre-column atoms are separated from each other by exactly one space
- before each present column i there is exactly one hard space run, ending at M_i − 1
- column values contain no internal consecutive spaces

**Column presence and values.** Column i is **present** on a row if the row contains space
characters at both position M_i − 2 and position M_i − 1 (the mandatory minimum for the hard-space
separator). Column i is **absent** from a row if the row ends before reaching position M_i − 2; a
row need not specify all columns and may omit any suffix of columns.

A present column has an **empty value** if position M_i is itself a space character or the row ends
at position M_i − 1. An empty value requires that the subsequent column (if any) is also present,
since otherwise the separator spaces at M_i − 2 and M_i − 1 would be trailing spaces, which are not
permitted. A row MUST NOT have trailing spaces (**E110**).

**Omitted column semantics.** When a schema is available, an absent column is interpreted according
to the schema member that corresponds to that column's position: if the member has a `Scalar`
type with a non-null `default`, the default value is used; if the member is not `required`, the
member is treated as absent (unfilled); if the member is `required` and has no default, the document
is invalid (**E307**).

**Width constraint.** For each present non-final column i, its value MUST NOT exceed M\_{i+1} − M_i
− 2 code points in width (**E121**). The final column is unbounded.

**Remarks.** Remarks are permitted on rows. The hard space that introduces a remark, and the remark
payload itself, are exempt from the column spacing constraints and are not subject to column-width
limits.

If a row violates any of these constraints, the document is invalid (see **E118** through **E121**).

## 18. Presentation Model and Semantic Model

TEL defines both a presentation model and a semantic model.

### 18.1 Presentation Model

The presentation model is constructed during parsing. When a schema is available, parsing and type
assignment proceed together: the schema informs error recovery decisions (particularly for
indentation errors, whose recovery algorithm is defined in the Error Recovery subsection below) that
cannot be resolved from syntax alone.

It preserves:

- the optional interpreter directive
- the optional pragma
- compounds and their keywords, atoms, and remarks
- the block structure: for each block, its attached comments, its optional tabulation, its ordered
  compound children, and its trailing blank line count
- atom presentation form (`Inline`, `Source`, or `Literal`)
- for each inline atom, the number of spaces immediately preceding it
- ordering and structure derived from indentation

A conforming serializer MUST produce output that, when parsed, yields an identical presentation
model. Specifically, the serialized output MUST preserve:

- all compounds, with their keywords, atoms, remarks, and children
- all blocks, with their comments, tabulations (including marker offsets and headings), compounds,
  and `trailingBlankLines` counts
- for each `Inline` atom, the `precedingSpaces` count
- document-level fields: interpreter directive, pragma, and children order

A serializer MAY apply the following normalizations, since the affected details are not recorded in
the presentation model:

- Blank line content MAY be normalized to empty lines (rather than space-only lines).
- A minimum hard space (exactly two spaces) MAY be used before remark introducers.
- Multiple consecutive trailing blank lines at the end of a block MAY be collapsed to the recorded
  `trailingBlankLines` count.

All other presentation-model details MUST be reproduced exactly. In particular, the round-trip
guarantee preserves: all compounds with their keywords, atoms, remarks, and children; all block
structure including comments, tabulations, and ordering; atom presentation form and
`precedingSpaces`; and the `Literal.delimiter` string.

### 18.2 Semantic Model

The semantic model is derived from the presentation model by applying the type assignment algorithm
(defined in the Schema Language section) during parsing. The result is a tree of `Element` values:

```typescript
type Element = Node | Value;

interface Node {
  keywordIndex: number;
  type: Type;
  children: Element[];
}

interface Value {
  keywordIndex: number;
  type: Scalar;
  text: string;
}
```

A `Node` represents a `Struct`-typed or `Flag`-typed element. `Node.type` is the `Type` assigned by
the type assignment algorithm (defined in the Schema Language section). `Node.children` is the
ordered list of child elements; for a `Flag`-typed node, `children` is always empty.

A `Value` represents a `Scalar`-typed element. It is a leaf: it carries the atom text in
`Value.text` and has no children.

Every element carries a `keywordIndex`, which is the position of the element's keyword in the
keyword order of the parent's `Struct` type (as defined in the Schema Language section). For the
document root, `keywordIndex` is not applicable. The `keywordIndex` identifies which member (and,
for `Select` members, which variant) the element fills, and is sufficient to recover the keyword string
from the schema.

The interpreter directive, pragma, comments, tabulations, and remarks are not part of the semantic
model. There is a one-to-one mapping between presentation-layer atoms and compounds on the one hand,
and elements on the other: every atom and every compound maps to exactly one element.

### 18.3 Mapping Procedure

The mapping from presentation model to semantic model proceeds as follows. The type assignment
algorithm (defined in the Schema Language section) ascribes a type to every atom and compound. Given
these assignments, the semantic tree is constructed by:

1. **Document root.** Create a root `Node` with `type` equal to `Schema.document` and `children`
   constructed from steps 2–5.

2. **Compound children.** For each compound child C of the current node (iterating across all blocks
   in order, skipping comments and tabulations):
   - If the type assigned to C is `Struct` or `Flag`, create a `Node` with that type and `children`
     constructed by recursing into C's children (empty for `Flag`).
   - If the type assigned to C is `Scalar`, create a `Value` with that type and `text` equal to
     the compound's inline atom text if present, or the empty string if the compound has no inline
     atoms.

3. **Atoms.** For each atom A assigned to the current node (in order):
   - If the assigned type is `Scalar`, create a `Value` with that type and `text` equal to A's
     text.
   - If the assigned type is `Flag`, create a `Node` with that type and an empty `children` list.

4. **Ordering.** Atom-derived nodes and compound-derived nodes for the same member are interleaved
   in the order they were assigned. Atom-derived nodes for a member precede compound-derived nodes
   for the same member (atoms appear on the parent line, before any child lines).

5. **Defaults.** For each required `Field` member with a `Scalar` type and a non-null `default`
   that was not filled by any atom or compound child: create a `Value` with that `Scalar` type
   and `text` equal to `Scalar.default`. This node is placed at the position in the child list
   corresponding to the member's position in member order.

The resulting tree is fully determined by the presentation model and the schema. No ambiguity
remains: the type, value, and child order of every element are fixed by the type assignment
algorithm.

The schema language and type assignment algorithm are defined in the Schema Language section.

## 19. Schema-Governed Structure and Error Diagnosis

In addition to parsing errors, a TEL document may be structurally invalid with respect to a schema.

When a schema is available, it is applied during parsing rather than as a separate post-processing
stage. This is necessary because the schema informs certain error recovery decisions — in
particular, indentation recovery uses keyword validity at candidate indent levels to resolve
ambiguous lines (the algorithm is defined in the Indentation Recovery subsection). The result is a
presentation model and semantic model constructed together in a single pass.

### 19.1 Atom and Compound Interchangeability

Every presentation-layer atom and every presentation-layer compound corresponds to an element, and
every element has a type. The distinction between atom and compound is therefore presentational
rather than semantic.

A child whose type can be uniquely inferred from schema position may be written either as an atom in
atom position or as a compound with an explicit keyword. A child whose type cannot be uniquely
inferred must be written as a compound with an explicit keyword.

Conversely, when a child is written as a compound with an explicit keyword, an implementation may
determine that the same child could have been written positionally as an atom, provided the schema
would have assigned the same type deterministically.

### 19.2 Positional Assignment

For a given parent type, the schema defines an ordered sequence of child specifications.

The order in which child types are specified in the schema determines the order in which inline
atoms are assigned types.

Inline atoms may be assigned types from that ordered sequence so long as the applicable child
specifications are non-repeatable.

If an atom position is assigned to a repeatable child type, then all subsequent inline atoms on that
same compound line must be assigned to that same repeatable child type. Consequently, a repeatable
`Scalar` member may only consume atoms if it is the last atom-assignable member in member order;
no further atoms may be assigned to subsequent members.

Similarly, once atoms are assigned to an all-`Flag` `Select` member, no further atoms may be assigned
to subsequent members, because each atom is matched against the Select's variant keywords and the atom
phase cannot advance past a `Select` member except by skipping it entirely.

For a `repeatable` member, occurrences may be split across both of the following:

- inline atoms on the parent compound line, and
- subsequent compound children of the parent with the same keyword

These two assignment mechanisms may be combined freely. The E309 contiguity rule already prohibits
differently-typed compound children from being interleaved between such occurrences. Remark lines do
not affect this rule.

### 19.3 Error Taxonomy

Errors are identified by a code of the form **E1xx** (parsing), **E2xx** (schema), or **E3xx**
(validation). Parsing errors indicate violations of the presentation-model syntax. Schema errors
indicate a malformed schema. Validation errors indicate that a document does not conform to its
schema.

Each error is referenced by code at the point in the specification where its trigger condition is
defined.

**Diagnostic spans.** Every diagnostic MUST identify the relevant region of the document as a
half-open span `[start, end)` of zero-based code-point offsets from the start of the document. A
zero-width span (where `start = end`) denotes a point location. The span for each error code is
specified in the tables below.

#### Parsing Errors (E1xx)

| Code | Section  | Description                                                                                            | Span                                                                                                                             |
| ---- | -------- | ------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------- |
| E101 | §4       | BOM present at start of document                                                                       | The BOM bytes (`[0, 3)` for a UTF-8 BOM)                                                                                         |
| E102 | §8       | Pragma is not the first non-blank line after any interpreter directive                                 | The `tel` keyword on the misplaced line                                                                                          |
| E103 | §8       | Pragma line extends beyond the first 4096 bytes                                                        | The entire pragma line                                                                                                           |
| E104 | §8       | Pragma version parameter does not have the form `x.y` with non-negative integers                       | The version atom                                                                                                                 |
| E105 | —        | Reserved                                                                                               | —                                                                                                                                |
| E106 | §8       | Pragma sigil is a space, `LF`, `CR`, letter, digit, or parenthetical symbol                            | The sigil atom                                                                                                                   |
| E107 | —        | Reserved                                                                                               | —                                                                                                                                |
| E108 | §9       | Non-blank line begins with fewer than the margin number of spaces                                      | The leading spaces of the line (zero-width at line start if no spaces)                                                           |
| E109 | §9       | Relative indentation after the margin is odd                                                           | The leading spaces of the line; extended through subsequent lines if margin adjustment persists (see Indentation Recovery below) |
| E110 | §9, §17  | Trailing spaces on a non-blank ordinary line or tabulated row                                          | The trailing space characters                                                                                                    |
| E111 | §11.1    | Comment line not preceded by a blank line, another comment, start of document, or lesser-indented line | Zero-width span at the start of the comment line                                                                                 |
| E112 | §14      | Reserved (defined in §14 but not reached by the recursive parsing algorithm described herein; reserved for implementations that track an explicit ancestor stack) | The leading spaces of the line |
| E113 | §14      | Line indent exceeds the preceding non-blank line's indent by more than one                             | The leading spaces of the line                                                                                                   |
| E114 | §14, §17 | Line would become a child of a comment, tabulation, or tabulated row                                   | Zero-width span at the start of the line                                                                                         |
| E115 | §15      | Source atom introduced when the preceding compound already has a source or literal atom                | The first line of the duplicate source atom                                                                                      |
| E116 | §16      | Literal atom introduced when the preceding compound already has a source or literal atom               | The opening delimiter line of the duplicate literal atom                                                                         |
| E117 | §16      | Literal atom reaches end of file before its closing delimiter line                                     | The opening delimiter line                                                                                                       |
| E118 | §17      | Tabulated row has an indent different from the tabulation line                                         | The leading spaces of the row                                                                                                    |
| E119 | §17      | Hard space on a tabulated row does not end at a column start boundary                                  | The misaligned hard-space run                                                                                                    |
| E120 | §17      | Consecutive spaces appear within a keyword, pre-column atom, or column value on a tabulated row        | The consecutive space characters within the value                                                                                |
| E121 | §17      | Column value exceeds the maximum width for that column                                                 | The overflowing column value                                                                                                     |
| E122 | §11.2    | Malformed tabulation line heading                                                                      | The malformed heading region (from the marker to the next marker or end of line)                                                 |
| E123 | §4       | `CR` not immediately followed by `LF`, or line-ending mode inconsistency                               | The `CR` character (or `CR LF` pair that violates the established mode)                                                          |
| E124 | §8.1     | Schema identifier is not a valid URL or bare BASE64-URL hash                                           | The schema identifier atom                                                                                                       |
| E125 | §8       | Pragma line has extra atoms beyond the expected parameters, or contains a remark                       | The first extra atom, or the remark introducer                                                                                   |

Schema errors (E2xx) and validation errors (E3xx) arise from violations of the schema language rules
and document conformance constraints defined in the Schema Language and Validation sections. Their
trigger conditions and diagnostic spans are catalogued at the ends of the Schema Validity
Constraints and Validation subsections respectively.

### 19.4 Error Diagnosis

Error diagnosis in TEL has three layers:

- **parsing diagnosis**, which reports violations of the presentation syntax defined by this
  specification
- **schema diagnosis**, which reports violations that arise when the presentation model is checked
  against a schema and translated into the semantic model
- **validation diagnosis**, which reports violations of constraints in the parsing of elements

These three layers SHOULD be distinguished in diagnostics.

A conforming implementation SHOULD report multiple independent errors when validating a document,
rather than halting at the first error encountered. Each error has a defined recovery strategy
(described in the following Error Recovery subsection) that allows parsing to continue and
subsequent errors to be reported.

Every diagnostic MUST include the error code and the span defined for that error in §19.3. The span
is expressed as a half-open range `[start, end)` of zero-based code-point offsets from the start of
the document.

### 19.5 Error Recovery

For every error condition, a conforming implementation MUST apply the recovery strategy defined here
before continuing. No error SHALL prevent subsequent errors from being reported.

#### Parsing Error Recovery

| Code | Recovery strategy                                                                                                                                                                                                                                                                                                                   |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| E101 | Ignore the BOM and continue parsing from the next byte.                                                                                                                                                                                                                                                                             |
| E102 | Restart parsing the entire document using the version, schema identifier, and sigil extracted from the misplaced pragma.                                                                                                                                                                                                            |
| E103 | Allow the pragma line to exceed the 4096-byte limit and continue parsing its content normally.                                                                                                                                                                                                                                      |
| E104 | If the version parameter cannot be parsed as `x.y` at all, ignore it and parse with the latest known version. If it has the correct format but names an unknown version, use the most recent minor version of the same major version if one is known; if the major version itself is unknown, use the latest known version overall. |
| E106 | Ignore the invalid sigil and use the default sigil (`#`) instead.                                                                                                                                                                                                                                                                   |
| E108 | If the line has exactly one fewer leading space than the current margin, insert a synthetic leading space and parse the line at the current indentation level normally. If the line has two or more fewer leading spaces than the current margin, reset the margin to the line's actual indentation level from that point forward.  |
| E109 | Parse the line's keyword; check which of the two candidate indent levels (±1 space) makes the keyword valid according to the schema; adjust the margin accordingly. See indentation recovery algorithm below.                                                                                                                       |
| E110 | Ignore trailing spaces and parse the remainder of the line normally.                                                                                                                                                                                                                                                                |
| E111 | Ignore the missing preceding blank line (or other required predecessor) and treat the comment as normally attached to the following node.                                                                                                                                                                                           |
| E112 | Reserved. If a future implementation triggers E112, use the same recovery as E109.                                                                                                                                                                                                                                                  |
| E113 | The over-indented line is skipped (omitted from the presentation model) and parsing continues with the following line at the original expected indent.                                                                                                                                                                              |
| E114 | Same indentation recovery as E109: the line cannot be a child of its apparent parent (a comment, tabulation, or row), so treat it as if indented one level less and use the schema to validate the adjusted placement.                                                                                                              |
| E115 | Ignore the duplicate source atom; use the first one encountered.                                                                                                                                                                                                                                                                    |
| E116 | Ignore the duplicate literal atom; use the first one encountered.                                                                                                                                                                                                                                                                   |
| E117 | Treat the unclosed literal atom's payload as everything from the opening delimiter line to the end of file (excluding the final `LF`, if any).                                                                                                                                                                                      |
| E118 | Interpret the tabulated row according to its actual hard-space positions regardless of alignment with column markers. Suppress any further alignment errors (E119, E120, E121) on the same row.                                                                                                                                     |
| E119 | Same as E118.                                                                                                                                                                                                                                                                                                                       |
| E120 | Same as E118.                                                                                                                                                                                                                                                                                                                       |
| E121 | Same as E118.                                                                                                                                                                                                                                                                                                                       |
| E122 | Report the error and continue parsing, but disable column-alignment checking for the remainder of the current tabulated block.                                                                                                                                                                                                      |
| E123 | Treat any malformed sequence of consecutive `CR` and `LF` characters as a single line break if it contains at most one `CR` and at most one `LF`; treat it as two line breaks if either `CR` or `LF` appears more than once in the sequence.                                                                                        |
| E124 | Ignore the invalid schema identifier and continue parsing as if no schema identifier were specified. The document is treated as untyped.                                                                                                                                                                                            |
| E125 | Ignore the extra atoms and any remark on the pragma line. Parse the pragma using only the first three atoms (version, schema identifier, sigil).                                                                                                                                                                                    |

#### Validation Error Recovery

All validation errors (E301 through E311) are self-contained: they do not have cascading effects on
the remainder of the type assignment or validation process. An implementation MUST record the error
and continue processing remaining nodes as if the erroneous node were absent or were assigned the
most plausible available type. Specific recovery notes:

- **E311** (`Flag` compound with atoms or children): ignore the atoms and children of the `Flag`
  compound; treat it as a bare keyword with no content.

#### Indentation Recovery (E109, E113)

When a line's relative indentation after the margin is odd (E109), the line sits between two valid
indentation positions: one space deeper and one space shallower. Recovery uses the schema to resolve
the ambiguity.

**Algorithm.** When an odd-indented line is encountered:

1. Parse the line to determine its keyword.
2. Let the two candidate indent levels be _deeper_ (adding one space to align to the next even
   level) and _shallower_ (removing one space to align to the previous even level).
3. If adjusting the margin by +1 space would make the keyword valid at the resulting indent level
   (according to the schema's expected types at that position), adopt that adjustment. If adjusting
   by −1 space would make the keyword valid, adopt that adjustment instead. If both or neither
   direction produces a valid keyword, the implementation SHOULD prefer the shallower
   interpretation.
4. Record an E109 error whose span begins at this line. Adjust the effective margin accordingly for
   all subsequent lines.

**Subsequent odd-indentation lines.** If a later line is also odd-indented:

- If the margin adjustment required is in the **opposite direction** to the current adjustment
  (i.e., the adjustment would restore the original even margin), it is not a new error. Instead, the
  span of the original E109 error is extended to cover all lines up to the point where the original
  margin is restored.
- If the margin adjustment is in the **same direction** as the current adjustment (i.e., the margin
  shifts further away from the original), a new E109 error is recorded at that point.

**E113 over-indentation.** When a line is indented more than one level deeper than the previous
compound line, the over-indented line is recorded as an **E113** error and omitted from the
presentation model. Parsing continues with the next line at the originally expected indent. This
keeps the parser deterministic without requiring schema-aware indent inference.

**E112.** §14 also defines **E112** for the case of a dedent that cannot find a matching ancestor
indent. The recursive parsing algorithm used here cannot produce that case (every dedent
encountered during the recursive descent terminates a `parse_blocks` invocation at the matching
ancestor level). E112 is therefore retained in the error catalogue as a reserved code for future
implementations that track an explicit ancestor stack.

## 20. Schema Language

A schema is expressed using the following types:

```typescript
interface Schema {
  name: string;
  document: Struct;
  layers: Layer[];
  sigil: Sigil | null;
  types: Definition[];
}

interface Layer {
  name: string;
  root: Struct;
  types: Definition[];
}

interface Definition {
  name: string;
  members: Member[];
}

type Type = Struct | Scalar | Flag | Reference;

interface Struct {
  members: Member[];
}

interface Scalar {
  validator: string;
  default: string | null;
}

interface Flag {}

interface Reference {
  name: string;
}

type Member = Field | Select;

interface Field {
  required: boolean;
  repeatable: boolean;
  keyword: string;
  type: Type;
}

interface Select {
  required: boolean;
  repeatable: boolean;
  variants: Variant[];
}

interface Variant {
  keyword: string;
  type: Type;
}
```

`Schema.name` is a kebab-case identifier (§20.7) for the schema. It is a human-readable label used
to identify the schema in source form; it is **not** the same as the schema identifier carried in
a document's pragma (§8.1), which is either a URL or a SHA-256 content hash of the schema's BinTEL.

`Schema.document` is the root `Struct` that defines the type of the document root compound. It
is `Struct`-typed directly (not `Type`-typed) by analogy with `Layer.root`: every schema must
define a root struct, and no other `Type` variant is meaningful at the document root.

`Schema.layers` is an ordered list of `Layer` values defining optional schema extensions. The empty
list is the normal case for a schema with no layers. Layer composition is defined in the Schema
Layering subsection.

`Schema.sigil` is the default sigil for documents that use this schema, or `null` if the schema does
not declare one. When non-null, it MUST satisfy the same character constraints as a pragma sigil
(§8): it MUST NOT be a space, `LF`, `CR`, a letter, a control character, a digit, or a
parenthetical symbol (§5) (**E210**). When a document's pragma omits a sigil but provides a schema
identifier that resolves to a schema with a non-null `sigil`, the schema's sigil is used as if it
had been specified in the pragma (§8.3).

`Schema.types` is an ordered list of `Definition` declarations. Each `Definition` binds a kebab-case
identifier to a `Struct`, allowing that struct to be referenced by name from elsewhere in the
schema via a `Reference`. This mechanism is what makes recursive schemas finitely expressible: the
schema-of-schemas itself (see the tel-schema subsection below) is necessarily recursive, and so is
any schema whose data has cyclical structure. The empty list is the normal case for non-recursive
schemas.

A `Layer`'s `name` is a kebab-case identifier (§20.7) labelling the layer. It MUST be unique
across all layers of a schema (**E207**). `Layer.root` is a `Struct` whose members are merged into
the composed schema's root struct by the algorithm in §20.3. `Layer.types` is an ordered list of
`Definition`s introduced by the layer; these merge with the base schema's `Schema.types` and any
preceding layers' `types` to form a single namespace of definitions visible to all references in
the composed schema. The empty list is the normal case for layers that only extend the root struct.

A `Definition` has a `name` and a list of `members`. The `name` MUST be a kebab-case identifier
(§20.7), unique across all `Definition`s in the composed schema — that is, the concatenation of
`Schema.types` with each `Layer.types` in layer order (duplicates anywhere in this concatenation
are **E213**). The `members` field is a list of `Member`s, structurally identical to those of a
`Struct`: a `Definition` is, in effect, a named `Struct` — it exists solely to give a reusable name
to a struct definition so that recursive schemas may be expressed in finite form. Non-`Struct`
types (`Scalar`, `Flag`) cannot be aliased through `Definition`; they should be inlined at their
use site.

A `Reference` is a `Type` whose semantic content is delegated to the `Definition` it points at by
name. During type assignment (§20.2) a value position whose schema type is `Reference(N)` is
treated as if its type were the `Struct` formed from the `members` of the `Definition` in
`Schema.types` whose `name` equals `N`. A `Reference` whose name does not match any
`Definition.name` in the schema (after layer composition) is invalid (**E212**).

TEL schemas are themselves representable as TEL documents. The TEL schema that describes the TEL
schema language is therefore self-describing; the schema for schemas has `Schema.name = tel-schema`. The serialization of a schema as a TEL document is governed by that schema. Because
schemas are TEL documents, they have a deterministic BinTEL encoding (see the
[BinTEL Specification](bintel-spec.md)), which is used for schema hashing and identification (§8.1).
The concrete TEL representation of the type model defined above — the keyword vocabulary, member
ordering, and validators used to write a schema as a TEL document — is given in §20.6 and embodied
in the file [`tel-schema.tel`](tel-schema.tel).

A `Struct` has an ordered list of `Member`s. Each member describes one logical child slot of the
struct and is either a `Field` or a `Select`. Both carry the common properties `required` and
`repeatable`:

- `required`: if `true`, the slot MUST be present at least once in a conforming document
- `repeatable`: if `true`, the slot MAY appear more than once; if `false`, it MUST appear at most
  once

A `Field` member has a single `keyword` and a single `type`. It represents a child whose keyword
and type are fixed.

A `Select` member has a non-empty list of `Variant`s. Any variant's keyword may be used to fill that
slot; the chosen keyword determines the type of the child node placed in that slot.

`Variant.keyword` is the keyword by which a child compound of that variant is written in TEL when
explicit. `Variant.type` may be any `Type`. A `Select` value looks and behaves exactly like one of its
variants: if the chosen variant has `Struct` type, the compound child has that struct's members as
children; if the variant has `Scalar` type, the compound child carries a value; if the variant
has `Flag` type, the compound child is a bare keyword with no content.

A `Select` member is **atom-assignable** if and only if all of its variants have `Flag` type. A `Select`
with any non-`Flag` variant may only be filled by compound children with explicit keywords.

A `Scalar` type represents a leaf value constrained by a validator. `Scalar.validator` names
the helper method to be invoked to validate the atom text (as described in the Validation section).
`Scalar.default` is either `null` (no default) or a string giving the value to be used when the
member is absent. A non-null default MAY only be specified if the `Scalar` appears in a
`required: true` member; specifying a non-null default on a non-required member is a schema error
(**E206**). When a required `Field` member whose type is a `Scalar` with a non-null default is
absent from the document, the default value is used as the semantic value and no E307 error is
raised.

**Serialising `Scalar.default`.** When a schema is written as a TEL document, the `default` field
of a `Scalar` carries an arbitrary string and is serialised using the atom-form escalation rules
of §22.2: an inline atom is used when the value contains no `LF` and no hard spaces in soft-space
mode; a source atom is used for multi-line values that have no trailing spaces and require no
byte-exact preservation; and a literal atom is used otherwise. This mirrors how any `Scalar` value
is serialised at a use site, so a schema author writes a default exactly as they would write any
other scalar value.

A `Flag` type carries no value of its own. Its identity is entirely determined by its keyword
(`Field.keyword` or `Variant.keyword`): in compound position, a `Flag`-typed node is written as
the keyword alone, with no inline atoms; in atom position, the atom text is matched against the
keyword. A `Flag`-typed member SHOULD NOT be `required`, since a required `Flag` member would be
unconditional boilerplate.

**Member ordering recommendation.** The order of members in a `Struct` determines which children can
be serialized as inline atoms (see the `construct` operation in the Reserialization and Editing
section). To maximize the use of inline atoms, schema authors SHOULD order members as follows:

1. Required `Field` members with `Scalar` type — especially any "identifying" field such as a
   keyword or name, since placing it first lets the whole field be declared inline with the
   identifier as the first atom (e.g. `field some-keyword required repeatable`).
2. Non-required `Field` members with `Scalar` type, prioritizing those most likely to be
   specified rather than absent.
3. Non-required `Field` members with `Flag` type. Each such member can be set inline by writing
   its keyword as an atom; if absent from the atoms, the skip rule in §20.2 advances past it.
4. Either an all-`Flag` `Select` member or a single `repeatable` `Field` member with `Scalar`
   type — but not both, since only one of these can appear in the trailing atom position.
5. All remaining members (`Struct`-typed fields, mixed-variant `Select` members, and any further
   members), which will always be serialized as compound children.

This ordering is a recommendation, not a requirement. Any member order is valid.

**Member order.** The **member order** of a `Struct` is the sequence of its members in their
declaration order within `Struct.members`. Where a specification rule refers to members "in member
order", it means iterating `members[0]`, `members[1]`, …, `members[n−1]`.

**Keyword order.** The **keyword order** of a `Struct` is a flat sequence of (keyword, type) pairs
obtained by expanding each member in member order: a `Field` member contributes a single entry
(its keyword and type); a `Select` member contributes one entry per variant, in the declaration order
of `Select.variants`. Keywords are numbered from 0 in this sequence; the position of a keyword in
keyword order is its **keyword index**.

**Identifier naming convention.** Programmatic identifiers defined by this specification — including
helper method names in `Scalar.validator` and the edit operation identifiers defined in the
Reserialization and Editing section — use **kebab-case**: a sequence of lowercase ASCII words
separated by hyphens (e.g. `update-value`, `switch-variant`). Schemas SHOULD use kebab-case for
validator names.

Every kebab-case identifier corresponds to a unique sequence of lowercase words. Implementations
SHOULD represent these identifiers idiomatically in their host language by applying the equivalent
convention:

- **kebab-case** (`update-value`) — the canonical form used in schemas and in this specification
- **snake_case** (`update_value`) — e.g. Rust, Python
- **camelCase** (`updateValue`) — e.g. Java, TypeScript, JavaScript
- **PascalCase** (`UpdateValue`) — e.g. C#, Go exported names

The mapping between these conventions is an isomorphism over sequences of lowercase words:
implementors SHOULD expect identifiers to appear in the idiomatic style of the host language and
MUST map them back to kebab-case when comparing against schema-defined names.

### 20.1 Schema Validity Constraints

A schema is invalid if any of the following holds:

- within a single `Struct`, the same keyword appears more than once across all members (considering
  `Field.keyword` and every `Variant.keyword` within each `Select`) (**E202**)
- a `Select` member has an empty `variants` list (**E203**)
- a `Scalar` has a non-null `default` and appears in a `Member` with `required: false`
  (**E206**). Absence of a non-required member always means the member is absent; defaults are only
  meaningful for required members that may be elided in the source document.
- `Schema.sigil` is non-null and is a space, `LF`, `CR`, a letter, a control character, a digit, or
  a parenthetical symbol (§5) (**E210**)
- the keyword `tel` appears as a `Field.keyword` or `Variant.keyword` in any `Struct` (**E211**)
- a `Reference` names an identifier that does not appear as a `Definition.name` in `Schema.types`
  (after layer composition) (**E212**)
- two or more `Definition`s in `Schema.types` share the same `name` (**E213**)

#### Schema Errors (E2xx)

| Code | Description                                                                                                               | Span                                           |
| ---- | ------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| E202 | Duplicate keyword within a `Struct` (across `Field` keywords and `Select` variant keywords)                                | The second occurrence of the duplicate keyword |
| E203 | `Select` member has an empty `variants` list                                                                                 | The `Select` member definition                    |
| E205 | Root struct has a `required` atom-assignable member (unreachable: the document root has no atoms)                         | The `required` member definition               |
| E206 | `Scalar` has a non-null `default` but appears in a non-`required` member                                               | The `default` field of the `Scalar`         |
| E207 | Two or more `Layer`s within a `Schema` share the same `name`                                                              | The second `Layer` with the duplicate `name`   |
| E208 | A `Layer` `Select` member has a variant keyword that overlaps with an existing keyword in the base `Struct`                  | The overlapping variant keyword in the layer   |
| E209 | A `Layer` `Field` member matches an existing keyword but the base or layer member is not a `Field` with `Struct` type | The layer member definition                    |
| E210 | `Schema.sigil` is non-null and is a space, `LF`, `CR`, a letter, a control character, a digit, or a parenthetical symbol  | The `sigil` field value                        |
| E211 | The keyword `tel` appears as a `Field.keyword` or `Variant.keyword` in any `Struct` (also §8)                           | The keyword definition containing `tel`        |
| E212 | A `Reference` names an identifier that does not resolve to a `Definition` in the schema                                    | The `Reference.name` value                     |
| E213 | Two or more `Definition`s in `Schema.types` share the same `name`                                                          | The second `Definition` with the duplicate name |

### 20.2 Type Assignment Algorithm

Type assignment translates the presentation model into the semantic model by ascribing a type to
every atom and compound node in the tree. It proceeds as a recursive descent over the tree, guided
by the schema.

**Reference resolution.** Wherever this algorithm asks "is T a Struct?", "is T a Scalar?", etc.,
the question is asked of the type T after **reference resolution**: if T is a `Reference(N)`, T is
replaced by the `Struct` formed from the `members` of the `Definition` in `Schema.types` whose
`name` is `N`. Because `Definition.members` is always a list of `Member`s (never another
`Reference`), resolution is a single step; no chasing of reference chains is required.

**Atom-assignable members.** A `Field` member M is _atom-assignable_ if M.type (after reference
resolution) is `Scalar` or `Flag`. A `Select` member is atom-assignable if and only if all of its
variants, after reference resolution, have `Flag` type. A member that is not atom-assignable may
only be satisfied by compound children (written with an explicit keyword), not by inline atoms.

**Document root.** The document root is a virtual compound node with type `Schema.document`. It has
no atoms; any `required` atom-assignable members of the root struct cannot be satisfied (**E205**).

**Type assignment for a compound node N with type T:**

1. T MUST be a `Struct`; if it is not, the document is invalid (**E301**).

2. Construct the keyword map K by iterating T in keyword order: for each entry (keyword, type) at
   member index i, map keyword → (i, type). (Schema validity ensures no duplicate keywords within
   the same struct.)

3. **Atom phase.** Let `pos` = 0. For each atom A in N.atoms, in order:

   a. Advance `pos` while the following skip condition holds: `pos` < len(T.members), the member M
   at `pos` is not `required`, and one of: (1) M is not atom-assignable, or (2) M is atom-assignable
   and is an all-`Flag` `Select` or a `Field` with `Flag` type, and the text of A does not match any
   keyword of M (i.e., M.keyword for a `Field`, or any variant's keyword for a `Select`). Each
   advanced-past member is recorded as absent.

   b. If `pos` ≥ len(T.members), the document is invalid (**E302**: more atoms than assignable
   member positions).

   c. Let M = T.members[pos]. M MUST be atom-assignable; if it is not, the document is invalid
   (**E303**: atom in non-atom-assignable member position).

   d. Assign A to M:
   - If M is a `Select`, the matched variant is the one whose keyword equals A's text; if no variant's
     keyword matches, the document is invalid (**E304**).
   - If M is a `Field` with `Flag` type, A's text MUST equal M.keyword; if it does not, the
     document is invalid (**E305**).
   - If M is a `Field` with `Scalar` type, the type of A is M.type regardless of A's text
     (validation against the named helper method is a separate step, described in the Validation
     section).

   e. If M is not `repeatable`, increment `pos`. If M is `repeatable`, leave `pos` unchanged; all
   subsequent atoms are also assigned to M.

4. **Compound child phase.** Let `current_member` = −1 and `seen_members` = {} (empty set). For each
   compound child C in N.children (iterating across all blocks in order):

   a. Look up C.keyword in K. If not found, the document is invalid (**E306**: unrecognized keyword
   for this parent type).

   b. Let (i, childType) = K[C.keyword]. The type of C is childType.

   c. If i ≠ `current_member`: if i is in `seen_members`, the document is invalid (**E309**: member
   children not contiguous); otherwise add `current_member` to `seen_members` (if ≥ 0) and set
   `current_member` = i.

   d. Record that member at index i has been filled by C.

   e. Recursively apply type assignment to C with type childType.

5. **Constraint check.** For each member M in T.members:

   a. Let fill_count = (number of atoms assigned to M) + (number of compound children assigned to
   M).

   b. If M.`required` and fill_count = 0: if M is a `Field` whose type is a `Scalar` with a
   non-null `default`, the default value is used as the semantic value and no error is raised;
   otherwise, the document is invalid (**E307**: required member absent and no default available).

   c. If not M.`repeatable` and fill_count > 1, the document is invalid (**E308**: non-repeatable
   member filled more than once).

### 20.3 Schema Layering

A `Schema` may include one or more `Layer` values in its `layers` list. Each layer describes an
incremental extension to the schema. A layer may add new members to a struct, and may introduce
new `Definition`s into the composed schema's type namespace. Layers are append-only: they may not
delete existing members, alter the `required` or `repeatable` properties of existing members, or
redefine an existing `Definition`.

**Composed schema identity.** A composed schema is identified by the base schema's `name` together
with the ordered sequence of layer `name`s applied to it. Two schemas with the same base `name`
but different layer sequences are distinct schemas.

**Composed type namespace.** The composed schema's type namespace is the concatenation, in order,
of the base schema's `Schema.types` followed by each `Layer.types` in layer order. Definition names
MUST be unique across this entire concatenation (**E213**). After composition, `Reference`s in the
base schema, in any layer, or in newly merged members may resolve to any `Definition` in the
composed namespace.

**Merge algorithm.** The function `Merge(base: Struct, layer: Struct): Struct` produces a new struct
that incorporates the layer's members into the base:

1. Begin with a copy of `base.members` in member order.

2. Construct the keyword map K for the base struct by iterating it in keyword order: for each entry
   (keyword, type) at member index i, map keyword → (i, members[i]).

3. For each member L in `layer.members` in member order:

   a. **Field members.** If L is a `Field` (keyword W, type T):
   - Look up W in K.
   - **Found:** Let (i, M) = K[W]. M MUST be a `Field` and both M.type and T MUST be `Struct`; if
     either is not, the layer is invalid (**E209**). Replace M.type in the merged member list at
     index i with `Merge(M.type, T)`.
   - **Not found:** Append L as a new member at the end of the member list. Add W → (new index, L)
     to K.

   b. **Select members.** If L is a `Select`:
   - Every variant keyword in L.variants MUST be absent from K; if any variant's keyword already
     exists in K, the layer is invalid (**E208**).
   - Append L as a new member at the end of the member list. For each variant V in L.variants, add
     V.keyword → (new index, L) to K.

4. Return the resulting member list as the merged struct.

**Layer validity constraints.** A schema is invalid if any of the following holds:

- Two or more layers within the same schema share the same `name` (**E207**)
- A layer `Select` member has any variant keyword that already appears in the keyword map of the
  (progressively merged) base struct (**E208**)
- A layer `Field` member matches an existing keyword, but the base member is not a `Field`, or
  the base type, the layer type, or both are not `Struct` (**E209**)
- A `Definition.name` in a `Layer.types` collides with any `Definition.name` already present in
  `Schema.types` or in a preceding layer's `types` (**E213**)

**Composing layers.** To apply a sequence of layers `[L₁, L₂, …, Lₙ]` to a base schema with root
struct R and types T, apply `Merge` to the root struct iteratively (R₀ = R, Rₖ = `Merge(Rₖ₋₁,
Lₖ.root)`), and concatenate types in order (T_composed = T ++ L₁.types ++ … ++ Lₙ.types). The final
Rₙ is the root struct of the composed schema, and T_composed is its type namespace.

### 20.4 BinTEL

The binary encoding of the semantic model, BinTEL, is defined in the companion
[BinTEL Specification](bintel-spec.md). BinTEL provides deterministic serialization of typed TEL
documents and defines the schema signature and value hash constructions used for schema
identification (§8.1) and compatibility checking (§8.2).

When a BinTEL document is to be carried in a text-oriented context, it MAY be encoded as Unicode
text using [BASE-256](base256-spec.md), defined as a companion specification. The BASE-256 textual
form is character-for-byte with the BinTEL byte sequence and is recovered losslessly by the BASE-256
decoder.

### 20.5 The tel-schema Schema

The concrete TEL representation of the schema model defined in §20 is itself a schema, identified
by `Schema.name = tel-schema`. The full document is supplied as the file
[`tel-schema.tel`](tel-schema.tel) at the root of this repository; this subsection specifies the
keyword vocabulary used by that document and states the self-describing closure property.

**Vocabulary.** The following kebab-case keywords are used in a schema TEL document. Each maps
one-to-one to a field or interface in the §20 type model:

| TEL keyword     | §20 construct                                  |
| --------------- | ---------------------------------------------- |
| `name`          | `Schema.name`, `Layer.name`, `Definition.name`, `Reference.name` (the parent compound's context determines which) |
| `document`      | `Schema.document`                              |
| `layer`         | `Schema.layers[i]`                             |
| `sigil`         | `Schema.sigil`                                 |
| `define`        | `Schema.types[i]` (at schema root) or `Layer.types[i]` (inside a `layer` compound) |
| `root`          | `Layer.root`                                   |
| `field`        | `Member` variant: `Field` (also fills `Definition.members[i]`, `Struct.members[i]`, and `Layer.root`/`Schema.document` members) |
| `select`        | `Member` variant: `Select` (same positions as `field`) |
| `required`      | `Field.required`, `Select.required` (Flag)   |
| `repeatable`    | `Field.repeatable`, `Select.repeatable` (Flag) |
| `keyword`       | `Field.keyword`, `Variant.keyword`           |
| `variant`       | `Select.variants[i]`                          |
| `struct`        | `Type` variant: `Struct`                      |
| `scalar`        | `Type` variant: `Scalar`                   |
| `flag`          | `Type` variant: `Flag`                        |
| `type`          | Names a `define`d struct via `Reference`. Used as one of the four variants in the type position of a `Field` or `Variant`. |
| `validator`     | `Scalar.validator`                          |
| `default`       | `Scalar.default`                            |

**Reserved keywords.** Only `tel` is universally reserved across all TEL documents (**E211**, §8).
The other keywords listed above are part of the tel-schema vocabulary and have meaning only when a
TEL document is being parsed as a *schema document* — i.e. when its schema is the tel-schema. They
do not constrain user-defined schemas: a user schema may freely define `Field.keyword` or
`Variant.keyword` values such as `name`, `document`, `layer`, `define`, `struct`, etc., because the
validity check applied to a user document is against the user schema's keyword set, not against
the schema-language vocabulary.

**Member ordering.** Within each Struct that the tel-schema describes, members are ordered per the
recommendation in §20: required Scalars first, optional Scalars next, then any all-Flag Select
or single repeatable Scalar member, and finally the structurally typed members. The `keyword`
Scalar in particular always comes first within `Field`-body and `Variant`-body so that a field
or variant may be declared with the keyword as the first inline atom, immediately followed by any
Flag atoms. For example, a `field` compound may be written:

```tel
field foo required repeatable
  scalar identifier
```

— with `foo` assigned to the `keyword` Scalar, `required` and `repeatable` matched against the
`required`/`repeatable` Flag members, and the type Select variant (`scalar`, carrying its own
inline `validator` atom) following as a child compound. No ambiguity arises because the keyword
position is fixed by member order; placing flag atoms before the keyword is invalid (E303).

**References.** Where the type of a `Field` or `Variant` is itself a `Reference`, the `type`
variant of the type Select is written directly as a child compound, with the referenced name as a
single inline atom:

```tel
field foo required
  type address
```

— where `address` is a `define` defined elsewhere in the same schema (or its base/layers). The
reference is resolved during type assignment per §20.2. References may form cycles via Structs in
their resolved bodies (the natural case for recursive data).

**Self-describing closure.** [`tel-schema.tel`](tel-schema.tel) MUST be a valid TEL document when
parsed under the schema it itself defines. Implementations MUST produce a byte-identical BinTEL
encoding of `tel-schema.tel`, and a single SHA-256 value hash (§3 of the BinTEL Specification) as
the schema identifier (§8.1 of this Specification). This hash, once stable, is normative — two
conforming implementations MUST agree on it.

**Bootstrap requirement.** A schema document cannot itself be parsed without a schema, and the
schema for schemas is `tel-schema.tel`. To break this regress, **every conforming TEL parser MUST
embed the `tel-schema` schema as a built-in**, available before any external schema has been
resolved. When a TEL document's pragma identifies `tel-schema` as its schema, the parser uses the
built-in form rather than performing schema retrieval. The built-in MUST produce the same
`Schema` model that would result from parsing the canonical `tel-schema.tel` under itself.

### 20.6 Schema Construction from the Semantic Model

Type assignment (§20.2) produces a tree of `Element` values typed by the tel-schema schema. To
obtain a `Schema` interface instance, an implementation traverses this tree and populates the
fields of the `Schema` model:

1. **Schema root.** Create a `Schema` value. Iterate the root element's children **in source order**
   (the order in which they appear in the document):
   - For the `name` child (a `Value` whose type is `Scalar` with `validator = "identifier"`),
     set `Schema.name` to its `text`.
   - For the `sigil` child, set `Schema.sigil` to its `text` (or leave `null` if absent).
   - For each `define` child, append a `Definition` to `Schema.types` constructed per step 2. The
     resulting list preserves source order: `Schema.types[0]` is the first `define` encountered,
     and so on.
   - For the `document` child, set `Schema.document` to the `Struct` constructed per step 3
     (using the `document` element's children as the Struct's members).
   - For each `layer` child, append a `Layer` to `Schema.layers` constructed per step 4. The
     resulting list preserves source order: `Schema.layers[0]` is the first `layer` encountered.
     This ordering is significant — layer composition (§20.3) applies layers in `Schema.layers`
     order, so two documents that differ only in layer ordering produce distinct composed schemas.
2. **`Definition` construction.** From a `define` element: take the `name` child as `Definition.name`,
   and construct `Definition.members` **in source order** by mapping each `field`/`select` child
   to a `Member` per step 3.
3. **Member construction.** A `field` element becomes a `Field`; a `select` element becomes a
   `Select`. Within a `Field`:
   - `keyword` child → `Field.keyword`.
   - `required` Flag child → `Field.required = true`.
   - `repeatable` Flag child → `Field.repeatable = true`.
   - One of `struct`, `scalar`, `flag`, `type` children → `Field.type`, constructed per step 5.

   Within a `Select`:
   - `required` Flag child → `Select.required = true`.
   - `repeatable` Flag child → `Select.repeatable = true`.
   - Each `variant` child → an entry in `Select.variants`, in source order. A `Variant` has a
     `keyword` child for `Variant.keyword`, and a `struct`/`scalar`/`flag`/`type` child for
     `Variant.type`.
4. **`Layer` construction.** From a `layer` element: take the `name` child as `Layer.name`;
   construct `Layer.root` from the `root` element's children (as a `Struct` per step 3); construct
   `Layer.types` from each `define` child within the layer, **in source order**, so that
   `Layer.types[0]` is the first definition declared inside the layer.
5. **`Type` construction.** From the chosen type-Select variant:
   - `struct` element → `Struct` whose members are constructed from the element's `field`/`select`
     children per step 3.
   - `scalar` element → `Scalar` with `validator` from its `validator` child and `default` from its
     optional `default` child (or `null`).
   - `flag` element → `Flag` (no fields).
   - `type` element → `Reference` with `name` from its inline atom.

Schema construction MUST be deterministic: two implementations applied to the same input MUST
produce identical `Schema` values. After construction, the resulting schema is checked against the
validity constraints of §20.1 and §20.3; any failure is reported as the corresponding **E2xx**
error.

### 20.7 Kebab-Case Identifier

Throughout this specification, a **kebab-case identifier** has the grammar:

```
identifier ::= lower-letter ( '-'? lower-letter-or-digit )*
lower-letter ::= [a-z]
lower-letter-or-digit ::= [a-z] | [0-9]
```

That is:

- An identifier MUST begin with a lowercase ASCII letter.
- It MAY contain lowercase letters, digits, and hyphens.
- Hyphens MUST NOT appear consecutively (no `--`).
- Hyphens MUST NOT appear at the start or end of the identifier.
- The empty string is not a valid identifier.

This grammar is enforced by the built-in `identifier` validator (§21.5).

### 20.8 Footnote: Inline-Atom Position Constraints

The atom phase in §20.2 allows skipping past non-required `Flag`-shaped members when an atom does
not match their keyword. It does **not** allow skipping past non-required `Scalar` members,
because a `Scalar` accepts any atom text — there is no "doesn't match" condition that would justify
a skip. As a consequence, when a struct's atom-assignable members include two or more non-required
`Scalar` `Field`s, only their prefix can be filled positionally with inline atoms; later optional
Scalars in the same struct must be filled via explicit compound children. Schema authors should
either order their members so this limitation does not arise, or expect users to write the later
optional Scalars as child compounds (e.g. `field-name value`).

## 21. Validation

Type assignment (§20.2) ascribes a `Type` to every node. For `Struct` and `Flag` types, the
structure of the document is sufficient to determine validity. For `Scalar` types, the atom text
must additionally be checked against a named **validator**.

### 21.1 Validators

A validator is identified by the string in `Scalar.validator`. This string names an external
**helper method** that checks whether a given string value conforms to the scalar's constraints.

Validation of scalar values is, in general, too complex to express within a schema: it may
require external data, complex algorithms, or application-specific logic. TEL therefore delegates
scalar validation to helper methods provided outside the schema.

This specification defines no validators. The set of valid validator names is determined entirely
by the application: a parser is configured with a callback (§21.4) that maps each
`Scalar.validator` string to a concrete check. Two applications using different validator
libraries can both produce valid TEL parsers; they simply accept different documents.

As an illustration, consider an application that defines a validator named `ipv4` accepting
dotted-quad IPv4 address literals such as `192.0.2.1`. A schema that wishes to constrain a field to
IPv4 addresses would set `Scalar.validator = "ipv4"`. The parser is supplied with a callback
that, given the `ValidationRequest { method: "ipv4", value: "192.0.2.1" }`, returns `Valid`, and
given `{ method: "ipv4", value: "999.0.0.1" }`, returns `Invalid` with a diagnostic spanning the
offending octet. The string `ipv4` is not normative — another application could use `ip4`, `inet4`,
or any other kebab-case name with the same effect.

### 21.2 Request and Response

The request and response of a helper method invocation are defined by the following types:

```typescript
interface ValidationRequest {
  method: string;
  value: string;
}

type ValidationResponse = Valid | Invalid;

interface Valid {}

interface Invalid {
  diagnostics: Diagnostic[];
}

interface Diagnostic {
  message: string;
  start: number;
  end: number;
}
```

`ValidationRequest.method` is the value of `Scalar.validator`. `ValidationRequest.value` is the
verbatim text of the scalar atom (whether `Inline`, `Source`, or `Literal` in the presentation
model — the atom form is not semantically significant).

A helper method MUST return either `Valid` or `Invalid`. An `Invalid` response includes a non-empty
list of `Diagnostic` entries. Each entry has a human-readable `message` and a half-open span
`[start, end)` of zero-based code-point indices into the input string, identifying the portion of
the input to which the message applies.

In many cases a single diagnostic entry spanning the entire input string is sufficient. However, a
helper method MAY return multiple entries highlighting different errors at different positions
within the input. Spans MAY overlap.

When reporting an E310 error, the implementation MUST translate each helper method span from
input-string-relative offsets to document-level code-point offsets by adding the offset of the
atom's beginning within the document.

### 21.3 Integration

The `Scalar.validator` field in the schema specifies which helper method provides validation for
that scalar type. A conforming implementation MUST invoke the named helper method for every
`Scalar` atom in the document during validation, unless the implementation explicitly opts out of
scalar validation (for example, during a parse-only pass that does not require full semantic
checking).

If the helper method returns an invalid response, the document is invalid (**E310**). The
implementation SHOULD report each diagnostic entry to the user, associating the span with the
corresponding source location in the original document.

### 21.4 Helper Method Binding

A TEL parser that wishes to enable scalar validation MUST be provided with a **callback
function** that conforms to the helper method interface: given a `ValidationRequest`, it returns a
`ValidationResponse`. The parser invokes this callback for each `Scalar` atom encountered during
validation. How the callback is supplied is determined by the host language or environment (e.g. as
a function parameter, a trait implementation, or an interface injection).

This specification does not prescribe a wire protocol, service discovery mechanism, or serialization
format for helper method invocation. In particular:

- A parser embedded in an application MAY implement the callback directly in the host language.
- An IDE, text editor, or LSP server MAY delegate helper method calls to an external service (e.g.
  via REST, RPC, or a subprocess), but the mechanism by which the editor discovers and configures
  such a service is outside the scope of this specification. From the parser's perspective, the
  editor simply provides a callback that handles the delegation internally.

If no callback is provided, the parser MUST skip scalar validation entirely (no E310 errors are
raised). All other parsing and validation proceeds normally.

### 21.5 Built-in Validators

This specification does not mandate a portable validator library — applications choose which
validators they implement (§21.1). However, three validators are referenced by the `tel-schema`
schema itself, and therefore MUST be implemented by any TEL parser that wishes to parse schema
documents at all. The behaviour of these three validators is fixed by this specification.

**`identifier`.** Accepts a string that conforms to the kebab-case identifier grammar of §20.7.
On failure, returns a single `Diagnostic` whose span is `[0, len(value))` and whose `message`
describes the first violation encountered (e.g. "leading hyphen", "consecutive hyphens", "empty
identifier", "non-ASCII or uppercase character").

**`sigil`.** Accepts a single-character string whose character satisfies the constraints in §8 of
this specification: it MUST NOT be `U+0020` SPACE, `U+000A` LINE FEED, `U+000D` CARRIAGE RETURN, an
ASCII letter, an ASCII control character, an ASCII digit, or a parenthetical symbol (§5). On
failure, returns a `Diagnostic` covering the offending input.

**`string`.** Accepts any input string without further constraint; equivalent to "no validation."
The `string` validator MUST always return `Valid`. It exists so that a schema can declare a field
whose validator is the unconstrained string type without the application needing to define a custom
validator.

Implementations MAY provide additional validators beyond these three. The three above are the
minimum required for `tel-schema` parsing to function.

#### Validation Errors (E3xx)

| Code | Description                                                                                           | Span                                                                                                     |
| ---- | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| E301 | Compound's type is not a `Struct`                                                                     | The compound's keyword                                                                                   |
| E302 | More atoms on a compound than there are assignable member positions                                   | The first excess atom                                                                                    |
| E303 | Atom appears at a member position that is not atom-assignable                                         | The atom                                                                                                 |
| E304 | Atom text matches no variant keyword of a `Select` member                                                | The atom                                                                                                 |
| E305 | Atom text does not match a `Field` member's `Flag` keyword                                          | The atom                                                                                                 |
| E306 | Compound keyword is not recognized for its parent type                                                | The compound's keyword                                                                                   |
| E307 | Required member absent, and member is not a `Field` with a `Scalar` type with non-null `default` | Zero-width span at the end of the parent compound's last child (or at the parent keyword if no children) |
| E308 | Non-repeatable member is filled more than once                                                        | The keyword of the second occurrence                                                                     |
| E309 | Compound children of the same member are not contiguous                                               | The keyword of the non-contiguous child (the second group's first child)                                 |
| E310 | Scalar value failed validation by the named helper method                                          | As reported by the helper method's diagnostic spans, translated to document offsets                      |
| E311 | `Flag`-typed compound has atoms or compound children                                                  | The first atom or child of the `Flag` compound                                                           |

## 22. Reserialization and Editing

The presentation model can be mutated to reflect changes to the semantic model, preserving
formatting, comments, tabulations, and remarks wherever possible. Mutations are expressed as
operations on the semantic model, which are then reflected in the presentation layer.

There are two categories of editor:

- **Humans**, who modify source text directly with full flexibility and no constraints on what
  changes may be made
- **Machines** (agents or processors), which apply structured operations to the semantic model and
  reserialize through the presentation layer

### 22.1 Comment Attachment and Editing

Each `Block` in the presentation model carries zero or more attached comments (§11.1) that precede
its compounds. These comments travel with the block under programmatic transformations.

When a machine deletes a compound, the `Block` that contained it is updated. If the deleted compound
was the only compound in its block, and if the block has attached comments, those comments are also
removed (since their meaning was associated with that block).

When a machine moves a compound, its containing block's structure is preserved where possible: if
the move results in the block having no remaining compounds, the block (and any attached comments)
moves with the compound to the new location.

When a machine inserts a compound constructed from purely semantic information, no comment is
attached to it initially; it is placed into an existing block or a new empty block as appropriate.

### 22.2 Machine Operations

A machine MUST perform only operations drawn from the following set. Each operation preserves all
presentation-layer details that are not directly affected by the operation: remarks,
`trailingBlankLines` counts, `precedingSpaces` on inline atoms, and tabulation marker offsets are
all retained unless the operation explicitly targets them.

**Sigil invariant.** A machine MUST NOT change the document's sigil. The sigil in effect when the
document was parsed is preserved exactly in any reserialized output.

**Literal atom delimiter invariant.** The delimiter of a literal atom MUST NOT appear as a line
within the atom's payload. When a machine updates the value of a literal atom, it MUST check whether
the existing delimiter appears verbatim as a line in the new payload. If it does, the editor MUST
choose a new delimiter that does not appear as a line in the new payload before writing the updated
atom.

**`delete`** — Remove a compound that is not `required`. Any remark attached to the compound is
removed with it. If the compound's block becomes empty (no remaining compounds), the block and its
attached comments are also removed.

**`replace`** — Substitute a compound for another of the same member type at the same position in
the same block. The replacement retains the original compound's remark and its position within the
block. If the member is a `Select` and the replacement uses a different variant, the keyword in the
presentation layer is updated accordingly. Attached comments on the block are preserved.

**`construct`** — Create a new compound from purely semantic information, with no presentation-layer
context. The constructed compound carries no remark and has no attached comments. No blank lines
appear between its children. No tabulation is added. The canonical presentation form is determined
by iterating the struct's members in member order:

1. Starting from the first member, each non-repeatable `Field` member whose type is `Scalar` is
   serialized as an inline atom, in member order, for as long as consecutive members satisfy this
   condition and the value can be represented as an inline atom (see atom form escalation below).
2. If the next member after the initial run of non-repeatable scalars is an all-`Flag` `Select`,
   each present flag is serialized as an inline atom.
3. Otherwise, if the next member is a `repeatable` `Field` whose type is `Scalar`, each
   occurrence is serialized as an inline atom (if representable; see atom form escalation below).
4. All remaining children — including any `Field` members whose type is `Struct`, mixed-variant
   `Select` members, and any members beyond the first repeatable scalar — are serialized as compound
   children with explicit keywords.

**Atom form escalation.** When serializing a `Scalar` value, the atom form is selected as
follows:

1. **Inline atom**: used if the value contains no `LF` characters and can be represented on the
   parent line without violating the phrase-separation rules (§10.3) — that is, the value does not
   contain hard spaces when in soft-space mode.
2. **Source atom**: used if the value cannot be an inline atom but does not contain trailing spaces
   on any line and does not require exact byte-level preservation.
3. **Literal atom**: used if the value cannot be represented as a source atom — for example, if it
   contains trailing spaces on a line, or if the source-atom stripping rules would alter the
   content.

If a value requires source or literal atom form, it is serialized as a compound child with an
explicit keyword and the appropriate atom body, rather than as an inline atom.

Each inline atom uses a single preceding space (`precedingSpaces = 1`). Each compound child is
indented by one level (two spaces) relative to its parent.

**`insert`** — Insert a compound into the child structure of a parent at the natural position for
its member: after all existing compounds of the same member, within the same block if one exists for
that member group, or in a new block otherwise.

**`insert-before`** — Insert a compound immediately before a specified existing sibling compound.
The inserted compound is placed in the same block as the sibling if the block does not have a
tabulation, or in a new block immediately before the sibling's block if it does.

**`insert-after`** — Insert a compound immediately after a specified existing sibling compound,
subject to the same block-placement rules as `insert-before`.

**`insert-into-block`** — Append a compound to the `compounds` list of a specified existing block.
This is the natural way to add rows to a tabulated block. The block's tabulation must have
sufficient column capacity for the new compound; if not, `resize-tabulation` must be applied first.

**`attach-remark`** — Add a remark string to a compound. If the compound already has a remark, it is
replaced.

**`remove-remark`** — Remove the remark from a compound.

**`update-value`** — For a compound or atom whose schema type is `Scalar`, update the atom text
to a new string. The new string MUST be valid according to the named helper method (§21). All other
presentation details of the compound are retained.

**`set-flag`** — Add a `Flag`-typed node within a parent, provided the result satisfies the
`repeatable` constraint for that member. The flag is placed as an inline atom if both of the
following hold: (a) the flag's member precedes all compound children in member order (i.e., no
member that currently has compound children appears earlier), and (b) inserting the atom does not
require moving any existing compound children to atom position. If either condition is not met, the
flag is placed as a compound child using the `insert` placement rules.

**`unset-flag`** — Remove a `Flag`-typed node within a parent, provided the result satisfies the
`required` constraint for that member. If the flag is currently an inline atom, the atom is removed
and the `precedingSpaces` of subsequent atoms are preserved. If the flag is currently a compound
child, it is removed using the `delete` rules.

**`reorder-within-group`** — Change the position of a compound among its siblings within the same
member group (i.e., other compounds filling the same schema member). This operation never violates
E309. The reordered compound retains its remark; attached comments on the affected blocks are
preserved.

**`reorder-groups`** — Change the relative order of two distinct member groups within a parent's
child structure, by moving all blocks belonging to one group before or after all blocks belonging to
another. This is valid as long as neither group is interleaved with the other (E309 is satisfied
before and after). Attached comments on all affected blocks are preserved.

**`resize-tabulation`** — Adjust the `markerOffsets` of a block's `Tabulation` to accommodate all
current column values and any values about to be added. New offsets MUST be chosen such that every
existing and planned column value fits within the column widths defined by §11.2, and MUST be a
minimal adjustment (columns are not widened beyond what is required). After resizing, all existing
row content MUST be re-padded with spaces to align to the new column positions. The `headings` list
is updated in parallel with `markerOffsets`: existing headings are preserved in place and re-padded
within their updated column spans; no heading text is added or removed by this operation.

### 22.3 Canonical Document Serialization

A **canonical serialization** of a semantic model produces a single, deterministic TEL text
representation. Canonical serialization follows the same conventions as the `construct` operation
(§22.2) for individual compounds, extended to the entire document:

- The document margin is zero.
- No interpreter directive is included.
- A pragma line is included, specifying the TEL version of the serializer and the schema identifier.
  The sigil is not specified in the pragma (the default `#` is used).
- No comments or remarks are included anywhere in the document.
- No tabulation lines are included; all compounds are serialized as ordinary (non-tabulated) lines.
- No blank lines appear between children at any level.
- The root node has no inline atoms (the document root is a virtual struct with no atom positions),
  so every root-level member is serialized as a compound child.
- At every non-root level, the atom form escalation rules from the `construct` operation apply:
  inline atoms are preferred, falling back to source atoms for values containing `LF` characters,
  and to literal atoms as a last resort for values that source atom form cannot faithfully
  represent.
- Each inline atom uses a single preceding space (`precedingSpaces = 1`).
- Each compound child is indented by one level (two spaces) relative to its parent.
- Literal atoms use the delimiter `---` unless the payload contains that string as a line, in which
  case a unique delimiter is chosen.
- Line endings use LF mode.

Two documents with identical semantic models, serialized canonically by the same version of the
specification, MUST produce identical text output.

## 23. Invalidity Conditions

A TEL document is invalid if any condition identified by a **E1xx** or **E3xx** error code in this
specification is triggered. A schema is invalid if any **E2xx** condition is triggered. The complete
taxonomy of all error conditions, their trigger sections, and their recovery strategies are given in
§19.3 and §19.5 respectively.

## 24. Deferred Topics

The following topics remain underspecified or unresolved:

- **Complete error taxonomy.** Error codes E101–E125 (parsing), E201–E211 (schema), and E301–E311
  (validation) are believed to be complete. A malformed schema _document_ (as opposed to a malformed
  schema _definition_) is not a separate error category: since schemas are TEL documents typed by
  the `tel-schema` schema, errors in a schema document are ordinary parsing and validation errors
  reported against that schema. No additional error codes are believed to be needed, but this has
  not been exhaustively verified.

- **Mutation semantics.** The machine operations (§22.2) are defined individually. What is missing:
  the semantics of composing multiple operations (ordering, atomicity, conflict resolution), and the
  precise rules for how `replace` determines what constitutes a valid replacement.

- **Examples and reference algorithm.** No worked examples or reference parsing algorithm are
  provided. What is missing: example TEL documents with their presentation models, semantic models,
  and [BinTEL](bintel-spec.md) encodings; a reference implementation or pseudocode for the parsing
  algorithm; and example schema definitions with layering.
