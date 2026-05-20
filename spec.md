# TEL Specification Draft

## Abstract

TEL is a line-oriented, tree-structured, typed data language designed for data that is read, written
and maintained by both humans and machines.

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

When serialized as binary data, a TEL document MUST be encoded as UTF-8.

TEL uses `U+000A` LINE FEED (`LF`) as its primary line-ending character. Carriage return (`CR`,
`U+000D`) is also permitted under the following rules.

A `CR` appearing anywhere in a TEL document outside a literal atom payload MUST be immediately
followed by `LF` (**E123**).

The **line-ending mode** of a document is determined by the first `LF` character in the document:

- if that `LF` is immediately preceded by `CR`, the mode is **CRLF mode**
- otherwise, the mode is **LF mode**

Once the mode is established, every subsequent `LF` outside a literal atom payload MUST conform to
it: in CRLF mode every `LF` MUST be preceded by `CR`; in LF mode `CR` MUST NOT appear before any
`LF`. A violation of this rule is also an **E123** error.

No Unicode normalization is required or implied. TEL is defined over the exact Unicode code points
that appear in the serialized text.

A UTF-8 byte order mark MUST NOT appear in a TEL document (**E101**).

Visually misleading code points, such as zero-width characters, SHOULD be avoided. Control-heavy
content SHOULD be avoided except where required. TEL is not intended primarily as a binary-data
format, even though it can represent content containing non-printing code points.

## 5. Significant Characters and Terms

The following characters have syntactic significance in TEL:

- `U+000A` LINE FEED (`LF`)
- `U+0020` SPACE
- one other symbolic character designated as the **sigil** (§8.3)

A **line** is a contiguous, potentially empty sequence of non-linefeed characters delimited by
linefeed characters or by the start or end of the file. In CRLF mode (§4), the `CR` immediately
preceding each delimiting `LF` is part of the line terminator and is not part of the line's content.

A **soft space** is exactly one `U+0020` SPACE character.

A **hard space** is two or more consecutive `U+0020` SPACE characters.

A **blank line** is a line containing only `U+0020` SPACE characters, or no characters at all.

Blank lines have no defined indent.

A **parenthetical symbol** is one of the six bracket characters: `(`, `)`, `[`, `]`, `<`, `>`, `{`,
`}`.

A **word** is a maximal contiguous sequence of non-linefeed, non-separator characters on a line,
where separators are determined by the word-separation rules (§10.3).

The **first content character** of a non-blank line is the first character after the margin and any
indentation spaces — equivalently, the first non-space character on the line.

An **ordinary line** is any non-blank line that is not a comment line (§11.1), a tabulation line
(§11.2), or a line forming part of a source atom (§15) or literal atom (§16) payload.

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

type Sigil = "!" | '"' | "#" | "$" | "%" | "&" | "'" | "*" | "+" | "," | "-" | "." | "/" | ":" | ";" | "=" | "?" | "@" | "\\" | "^" | "_" | "`" | "|" | "~";
```

## 7. Interpreter Directive

If the first two characters of the document are `#!` (`NUMBER SIGN`, `EXCLAMATION MARK`), then the first physical line of the document is an interpreter directive line.
If not, the interpreter directive is absent.

The interpreter directive payload is the content of the first line after the leading `#!`, up to but
excluding the line terminator.

If a document has an interpreter directive and also has a pragma, then the pragma MUST appear after
the interpreter directive.

An interpreter directive line is not part of the `children` sequence.

## 8. Pragma

If present, the pragma MUST be the first non-blank line after any interpreter directive line, and is
parsed using the ordinary TEL line rules (**E102**).

If present, the entire pragma line MUST be fully contained within the first 4096 bytes of the
document (**E103**).

The keyword of the pragma line MUST be `pragma`. The keyword `pragma` is reserved: it MUST NOT
appear as a `Product.keyword` or `Variant.keyword` in any `Struct` within a schema (**E211**).

The pragma line MUST contain at most three atoms after the keyword (version, schema identifier, and
sigil). Any additional atoms are invalid (**E125**). The remark rule (§11.3) does not apply to the
pragma line; any content that would otherwise be parsed as a remark is invalid (**E125**).

The positional form of the pragma is:

```text
pragma 1.0 schema-id #
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
contains no space characters, a schema identifier always occupies a single word.

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

The sigil declared by a schema is given by `Schema.sigil` (§20).

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
non-blank line necessarily has indent `0`.

Trailing spaces on a non-blank ordinary line are not permitted (**E110**).

Blank lines have no structural effect, except where explicitly stated for tabulated blocks, source
atoms, or literal atoms.

## 10. Keywords and Inline Atoms

Each non-blank ordinary line is parsed into one or more words by the word-separation rule (§10.3).

The first word on a non-blank ordinary line is the **keyword**.

Each subsequent word on that line is an **inline atom**.

### 10.1 Keyword Characters

A keyword may contain any Unicode code point except `U+0020` SPACE and `U+000A` LINE FEED.
Non-printing code points are NOT RECOMMENDED in keywords but are not forbidden. Although non-ASCII
keywords are permitted, ASCII keywords are generally RECOMMENDED for interoperability and
readability.

### 10.2 Inline Atom Characters

An inline atom may contain any Unicode code point other than `LF`, subject to the TEL
word-separation rules and any special parsing rules for remarks.

### 10.3 Word-Separation Rule

After the keyword, the line is parsed left-to-right.

Initially, a single space is sufficient to terminate a word and begin the next word.

If a hard space is encountered anywhere after the keyword, then from that point onward only hard
spaces terminate words. Soft spaces after that point become part of the current word.

Accordingly:

- before the first hard space on a line, either a soft space or a hard space terminates a word
- after the first hard space on a line, only a hard space terminates a word
- after the first hard space on a line, a soft space becomes content within the current word
- each new line resets this rule

Consequently, after the first hard space on a line, a word may contain soft spaces but may not
contain hard spaces.

The remark rule (§11.3) is subject to the word-separation mode in effect at the point it is tested.
In soft-space mode, the sigil preceded by a soft space is a remark introducer. In hard-space mode,
the sigil preceded by a soft space is not at a word boundary and is therefore not a remark
introducer; it must be preceded by a hard space to be recognized as one.

## 11. Comments, Tabulations, and Remarks

TEL distinguishes between:

- a **comment**, which occupies an entire line and is represented as a line-level presentation node,
- a **tabulation**, which occupies an entire line and represents the start of a tabulated block, and
- a **remark**, which is attached to a compound line and is not an ordinary child node.

The document's **sigil** — the character that introduces comments, tabulations, and remarks — is
determined by the resolution rules in §8.3.

### 11.1 Comment

A line is a comment line if, after its leading indentation, its keyword is exactly equal to the
sigil, and the line does not qualify as a tabulation line (§11.2). A line qualifies as a tabulation
line if at least one further occurrence of the sigil appears on the line preceded by a hard space;
in that case the line is a tabulation line and not a comment line, regardless of any other content.

If the sigil is followed immediately by the end of line, the comment payload is the empty string.

If the sigil is followed by a soft space, the comment payload begins at the first character after
that soft space and continues unchanged to the end of the line.

A word such as `#foo` (the sigil concatenated with other characters) is not a comment keyword.

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

A tabulation line is represented as a distinct presentation node. The remark rule (§11.3) does not
apply to tabulation lines; any content on a tabulation line that would otherwise form a remark is
instead part of the heading text for the final column.

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

A remark begins when the sigil appears at the start of a word and is immediately followed by exactly
one soft space. Whether a given occurrence of the sigil is at the start of a word depends on the
word-separation mode in effect at that point (§10.3).

Accordingly:

- the sigil followed by end of line is an ordinary word, not a remark introducer
- the sigil followed by a hard space is an ordinary word, not a remark introducer
- the sigil not preceded by a word boundary in the current mode is ordinary content within the
  current word
- the sigil at a word boundary followed by a soft space introduces a remark

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
  text: string;
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

- zero or more **attached comments** (each preceded by a blank line, per §11.1)
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

A source atom is represented in the presentation model as `Atom.Source(text)` and is appended to the
end of the atom sequence of the immediately preceding compound.

A compound may have at most one source atom. Introducing a source atom when the preceding compound
already has a source or literal atom is invalid (**E115**).

The source atom begins on the double-indented line and includes that line together with each
subsequent line until either:

- the end of the document is reached, or
- a non-blank line is encountered whose indent is less than the indent of the first source-atom line

Blank lines are permitted within a source atom.

For each non-blank captured line, exactly the indentation of the first source-atom line is stripped
from the start of the line. Any surplus leading spaces are preserved.

For each captured line, trailing spaces are stripped. (Source-atom lines are not ordinary lines, so
E110 does not apply to them; trailing spaces are silently removed rather than being an error.)

A blank line within a source atom contributes exactly a `LF` character to the source-atom payload,
regardless of how many spaces it physically contains.

The source-atom payload is otherwise captured literally. In particular, the sigil has no special
meaning inside a source atom.

The source-atom payload always ends with exactly one final `LF` character.

Source-atom lines are subject to the normal line rules (§5): in CRLF mode, the `CR` preceding each
`LF` is part of the line terminator and is not part of the line content. Consequently, source-atom
payloads always use `LF` as the line separator, regardless of the document's line-ending mode.

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

The remainder of that opening line, from its first content character up to but excluding the line
terminator, is the delimiter.

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
word-separation rules as ordinary lines (§10.3). Column values are introduced by the column
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
to the schema member that corresponds to that column's position: if the member has a `Primitive`
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
indentation errors; see §19.5) that cannot be resolved from syntax alone.

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
(§20.2) during parsing. The result is a tree of `Element` values:

```typescript
type Element = Node | Value;

interface Node {
  keywordIndex: number;
  type: Type;
  children: Element[];
}

interface Value {
  keywordIndex: number;
  type: Primitive;
  text: string;
}
```

A `Node` represents a `Struct`-typed or `Flag`-typed element. `Node.type` is the `Type` assigned by
the type assignment algorithm (§20.2). `Node.children` is the ordered list of child elements; for a
`Flag`-typed node, `children` is always empty.

A `Value` represents a `Primitive`-typed element. It is a leaf: it carries the atom text in
`Value.text` and has no children.

Every element carries a `keywordIndex`, which is the position of the element's keyword in the
keyword order (§20) of the parent's `Struct` type. For the document root, `keywordIndex` is not
applicable. The `keywordIndex` identifies which member (and, for `Sum` members, which variant) the
element fills, and is sufficient to recover the keyword string from the schema.

The interpreter directive, pragma, comments, tabulations, and remarks are not part of the semantic
model. There is a one-to-one mapping between presentation-layer atoms and compounds on the one hand,
and elements on the other: every atom and every compound maps to exactly one element.

### 18.3 Mapping Procedure

The mapping from presentation model to semantic model proceeds as follows. The type assignment
algorithm (§20.2) ascribes a type to every atom and compound. Given these assignments, the semantic
tree is constructed by:

1. **Document root.** Create a root `Node` with `type` equal to `Schema.document` and `children`
   constructed from steps 2–5.

2. **Compound children.** For each compound child C of the current node (iterating across all blocks
   in order, skipping comments and tabulations):
   - If the type assigned to C is `Struct` or `Flag`, create a `Node` with that type and `children`
     constructed by recursing into C's children (empty for `Flag`).
   - If the type assigned to C is `Primitive`, create a `Value` with that type and `text` equal to
     the compound's inline atom text if present, or the empty string if the compound has no inline
     atoms.

3. **Atoms.** For each atom A assigned to the current node (in order):
   - If the assigned type is `Primitive`, create a `Value` with that type and `text` equal to A's
     text.
   - If the assigned type is `Flag`, create a `Node` with that type and an empty `children` list.

4. **Ordering.** Atom-derived nodes and compound-derived nodes for the same member are interleaved
   in the order they were assigned. Atom-derived nodes for a member precede compound-derived nodes
   for the same member (atoms appear on the parent line, before any child lines).

5. **Defaults.** For each required `Product` member with a `Primitive` type and a non-null `default`
   that was not filled by any atom or compound child: create a `Value` with that `Primitive` type
   and `text` equal to `Primitive.default`. This node is placed at the position in the child list
   corresponding to the member's position in member order.

The resulting tree is fully determined by the presentation model and the schema. No ambiguity
remains: the type, value, and child order of every element are fixed by the type assignment
algorithm.

The schema language and type assignment algorithm are defined in §20.

## 19. Schema-Governed Structure and Error Diagnosis

In addition to parsing errors, a TEL document may be structurally invalid with respect to a schema.

When a schema is available, it is applied during parsing rather than as a separate post-processing
stage. This is necessary because the schema informs certain error recovery decisions — in
particular, indentation recovery (§19.5) uses keyword validity at candidate indent levels to resolve
ambiguous lines. The result is a presentation model and semantic model constructed together in a
single pass.

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
`Primitive` member may only consume atoms if it is the last atom-assignable member in member order;
no further atoms may be assigned to subsequent members.

Similarly, once atoms are assigned to an all-`Flag` `Sum` member, no further atoms may be assigned
to subsequent members, because each atom is matched against the Sum's variant keywords and the atom
phase cannot advance past a `Sum` member except by skipping it entirely.

For a `repeatable` member, occurrences may be split across both of the following:

- inline atoms on the parent compound line, and
- subsequent compound children of the parent with the same keyword

These two assignment mechanisms may be combined freely. The E309 contiguity rule (§19.3) already
prohibits differently-typed compound children from being interleaved between such occurrences.
Remark lines do not affect this rule.

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

| Code | Section  | Description                                                                                            | Span                                                                                                        |
| ---- | -------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| E101 | §4       | BOM present at start of document                                                                       | The BOM bytes (`[0, 3)` for a UTF-8 BOM)                                                                    |
| E102 | §8       | Pragma is not the first non-blank line after any interpreter directive                                 | The `pragma` keyword on the misplaced line                                                                  |
| E103 | §8       | Pragma line extends beyond the first 4096 bytes                                                        | The entire pragma line                                                                                      |
| E104 | §8       | Pragma version parameter does not have the form `x.y` with non-negative integers                       | The version atom                                                                                            |
| E106 | §8       | Pragma sigil is a space, `LF`, `CR`, letter, digit, or parenthetical symbol                            | The sigil atom                                                                                              |
| E108 | §9       | Non-blank line begins with fewer than the margin number of spaces                                      | The leading spaces of the line (zero-width at line start if no spaces)                                      |
| E109 | §9       | Relative indentation after the margin is odd                                                           | The leading spaces of the line; extended through subsequent lines if margin adjustment persists (see §19.5) |
| E110 | §9, §17  | Trailing spaces on a non-blank ordinary line or tabulated row                                          | The trailing space characters                                                                               |
| E111 | §11.1    | Comment line not preceded by a blank line, another comment, start of document, or lesser-indented line | Zero-width span at the start of the comment line                                                            |
| E112 | §14      | Line indent is less than the preceding non-blank line's indent and no ancestor has the same indent     | The leading spaces of the line                                                                              |
| E113 | §14      | Line indent exceeds the preceding non-blank line's indent by more than one                             | The leading spaces of the line                                                                              |
| E114 | §14, §17 | Line would become a child of a comment, tabulation, or tabulated row                                   | Zero-width span at the start of the line                                                                    |
| E115 | §15      | Source atom introduced when the preceding compound already has a source or literal atom                | The first line of the duplicate source atom                                                                 |
| E116 | §16      | Literal atom introduced when the preceding compound already has a source or literal atom               | The opening delimiter line of the duplicate literal atom                                                    |
| E117 | §16      | Literal atom reaches end of file before its closing delimiter line                                     | The opening delimiter line                                                                                  |
| E118 | §17      | Tabulated row has an indent different from the tabulation line                                         | The leading spaces of the row                                                                               |
| E119 | §17      | Hard space on a tabulated row does not end at a column start boundary                                  | The misaligned hard-space run                                                                               |
| E120 | §17      | Consecutive spaces appear within a keyword, pre-column atom, or column value on a tabulated row        | The consecutive space characters within the value                                                           |
| E121 | §17      | Column value exceeds the maximum width for that column                                                 | The overflowing column value                                                                                |
| E122 | §11.2    | Malformed tabulation line heading                                                                      | The malformed heading region (from the marker to the next marker or end of line)                            |
| E123 | §4       | `CR` not immediately followed by `LF`, or line-ending mode inconsistency                               | The `CR` character (or `CR LF` pair that violates the established mode)                                     |
| E124 | §8.1     | Schema identifier is not a valid URL or bare BASE64-URL hash                                           | The schema identifier atom                                                                                  |
| E125 | §8       | Pragma line has extra atoms beyond the expected parameters, or contains a remark                       | The first extra atom, or the remark introducer                                                              |

#### Schema Errors (E2xx)

| Code | Section   | Description                                                                                                               | Span                                           |
| ---- | --------- | ------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| E201 | §20.1     | `Schema.document` is not a `Struct`                                                                                       | The `document` field in the schema             |
| E202 | §20.1     | Duplicate keyword within a `Struct` (across `Product` keywords and `Sum` variant keywords)                                | The second occurrence of the duplicate keyword |
| E203 | §20.1     | `Sum` member has an empty `variants` list                                                                                 | The `Sum` member definition                    |
| E205 | §20.2     | Root struct has a `required` atom-assignable member (unreachable: the document root has no atoms)                         | The `required` member definition               |
| E206 | §20.1     | `Primitive` has a non-null `default` but appears in a non-`required` member                                               | The `default` field of the `Primitive`         |
| E207 | §20.3     | Two or more `Layer`s within a `Schema` share the same `id`                                                                | The second `Layer` with the duplicate `id`     |
| E208 | §20.3     | A `Layer` `Sum` member has a variant keyword that overlaps with an existing keyword in the base `Struct`                  | The overlapping variant keyword in the layer   |
| E209 | §20.3     | A `Layer` `Product` member matches an existing keyword but the base or layer member is not a `Product` with `Struct` type | The layer member definition                    |
| E210 | §20.1     | `Schema.sigil` is non-null and is a space, `LF`, `CR`, letter, digit, or parenthetical symbol                             | The `sigil` field value                        |
| E211 | §8, §20.1 | The keyword `pragma` appears as a `Product.keyword` or `Variant.keyword` in any `Struct`                                  | The keyword definition containing `pragma`     |

#### Validation Errors (E3xx)

| Code | Section | Description                                                                                           | Span                                                                                                     |
| ---- | ------- | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| E301 | §20.2   | Compound's type is not a `Struct`                                                                     | The compound's keyword                                                                                   |
| E302 | §20.2   | More atoms on a compound than there are assignable member positions                                   | The first excess atom                                                                                    |
| E303 | §20.2   | Atom appears at a member position that is not atom-assignable                                         | The atom                                                                                                 |
| E304 | §20.2   | Atom text matches no variant keyword of a `Sum` member                                                | The atom                                                                                                 |
| E305 | §20.2   | Atom text does not match a `Product` member's `Flag` keyword                                          | The atom                                                                                                 |
| E306 | §20.2   | Compound keyword is not recognized for its parent type                                                | The compound's keyword                                                                                   |
| E307 | §20.2   | Required member absent, and member is not a `Product` with a `Primitive` type with non-null `default` | Zero-width span at the end of the parent compound's last child (or at the parent keyword if no children) |
| E308 | §20.2   | Non-repeatable member is filled more than once                                                        | The keyword of the second occurrence                                                                     |
| E309 | §20.2   | Compound children of the same member are not contiguous                                               | The keyword of the non-contiguous child (the second group's first child)                                 |
| E310 | §21     | Primitive value failed validation by the named helper method                                          | As reported by the helper method's diagnostic spans, translated to document offsets                      |
| E311 | §20.2   | `Flag`-typed compound has atoms or compound children                                                  | The first atom or child of the `Flag` compound                                                           |

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
(§19.5) that allows parsing to continue and subsequent errors to be reported.

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
| E112 | Same indentation recovery as E109: use the schema to determine which candidate indent level produces a valid keyword placement, and adjust the margin accordingly.                                                                                                                                                                  |
| E113 | Same indentation recovery as E109: use the schema to determine which candidate indent level produces a valid keyword placement, and adjust the margin accordingly.                                                                                                                                                                  |
| E114 | Same indentation recovery as E109: the line cannot be a child of its apparent parent (a comment, tabulation, or row), so treat it as if indented one level less and use the schema to validate the adjusted placement.                                                                                                              |
| E115 | Ignore the duplicate source atom; use the first one encountered.                                                                                                                                                                                                                                                                    |
| E116 | Ignore the duplicate literal atom; use the first one encountered.                                                                                                                                                                                                                                                                   |
| E117 | Treat the unclosed literal atom's payload as everything from the opening delimiter line to the end of file (excluding the final `LF`, if any).                                                                                                                                                                                   |
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

#### Indentation Recovery (E109, E112, E113)

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

**E112 and E113.** These errors (unmatched dedent and over-indentation) are resolved by the same
mechanism: the parser uses the schema to determine which of the candidate indent levels produces a
valid keyword placement. The margin adjustment and span-extension rules apply identically.

## 20. Schema Language

A schema is expressed using the following types:

```typescript
interface Schema {
  id: string;
  document: Type;
  layers: Layer[];
  sigil: Sigil | null;
}

interface Layer {
  id: string;
  root: Struct;
}

type Type = Struct | Primitive | Flag;

interface Struct {
  members: Member[];
}

interface Primitive {
  validator: string;
  default: string | null;
}

interface Flag {}

type Member = Product | Sum;

interface Product {
  required: boolean;
  repeatable: boolean;
  keyword: string;
  type: Type;
}

interface Sum {
  required: boolean;
  repeatable: boolean;
  variants: Variant[];
}

interface Variant {
  keyword: string;
  type: Type;
}
```

`Schema.id` is an ASCII string identifier for the schema. It MUST consist only of printable ASCII
characters other than whitespace.

`Schema.document` is the type of the document root. It MUST be a `Struct`.

`Schema.layers` is an ordered list of `Layer` values defining optional schema extensions. The empty
list is the normal case for a schema with no layers. Layer composition is defined in §20.3.

`Schema.sigil` is the default sigil for documents that use this schema, or `null` if the schema does
not declare one. When non-null, it MUST satisfy the same character constraints as a pragma sigil
(§8): it MUST NOT be a space, `LF`, `CR`, letter, digit, or parenthetical symbol (§5) (**E210**). When a document's pragma omits a
sigil but provides a schema identifier that resolves to a schema with a non-null `sigil`, the
schema's sigil is used as if it had been specified in the pragma (§8.3).

TEL schemas are themselves representable as TEL documents. The TEL schema that describes the TEL
schema language is therefore self-describing; the schema for schemas is identified by the schema ID
`tel-schema`. The serialization of a schema as a TEL document is governed by that schema. Because
schemas are TEL documents, they have a deterministic BinTEL encoding (see the
[BinTEL Specification](bintel-spec.md)), which is used for schema hashing and identification (§8.1).

`FIXME:` The `tel-schema` schema — the concrete TEL representation of the types defined above
(`Schema`, `Layer`, `Type`, `Struct`, `Primitive`, `Flag`, `Member`, `Product`, `Sum`, `Variant`) —
is not yet specified. This schema defines the keywords, structure, and validation rules for
expressing schemas as TEL documents. Once defined, the `tel-schema` will be self-describing: it is
itself a TEL document conforming to itself.

A `Struct` has an ordered list of `Member`s. Each member describes one logical child slot of the
struct and is either a `Product` or a `Sum`. Both carry the common properties `required` and
`repeatable`:

- `required`: if `true`, the slot MUST be present at least once in a conforming document
- `repeatable`: if `true`, the slot MAY appear more than once; if `false`, it MUST appear at most
  once

A `Product` member has a single `keyword` and a single `type`. It represents a child whose keyword
and type are fixed.

A `Sum` member has a non-empty list of `Variant`s. Any variant's keyword may be used to fill that
slot; the chosen keyword determines the type of the child node placed in that slot.

`Variant.keyword` is the keyword by which a child compound of that variant is written in TEL when
explicit. `Variant.type` may be any `Type`. A `Sum` value looks and behaves exactly like one of its
variants: if the chosen variant has `Struct` type, the compound child has that struct's members as
children; if the variant has `Primitive` type, the compound child carries a value; if the variant
has `Flag` type, the compound child is a bare keyword with no content.

A `Sum` member is **atom-assignable** if and only if all of its variants have `Flag` type. A `Sum`
with any non-`Flag` variant may only be filled by compound children with explicit keywords.

A `Primitive` type represents a leaf value constrained by a validator. `Primitive.validator` names
the helper method to be invoked to validate the atom text (§21). `Primitive.default` is either
`null` (no default) or a string giving the value to be used when the member is absent. A non-null
default MAY only be specified if the `Primitive` appears in a `required: true` member; specifying a
non-null default on a non-required member is a schema error (**E206**). When a required `Product`
member whose type is a `Primitive` with a non-null default is absent from the document, the default
value is used as the semantic value and no E307 error is raised.

A `Flag` type carries no value of its own. Its identity is entirely determined by its keyword
(`Product.keyword` or `Variant.keyword`): in compound position, a `Flag`-typed node is written as
the keyword alone, with no inline atoms; in atom position, the atom text is matched against the
keyword. A `Flag`-typed member SHOULD NOT be `required`, since a required `Flag` member would be
unconditional boilerplate.

**Member ordering recommendation.** The order of members in a `Struct` determines which children can
be serialized as inline atoms (see the `construct` operation in §22.2). To maximize the use of
inline atoms, schema authors SHOULD order members as follows:

1. Required `Product` members with `Primitive` type (most likely to always be present).
2. Non-required `Product` members with `Primitive` type, prioritizing those most likely to be
   specified rather than absent.
3. Either an all-`Flag` `Sum` member or a single `repeatable` `Product` member with `Primitive` type
   — but not both, since only one of these can appear in the trailing atom position.
4. All remaining members (`Struct`-typed products, mixed-variant `Sum` members, and any further
   members), which will always be serialized as compound children.

This ordering is a recommendation, not a requirement. Any member order is valid.

**Member order.** The **member order** of a `Struct` is the sequence of its members in their
declaration order within `Struct.members`. Where a specification rule refers to members "in member
order", it means iterating `members[0]`, `members[1]`, …, `members[n−1]`.

**Keyword order.** The **keyword order** of a `Struct` is a flat sequence of (keyword, type) pairs
obtained by expanding each member in member order: a `Product` member contributes a single entry
(its keyword and type); a `Sum` member contributes one entry per variant, in the declaration order
of `Sum.variants`. Keywords are numbered from 0 in this sequence; the position of a keyword in
keyword order is its **keyword index**.

**Identifier naming convention.** Programmatic identifiers defined by this specification — including
helper method names in `Primitive.validator` and the edit operation identifiers in §22.2 — use
**kebab-case**: a sequence of lowercase ASCII words separated by hyphens (e.g. `update-value`,
`switch-variant`). Schemas SHOULD use kebab-case for validator names.

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

- `Schema.document` is not a `Struct` (**E201**)
- within a single `Struct`, the same keyword appears more than once across all members (considering
  `Product.keyword` and every `Variant.keyword` within each `Sum`) (**E202**)
- a `Sum` member has an empty `variants` list (**E203**)
- a `Primitive` has a non-null `default` and appears in a `Member` with `required: false`
  (**E206**). Absence of a non-required member always means the member is absent; defaults are only
  meaningful for required members that may be elided in the source document.
- `Schema.sigil` is non-null and is a space, `LF`, `CR`, letter, digit, or parenthetical symbol (§5)
  (**E210**)
- the keyword `pragma` appears as a `Product.keyword` or `Variant.keyword` in any `Struct`
  (**E211**)

### 20.2 Type Assignment Algorithm

Type assignment translates the presentation model into the semantic model by ascribing a type to
every atom and compound node in the tree. It proceeds as a recursive descent over the tree, guided
by the schema.

**Atom-assignable members.** A `Product` member M is _atom-assignable_ if M.type is `Primitive` or
`Flag`. A `Sum` member is atom-assignable if and only if all of its variants have `Flag` type. A
member that is not atom-assignable may only be satisfied by compound children (written with an
explicit keyword), not by inline atoms.

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
   and is an all-`Flag` `Sum` or a `Product` with `Flag` type, and the text of A does not match any
   keyword of M (i.e., M.keyword for a `Product`, or any variant's keyword for a `Sum`). Each
   advanced-past member is recorded as absent.

   b. If `pos` ≥ len(T.members), the document is invalid (**E302**: more atoms than assignable
   member positions).

   c. Let M = T.members[pos]. M MUST be atom-assignable; if it is not, the document is invalid
   (**E303**: atom in non-atom-assignable member position).

   d. Assign A to M:
   - If M is a `Sum`, the matched variant is the one whose keyword equals A's text; if no variant's
     keyword matches, the document is invalid (**E304**).
   - If M is a `Product` with `Flag` type, A's text MUST equal M.keyword; if it does not, the
     document is invalid (**E305**).
   - If M is a `Product` with `Primitive` type, the type of A is M.type regardless of A's text
     (validation against the named helper method is a separate step; see §21).

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

   b. If M.`required` and fill_count = 0: if M is a `Product` whose type is a `Primitive` with a
   non-null `default`, the default value is used as the semantic value and no error is raised;
   otherwise, the document is invalid (**E307**: required member absent and no default available).

   c. If not M.`repeatable` and fill_count > 1, the document is invalid (**E308**: non-repeatable
   member filled more than once).

### 20.3 Schema Layering

A `Schema` may include one or more `Layer` values in its `layers` list. Each layer describes an
incremental extension to the schema's root struct. Layers are append-only: they may add new members
to a struct but may not delete existing members or alter the `required` or `repeatable` properties
of existing members.

**Composed schema identity.** A composed schema is identified by the base schema's `id` together
with the ordered sequence of layer `id`s applied to it. Two schemas with the same base `id` but
different layer sequences are distinct schemas.

**Merge algorithm.** The function `Merge(base: Struct, layer: Struct): Struct` produces a new struct
that incorporates the layer's members into the base:

1. Begin with a copy of `base.members` in member order.

2. Construct the keyword map K for the base struct by iterating it in keyword order: for each entry
   (keyword, type) at member index i, map keyword → (i, members[i]).

3. For each member L in `layer.members` in member order:

   a. **Product members.** If L is a `Product` (keyword W, type T):
   - Look up W in K.
   - **Found:** Let (i, M) = K[W]. M MUST be a `Product` and both M.type and T MUST be `Struct`; if
     either is not, the layer is invalid (**E209**). Replace M.type in the merged member list at
     index i with `Merge(M.type, T)`.
   - **Not found:** Append L as a new member at the end of the member list. Add W → (new index, L)
     to K.

   b. **Sum members.** If L is a `Sum`:
   - Every variant keyword in L.variants MUST be absent from K; if any variant's keyword already
     exists in K, the layer is invalid (**E208**).
   - Append L as a new member at the end of the member list. For each variant V in L.variants, add
     V.keyword → (new index, L) to K.

4. Return the resulting member list as the merged struct.

**Layer validity constraints.** A schema is invalid if any of the following holds:

- Two or more layers within the same schema share the same `id` (**E207**)
- A layer `Sum` member has any variant keyword that already appears in the keyword map of the
  (progressively merged) base struct (**E208**)
- A layer `Product` member matches an existing keyword, but the base member is not a `Product`, or
  the base type, the layer type, or both are not `Struct` (**E209**)

**Composing layers.** To apply a sequence of layers `[L₁, L₂, …, Lₙ]` to a base schema with root
struct R, apply `Merge` iteratively: start with R₀ = R, then Rₖ = `Merge(Rₖ₋₁, Lₖ.root)` for k = 1 …
n. The final Rₙ is the root struct of the composed schema.

### 20.4 BinTEL

The binary encoding of the semantic model, BinTEL, is defined in the companion
[BinTEL Specification](bintel-spec.md). BinTEL provides deterministic serialization of typed TEL
documents and defines the schema signature and value hash constructions used for schema
identification (§8.1) and compatibility checking (§8.2).

When a BinTEL document is to be carried in a text-oriented context, it MAY be encoded as Unicode
text using [BASE-256](base256-spec.md), defined as a companion specification. The BASE-256 textual
form is character-for-byte with the BinTEL byte sequence and is recovered losslessly by the
BASE-256 decoder.

## 21. Validation

Type assignment (§20.2) ascribes a `Type` to every node. For `Struct` and `Flag` types, the
structure of the document is sufficient to determine validity. For `Primitive` types, the atom text
must additionally be checked against a named **validator**.

### 21.1 Validators

A validator is identified by the string in `Primitive.validator`. This string names an external
**helper method** that checks whether a given string value conforms to the primitive's constraints.

Validation of primitive values is, in general, too complex to express within a schema: it may
require external data, complex algorithms, or application-specific logic. TEL therefore delegates
primitive validation to helper methods provided outside the schema.

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

`ValidationRequest.method` is the value of `Primitive.validator`. `ValidationRequest.value` is the
verbatim text of the primitive atom (whether `Inline`, `Source`, or `Literal` in the presentation
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
atom's first content character within the document.

### 21.3 Integration

The `Primitive.validator` field in the schema specifies which helper method provides validation for
that primitive type. A conforming implementation MUST invoke the named helper method for every
`Primitive` atom in the document during validation, unless the implementation explicitly opts out of
primitive validation (for example, during a parse-only pass that does not require full semantic
checking).

If the helper method returns an invalid response, the document is invalid (**E310**). The
implementation SHOULD report each diagnostic entry to the user, associating the span with the
corresponding source location in the original document.

### 21.4 Helper Method Binding

A TEL parser that wishes to enable primitive validation MUST be provided with a **callback
function** that conforms to the helper method interface: given a `ValidationRequest`, it returns a
`ValidationResponse`. The parser invokes this callback for each `Primitive` atom encountered during
validation. How the callback is supplied is determined by the host language or environment (e.g. as
a function parameter, a trait implementation, or an interface injection).

This specification does not prescribe a wire protocol, service discovery mechanism, or serialization
format for helper method invocation. In particular:

- A parser embedded in an application MAY implement the callback directly in the host language.
- An IDE, text editor, or LSP server MAY delegate helper method calls to an external service (e.g.
  via REST, RPC, or a subprocess), but the mechanism by which the editor discovers and configures
  such a service is outside the scope of this specification. From the parser's perspective, the
  editor simply provides a callback that handles the delegation internally.

If no callback is provided, the parser MUST skip primitive validation entirely (no E310 errors are
raised). All other parsing and validation proceeds normally.

## 22. Reserialization and Editing

The presentation model can be mutated to reflect changes to the semantic model, preserving
formatting, comments, tabulations, and remarks wherever possible. Mutations are expressed as
operations on the semantic model, which are then reflected in the presentation layer.

There are two categories of editor:

- **Human editors**, who modify source text directly with full flexibility and no constraints on
  what changes may be made
- **Computer editors** (programmatic transformations), which apply structured operations to the
  semantic model and reserialize through the presentation layer

### 22.1 Comment Attachment and Editing

Each `Block` in the presentation model carries zero or more attached comments (§11.1) that precede
its compounds. These comments travel with the block under programmatic transformations.

When a computer editor deletes a compound, the `Block` that contained it is updated. If the deleted
compound was the only compound in its block, and if the block has attached comments, those comments
are also removed (since their meaning was associated with that block).

When a computer editor moves a compound, its containing block's structure is preserved where
possible: if the move results in the block having no remaining compounds, the block (and any
attached comments) moves with the compound to the new location.

When a computer editor inserts a compound constructed from purely semantic information, no comment
is attached to it initially; it is placed into an existing block or a new empty block as
appropriate.

### 22.2 Computer Editor Operations

A computer editor MUST perform only operations drawn from the following set. Each operation
preserves all presentation-layer details that are not directly affected by the operation: remarks,
`trailingBlankLines` counts, `precedingSpaces` on inline atoms, and tabulation marker offsets are
all retained unless the operation explicitly targets them.

**`delete`** — Remove a compound that is not `required`. Any remark attached to the compound is
removed with it. If the compound's block becomes empty (no remaining compounds), the block and its
attached comments are also removed.

**`replace`** — Substitute a compound for another of the same member type at the same position in
the same block. The replacement retains the original compound's remark and its position within the
block. If the member is a `Sum` and the replacement uses a different variant, the keyword in the
presentation layer is updated accordingly. Attached comments on the block are preserved.

**`construct`** — Create a new compound from purely semantic information, with no presentation-layer
context. The constructed compound carries no remark and has no attached comments. No blank lines
appear between its children. No tabulation is added. The canonical presentation form is determined
by iterating the struct's members in member order:

1. Starting from the first member, each non-repeatable `Product` member whose type is `Primitive` is
   serialized as an inline atom, in member order, for as long as consecutive members satisfy this
   condition and the value can be represented as an inline atom (see atom form escalation below).
2. If the next member after the initial run of non-repeatable primitives is an all-`Flag` `Sum`,
   each present flag is serialized as an inline atom.
3. Otherwise, if the next member is a `repeatable` `Product` whose type is `Primitive`, each
   occurrence is serialized as an inline atom (if representable; see atom form escalation below).
4. All remaining children — including any `Product` members whose type is `Struct`, mixed-variant
   `Sum` members, and any members beyond the first repeatable primitive — are serialized as compound
   children with explicit keywords.

**Atom form escalation.** When serializing a `Primitive` value, the atom form is selected as
follows:

1. **Inline atom**: used if the value contains no `LF` characters and can be represented on the
   parent line without violating the word-separation rules (§10.3) — that is, the value does not
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

**`update-value`** — For a compound or atom whose schema type is `Primitive`, update the atom text
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
  inline atoms are preferred, falling back to source atoms for values containing `LF` characters, and to
  literal atoms as a last resort for values that source atom form cannot faithfully represent.
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

- **Mutation semantics.** The computer editor operations (§22.2) are defined individually. What is
  missing: the semantics of composing multiple operations (ordering, atomicity, conflict
  resolution), and the precise rules for how `replace` determines what constitutes a valid
  replacement.

- **Examples and reference algorithm.** No worked examples or reference parsing algorithm are
  provided. What is missing: example TEL documents with their presentation models, semantic models,
  and [BinTEL](bintel-spec.md) encodings; a reference implementation or pseudocode for the parsing
  algorithm; and
  example schema definitions with layering.
