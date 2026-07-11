# TEL Specification Draft

## Abstract

TEL is a line-oriented, tree-structured, typed data language designed for data that is read, written
and maintained by _humans_, intelligent _agents_ or deterministic _processors_.

TEL defines a **presentation model**, which preserves the document as written, and a
schema-driven **semantic model**, which ascribes a type to every element of the tree; the two
are connected by a deterministic type-assignment algorithm (§3). A companion specification,
[BinTEL](bintel.md), defines a compact binary encoding that provides an unambiguous
serialization of the semantic model.

The design of TEL is motivated by the following goals:

- **Formatting preservation.** Machine edits should not disturb formatting, comments or whitespace
  on lines they do not semantically change, so that line-based version control produces minimal,
  meaningful diffs.
- **Minimal escaping.** Syntax conflicts between content and structure should be rare; literal and
  source atoms allow arbitrary content with no character escaping.
- **Strict, recoverable parsing.** Parsing is unambiguous and every parsing error condition has
  a defined recovery strategy, so that a single mistake does not shadow subsequent errors.
- **Schema-driven typing.** Every element is typed by a schema. Validation, including string-level
  constraints, is an integral part of the format rather than an external layer.
- **Layered extensibility.** Schemas support append-only layering, enabling forwards-compatible
  extensions with clear compatibility semantics.
- **Human and machine editors.** The format is designed for direct human authorship, IDE tooling
  with immediate feedback, programmatic transformation, and AI-assisted editing alike.

## 1. Status

This document is a draft specification of TEL.

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

When written to a file, a TEL document MUST be encoded as UTF-8. A byte sequence that is not
well-formed UTF-8 — one containing an invalid byte, an overlong encoding, or an encoding of a
surrogate code point or of a value above `U+10FFFF` — is invalid (**E123**).

Line endings in a TEL document are governed by the following rules (literal atom payloads, defined
in §15, are exempt from all of them):

1. The line-ending style is uniform across the entire document: either every line ends with `LF`, or
   every line ends with `CR LF`.
2. The **line-ending mode** is determined by the first `CR` or `LF` character in the document: if it
   is `CR`, the mode is **CRLF mode**; otherwise the mode is **LF mode**. A document containing
   neither `CR` nor `LF` (a single line with no terminator) is in LF mode.
3. In CRLF mode, `CR` and `LF` may only appear as part of a `CR LF` line ending, except within
   literal atoms (**E120**).
4. In LF mode, `CR` may not appear anywhere in the document, except within literal atoms (**E120**).

LF mode is RECOMMENDED. Human authors SHOULD use LF endings but MAY use CRLF endings. Agents and
processors MUST use LF endings when creating new documents, and SHOULD NOT change the line-ending
mode of an existing document.

No Unicode normalization is required or implied. TEL is defined over the exact Unicode code points
that appear in the serialized text.

A UTF-8 byte order mark MUST NOT appear in a TEL document (**E101**). In a multi-document source
(§6.1), the byte order mark is significant only at the very start of the source, and the
line-ending mode is determined once for the whole source; a document separator terminates the
preceding document exactly as the true end of input would.

Visually misleading code points, such as zero-width characters, SHOULD be avoided. Control-heavy
content SHOULD be avoided except where required. TEL is not intended primarily as a binary-data
format, even though it can represent content containing non-printing code points.

## 5. Significant Characters and Terms

The following three characters have syntactic significance within a line's content:

- `U+000A` LINE FEED (`LF`)
- `U+0020` SPACE
- one other symbolic character designated as the **sigil**

(`U+000D` CARRIAGE RETURN is additionally significant within line terminators in CRLF mode,
§4.) Every other code point is ordinary content. In particular, `U+0009` TAB is not whitespace
for the purposes of indentation, phrase separation, or blank-line detection; a TAB is content
like any letter.

The following definitions apply:

A **line** is a contiguous, potentially empty sequence of non-linefeed characters delimited by `LF`
characters or by the start or end of the file. In CRLF mode (§4), the `CR` immediately preceding
each delimiting `LF` is part of the line terminator and is not part of the line's content.

A **soft space** is exactly one `U+0020` SPACE character.

A **hard space** is two or more consecutive `U+0020` SPACE characters. A **hard-space run** is a
maximal such sequence: a hard space extended as far as the spaces continue in both directions.

A **blank line** is a line containing only `U+0020` SPACE characters, or no characters at all.

A **parenthetical symbol** is one of the eight bracket characters: `(`, `)`, `[`, `]`, `<`, `>`,
`{`, `}`.

A **phrase** is a maximal contiguous sequence of non-linefeed, non-separator characters on a line,
where separators are determined by the phrase-separation rules. A phrase MAY contain soft spaces;
see §10.3.

An **ordinary line** is any non-blank line that is not a comment line (§11.1), a tabulation line
(§16.1), or a payload line of a source atom (§14) or literal atom (§15).

A **document separator** is a line whose content is exactly two sigil characters and nothing else:
no leading spaces, no trailing spaces, and no other characters. It terminates the current document
as if the end of input had been reached, and begins a new document on the following line (§6.1). A
document separator is recognised only at the structural level; within a source atom (§14) or a
literal atom (§15) payload it has no special meaning. A document separator is never a comment
(§11.1) and never a compound (§12); the two-sigil sequence does not denote a keyword.

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

A character is **sigil-valid** if it is one of the twenty-four characters enumerated by `Sigil`
above — equivalently, if it is a single ASCII character that is not `U+0020` SPACE, `U+000A`
LINE FEED, `U+000D` CARRIAGE RETURN, a letter, a digit, a control character, or a parenthetical
symbol (§5). Every constraint on sigil characters in this specification (§8, §20.1, §21.5) is
stated in terms of this definition.

### 6.1 Document Streams

A TEL source MAY contain a sequence of one or more documents, separated by document-separator
lines (§5). This supports streaming consumption of an unbounded sequence of documents, analogous to
newline-delimited JSON.

Each document in the sequence is independent. A document has its own interpreter directive (§7),
its own pragma (§8), its own resolved sigil (§8.3), and its own margin (§9). The document separator
that terminates a document is matched against **that document's** resolved sigil: a document whose
sigil is `#` is terminated by `##`, and a document whose pragma sets the sigil to `%` is terminated
by `%%` (and a `##` line within such a document is ordinary content, not a separator).

A document separator is equivalent to the end of input for the document that precedes it: the
preceding document ends exactly as it would at the true end of input, and a new document begins on
the line immediately following the separator. The separator line is part of neither the preceding
nor the following document, and contributes nothing to either document's presentation model.

A conforming processor MUST offer both of the following parsing modes:

- **Single-document parsing** reads exactly one document and stops at the first document separator,
  returning that one document. Any content after the separator is not processed and need not be
  valid TEL. This is the supported way to prefix arbitrary, possibly non-TEL, content with a TEL
  header: the header is a TEL document, and everything after the separator is opaque to the TEL
  parser.

- **Streaming parsing** yields the documents of the source in order, each parsed independently. A
  document separator that is followed only by blank lines, or by nothing, does not yield a trailing
  empty document. Two consecutive document separators (a separator pair with no content between
  them) delimit, and therefore yield, an empty document.

Because a document separator is recognised only at the structural level, a UTF-8 byte order mark
(§4) is significant only at the very start of the source, not after a separator; the line-ending
mode (§4) is likewise determined once for the whole source.

## 7. Interpreter Directive

If the first two characters of the document are `#!` (`NUMBER SIGN`, `EXCLAMATION MARK`), then the
first line of the document is an interpreter directive line. If not, the interpreter directive is
absent.

The interpreter directive payload is the content of the first line after the leading `#!`, up to but
excluding the line terminator.

If a document has an interpreter directive and also has a pragma, then the pragma MUST appear after
the interpreter directive.

An interpreter directive line is not part of the `children` sequence. It is not part of the semantic
content of the document, but MUST be preserved unchanged by reserialization.

## 8. Pragma

If present, the pragma MUST be the first non-blank line after any interpreter directive line
(**E102**). Its content is parsed by the phrase-separation rules of an ordinary line (§10). The
pragma line is identified by its `tel` keyword; it is exempt from the margin and indentation
rules of §9 and does not set the margin.

If present, the entire pragma line MUST be fully contained within the first 4096 bytes of the
document (**E103**).

The first phrase on the pragma line (its keyword, as defined in §10) MUST be `tel`. The keyword
`tel` is reserved: it MUST NOT appear as a `Field.keyword` in any `Struct`, or as a
`Variant.keyword` in any `SelectDefinition`, within a schema (**E208**).

The pragma line MUST contain at most three phrases after `tel` (version, schema identifier, and
sigil). Any additional phrases are invalid (**E122**). A pattern of the form `<sigil> <text>` that
would otherwise be treated as an inline comment does not apply on the pragma line; any such content
is invalid (**E122**).

The positional form of the pragma is:

```text
tel 1.0 schema-id #
```

The parameters are interpreted in order as follows:

1. TEL version
2. schema identifier
3. sigil

The version parameter is REQUIRED: a pragma consisting of the keyword `tel` alone is invalid
(**E104**). When present, it MUST have the form `x.y`, where `x` and `y` are non-negative
integers (**E104**). `x` is the major version and `y` is the minor version.

The following rules govern how the version number changes across revisions of this specification:

- A revision that rejects a document that would have been accepted by the previous revision MUST
  increment the major version.
- A revision that accepts a previously accepted document but assigns it a different interpretation
  in its presentation or semantic model MUST increment the major version.
- A revision that accepts documents which would not have been accepted by an earlier revision, but
  does not reject or reinterpret any previously valid document, MUST keep the same major version and
  increment the minor version.

The schema identifier parameter is optional.

The sigil parameter is optional. It MUST be a single sigil-valid character (§6) (**E105**).

The parameters are strictly positional: they are not distinguished by form. The sigil can
therefore be specified only when a schema identifier is also given. In `tel 1.0 %`, the `%`
occupies the schema-identifier position and is invalid there (**E121**); it is not read as a
sigil. Consequently, a document whose pragma carries no schema identifier can depart from the
default sigil `#` only through a schema supplied by invocation (§8.2, §8.3).

The default sigil is `#`, used unless the pragma or the document schema specifies a different one.

### 8.1 Schema Identifier

The schema identifier, if present, MUST be one of:

- an HTTP or HTTPS URL, optionally with a fragment (the `#` separator and everything after it)
  that is the **BASE-256-encoded schema signature** of the schema (as defined in §8 of the
  [BinTEL Specification](bintel.md))
- a bare BASE-256-encoded schema signature of the schema

A schema identifier that does not match either of these forms is invalid (**E121**).

The `#` used in the URL form is the standard URI fragment separator (RFC 3986 §3.5). A bare
signature is distinguished from a URL by the absence of a `://` substring. The BASE-256 alphabet
(see the [BASE-256 Specification](base256.md) §4) consists entirely of Unicode letters and
ASCII digits — no whitespace or punctuation — so a schema identifier always occupies a single
phrase and is selected as a single word by a double-click in any conforming text-handling
environment.

A **schema signature** is a deterministic byte string derived from the 256-bit BLAKE3 hashes of
the schema's components (base schema and any layers, in order). It is constructed as a
**palimpsest** of those hashes at the BinTEL-pinned parameters `(H, k_i, k_r) = (32, 4, 2)` —
i.e. an initial cadence of 4 bytes between the base hash and the first layer hash, and a
regular cadence of 2 bytes between subsequent layer hashes, followed by a one-byte cadence
trailer (see §8 of the [BinTEL Specification](bintel.md) and the
[Palimpsest Specification](palimpsest.md)). The palimpsest form is what the pragma carries: it
not only identifies the fully composed schema but also encodes the **identities of the base and
each layer in order**, so that a receiver holding a library of known schemas and layers can
decode the signature to reconstruct the exact composition.

For a non-layered schema (one component), the signature is 33 bytes (33 BASE-256 characters) —
the 32-byte base value hash followed by the one-byte cadence trailer. For a schema with `n ≥ 2`
total components, the signature is `37 + 2·(n − 2)` bytes (37, 39, 41, … BASE-256 characters
for `n = 2, 3, 4, …`). A producer that wishes to extend a schema with additional layers
publishes a new signature by appending each layer's hash to the palimpsest body (and recomputing
the cadence trailer); a consumer that decodes the signature against its library reconstructs
the same composition that the producer intended (§20.3 of this specification).

### 8.2 Schema Resolution

A schema may be supplied in two independent ways when parsing a TEL document:

- an **invocation schema**, supplied directly to the parser by the calling application
- a **document schema**, identified by the schema identifier in the pragma

The following table defines the outcome for each combination:

| Invocation schema | Document schema       | Outcome                                                      |
| ----------------- | --------------------- | ------------------------------------------------------------ |
| absent            | absent                | Untyped document; only the presentation model is available   |
| absent            | present               | Semantic model available, but types are not statically known |
| present           | absent                | Semantic model available with statically known types         |
| present           | present, matching     | Same as invocation-only; types are statically known          |
| present           | present, compatible   | Parsed with invocation schema; types are statically known    |
| present           | present, incompatible | Runtime resolution error (§8.2)                              |

Types are **statically known** when the schema is available at compile time (or equivalent) in the
host language, enabling type-safe access through generated types, type providers, or similar
mechanisms. When types are not statically known, the semantic model is still available but must be
accessed through a dynamic or generic interface.

Two schema identifiers **match** if:

- both carry a signature, and the signatures are identical; or
- neither carries a signature, and the URLs are identical

A schema identifier that carries a signature takes precedence for matching purposes: a URL-only
identifier and a URL-with-signature identifier for the same URL do not automatically match (the
signature is authoritative).

A document carrying signature `S_doc` is **compatible** with a consumer carrying signature
`S_cons` iff `S_doc <: S_cons` under §24 (the formal subtype relation). The signature-subsequence
rule is the concrete decision procedure: `S_doc <: S_cons` iff `S_cons`'s decoded hash sequence is
a subsequence of `S_doc`'s. Every component of `S_cons` (base schema and layers, in order) appears
in `S_doc` in the same order, but `S_doc` may include additional layers between or after them.

Compatibility is directional. Under the Liskov Substitution Principle (§24.5), a consumer
expecting `S_cons` MAY read any document whose carried signature `S_doc` satisfies
`S_doc <: S_cons`, because every constraint imposed by `S_cons` is also satisfied by `S_doc`. The
converse does not hold in general: a consumer expecting `S_doc` cannot necessarily read a document
carrying a supertype `S_cons`, since `S_doc` may require additional layers (and therefore members)
that the supertype does not supply.

#### Resolution Protocol

A parser presented with a document schema MUST resolve it to a `Schema` value (as defined in §20)
before any rule that depends on the schema is applied. Resolution proceeds in the following
order:

0. **Embedded-schema lookup.** When the document is a BinTEL byte sequence in self-contained
   mode (§6.2 of the BinTEL Specification), the parser MUST decode the embedded schema body
   under the built-in `tel-schema` axiom (§20.5), construct the composed `Schema` from it per
   §20.3, and verify that the composed signature recomputed from the embedded body equals the
   signature carried in the document (B11 of the BinTEL Specification on mismatch). On success
   the parser MUST use that composed `Schema` and skip the remaining steps. Step 0 applies only
   to BinTEL inputs in self-contained mode; for TEL-source inputs and external-mode BinTEL
   inputs, resolution begins at step 1.
1. **Built-in lookup.** If the document schema's signature equals the value hash of the built-in
   `tel-schema` schema (§20.5), the parser MUST use the built-in `Schema` and skip the remaining
   steps.
2. **Cache lookup.** A parser MAY maintain an in-memory or on-disk cache keyed by schema signature.
   If the cache contains a `Schema` whose composed signature equals the document schema's
   signature, the parser MUST use that cached `Schema`.
3. **Library lookup.** If the document schema's signature decodes (per §8 of the BinTEL
   Specification) against the parser's library of known schemas and layers, the parser MUST use the
   composition described by the decoded hash sequence.
4. **URL fetch.** If the document schema carries a URL, the parser MAY fetch the body of that URL
   over HTTPS (or HTTP, where the deployment permits non-confidential carriage). HTTPS MUST be
   supported by any conforming network-capable implementation; HTTP support is OPTIONAL. The body
   MUST be a TEL document conforming to the `tel-schema` schema; on parse failure the resolution
   fails. The parser MAY follow up to a small fixed number of HTTPS redirects (3 is RECOMMENDED);
   it MUST NOT follow HTTPS-to-HTTP redirects.
5. **Failure.** If none of the above produce a `Schema`, resolution fails and the parse is aborted.
   The implementation MUST report a runtime resolution error identifying which step failed.

A non-network-capable parser (for example, one embedded in a build tool with no outbound
connectivity) MAY omit step 4; in that case it MUST treat any document schema not satisfied by
steps 1–3 as a resolution failure.

#### Signature Verification

When a document schema's identifier carries a signature (either as a URL fragment or as a bare
signature) and a `Schema` is obtained by URL fetch, the parser MUST verify integrity by:

1. Computing the value hash (§3 of the BinTEL Specification) of the fetched schema document's
   BinTEL encoding.
2. Composing the value hashes of any layers identified by the signature into a candidate signature
   per §8 of the BinTEL Specification.
3. Comparing the candidate signature, byte-for-byte, with the signature carried by the document
   schema.

If the comparison fails, the fetched schema MUST be discarded and resolution fails. A parser MAY
cache only verified schemas.

When the document schema carries a URL with no fragment (no signature), no verification is
possible; the parser MUST treat the fetched body as authoritative, with the understanding that the
binding is then by URL alone.

#### Caching

Schema bodies MAY be cached indefinitely after successful resolution and verification, keyed by
signature. URL-only resolutions (no signature) SHOULD honour standard HTTP cache headers; an
implementation that does not support HTTP caching MUST treat URL-only schemas as freshly
resolvable on every parse.

#### Layered-Signature Decomposition

When the document schema's signature contains more than one component (signature byte length
`37 + 2·(n − 2)` for `n ≥ 2`, per §8 of the BinTEL Specification), the parser MUST decompose
it against its library of known hashes before parsing the document body. Decomposition produces an ordered sequence
`h₀, h₁, …, h_{n-1}` of component value hashes; the parser MUST construct the composed `Schema` by
applying the layers identified by `h₁ … h_{n-1}` to the base schema identified by `h₀`, in that
order, using the merge algorithm of §20.3. If any component hash is unknown to the parser's
library and cannot be retrieved by URL (the signature alone does not encode a URL for any
component), resolution fails.

#### Runtime Resolution Error

A resolution failure is a runtime error, not a parsing error: it is reported outside the E1xx /
E2xx / E3xx taxonomy. Implementations SHOULD report it with sufficient detail for the user to
distinguish the failure modes (built-in mismatch, library miss, fetch failure, signature
verification failure, malformed body).

### 8.3 Sigil Resolution

The sigil is determined in the following order of increasing precedence:

1. The default sigil (`#`)
2. The sigil declared by the resolved schema, if any
3. The sigil specified in the pragma, if present

The sigil MUST be determined before parsing any content after the pragma line. If the effective
sigil requires the schema (i.e., the pragma does not specify a sigil and the schema may declare
one), the parser MUST resolve the schema before continuing.

The sigil declared by a schema is given by the `sigil` field of the `Schema` type (§20).

The pragma line is not included in the `Document.children` sequence. It is recorded only in the
`Document.pragma` field.

The resolved sigil is not itself a field of `Document` (§6): it is derived deterministically
from `Pragma.sigil`, the resolved schema's `sigil`, and the default, by the precedence above.
An implementation MUST retain or re-derive it for reserialization, since comments, remarks, and
tabulations are written using the resolved sigil (see also the sigil invariant of §22.2).

## 9. Lines, Margin, and Indentation

A TEL document MAY begin with zero or more blank lines.

A document containing no non-blank lines (other than an interpreter directive or pragma) is valid
and has an empty `children` list.

The **margin** is determined as follows:

- If the document begins with an interpreter directive, the margin is zero.
- Otherwise the margin is the sequence of leading spaces on the first non-blank line after the
  pragma line, when a pragma is present, or on the first non-blank line of the document
  otherwise. (The pragma line itself is exempt from the rules of this section and does not set
  the margin, §8.)
- If no such line exists, the margin is zero.

The rules of this section apply to every non-blank line of the document except the pragma line
(§8) and the payload lines of source atoms (§14) and literal atoms (§15), which are exempt from
all of them.

Every such line MUST begin with the margin, optionally followed by additional
spaces. A line which does not begin with the margin is invalid (**E106**).

For each such line, the number of spaces following the margin MUST be even (**E107**). The
**indent** is defined as one half of the number of spaces between the margin and the first non-space
character.

Therefore, after removing the margin, indentation is measured in units of two spaces. When the
margin is taken from a line of the document, that line necessarily has indent `0`; when the
margin is zero because the document begins with an interpreter directive, the first non-blank
line after the directive (and after any pragma) MUST itself have an even number of leading
spaces (E107 otherwise) and its indent MUST be `0` (E111 otherwise, there being no shallower
line for it to follow). Blank lines have no defined indent.

Trailing spaces on a non-blank ordinary line are not permitted (**E108**).

Blank lines have no structural effect, except as explicitly noted in §14 (source atoms), §15
(literal atoms), and §16 (tabulated blocks).

## 10. Keywords and Inline Atoms

Each non-blank ordinary line is parsed into one or more phrases by the phrase-separation rule
(defined in §10.3 below).

The first phrase on a non-blank ordinary line is the **keyword**.

Each subsequent phrase on that line is an **inline atom**.

### 10.1 Keyword Characters

A keyword may contain any Unicode code point except `U+0020` SPACE and `U+000A` LINE FEED.
Non-printing code points are NOT RECOMMENDED in keywords but are not forbidden. Although non-ASCII
keywords are permitted, ASCII keywords are RECOMMENDED for interoperability and
readability.

### 10.2 Inline Atom Characters

An inline atom may contain any Unicode code point other than `LF`, subject to the phrase-separation
rules defined in §10.3 below.

### 10.3 Phrase-Separation Rule

After the keyword, the line is parsed left-to-right.

Initially, a single space is sufficient to terminate a phrase and begin the next phrase.

If a hard-space run occurs anywhere on the same line after the keyword, then from the **start**
of that run onward, only hard-space runs terminate phrases. Soft spaces after that point become
part of the current phrase.

Accordingly:

- before the start of the first hard-space run on a line, either a soft space or a hard-space
  run terminates a phrase
- from the start of the first hard-space run onward, only a hard-space run terminates a phrase
- from the start of the first hard-space run onward, a soft space becomes content within the
  current phrase
- each new line resets this rule

Consequently, from the start of the first hard-space run onward, a phrase may contain soft
spaces but may not contain hard-space runs.

## 11. Comments and Remarks

TEL recognises two presentation-only constructs introduced by the sigil but not contributing to
the semantic model:

- a **comment**, which occupies an entire line and is represented as a line-level presentation node,
- a **remark**, which is attached to a compound line and is not an ordinary child.

A third sigil-introduced construct, the **tabulation** line, also exists; tabulations are defined
together with the tabulated blocks they introduce in §16.

The document's **sigil** — the character that introduces comments, remarks, and tabulations — is
determined by the resolution rules in §8.3.

### 11.1 Comment

A line is a comment line if, after its leading indentation, its keyword is exactly equal to the
sigil, and the line does not qualify as a tabulation line. A line qualifies as a tabulation line if
at least one further occurrence of the sigil appears on the line immediately preceded by a hard
space; in that case the line is a tabulation line and not a comment line, regardless of any other
content.

If the sigil is followed immediately by the end of line, the comment payload is the empty string.

If the sigil is followed by at least one space character, the first such space is consumed as the
comment introducer; the comment payload is the remainder of the line, preserved exactly (including
any additional leading or internal spaces).

A phrase such as `#foo` (the sigil concatenated with other characters) is not a comment keyword.
This makes it possible to use the sigil as part of a word.

The payload of a comment is not further parsed. Spaces inside the payload are preserved exactly.

Comments participate in indentation and structural ordering as line-level presentation nodes. Comments cannot
have children.

A comment line MUST be immediately preceded by one of the following: a blank line, another
comment line, the start of the document (i.e., a comment may be the very first non-blank line),
or a non-blank line at a **strictly lesser indent** than the comment itself — that is, the
comment opens a new (deeper) child block of that preceding compound (**E109**). In particular,
a comment placed between two peer compounds without a preceding blank line is invalid: comments
belong to the head of a `Block` (§17), so a comment among peers must be preceded by a blank
line, which closes the current block and lets the comment open the next one. This keeps comment
attachment unambiguous — a comment always precedes the block it belongs to, and never floats
between siblings of an existing block. Because a blank line terminates any active tabulated
block, the rule also ensures that comments cannot appear inside tabulated blocks. Example:

```text
parent              # indent 0
  # comment         # indent 1 — preceded by a non-blank line at lesser indent (0); valid
  child             # indent 1
```

A comment is **attached** to the immediately following compound or tabulation line if there is
no blank line between them *and* that line is at the same indentation level as the comment.
When the following line is a tabulation line, the comment is attached to the tabulated block
that the tabulation line introduces. A comment that is followed by a blank line, by end of
input, by a line at a shallower indentation level, or by a line at a deeper indentation level
is **free-standing**.

Comment attachment is a semantic property recorded in the presentation model. It is significant
during programmatic editing: when a node is moved or deleted, its attached comments travel with it
or are removed with it.

### 11.2 Remark

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

## 12. Compound Tree Structure

Each non-comment non-tabulation ordinary line defines a `Compound` node whose keyword is the line
keyword.

Each subsequent inline atom after the keyword defines an `Atom.Inline` attached to that compound,
unless superseded by the remark rule.

After its inline atoms, a compound may have zero or more child blocks (§17), determined by
indentation and blank-line structure.

## 13. Parent, Child, and Peer Relations

For each non-blank line after the first non-blank line, excluding lines consumed by source atoms or
literal atoms, let the **previous compound line** be the most recent preceding non-blank compound
line (i.e., excluding comment lines and tabulation lines):

- if its indent is exactly one greater than that of the previous compound line, it is a child of the
  previous compound line
- if its indent is equal to that of the previous compound line, it is a peer of the previous
  compound line
- if its indent is less than that of the previous compound line, it closes one or more open
  compounds and becomes a peer of the nearest preceding compound line with the same indent; if no
  preceding compound line has the same indent, the document is invalid (**E110**)

A line may not have indent greater than one plus the indent of the previous compound line, except
where the source-atom or literal-atom rules apply (**E111**).

Comments and tabulations follow the same indentation and peer/child rules as compounds during
parsing, except that comments and tabulations cannot have children. A line that would become a child
of a comment or tabulation is invalid (**E112**). In the resulting presentation model, comments and
tabulations are absorbed into `Block` nodes (§17) and do not appear as standalone siblings of
compounds.

## 14. Source Atoms

If a line immediately follows a compound line with no intervening blank line, and its indent is
exactly two greater than that compound line's indent, then it begins a source atom, provided:

- the preceding compound does not already have a source atom or literal atom

A source atom is represented in the presentation model as `Atom.Source(text)` and is appended to
the end of the atom sequence of the immediately preceding compound.

A compound may have at most one source atom. Introducing a source atom when the preceding compound
already has a source or literal atom is invalid (**E113**).

The source atom begins on the double-indented line and includes that line together with each
subsequent line until either:

- the end of the document is reached, or
- a non-blank line is encountered with fewer leading spaces (after the margin) than the first
  source-atom line. (The comparison is on raw leading-space counts: captured lines are not
  ordinary lines, may have an odd number of leading spaces, and so do not always have a defined
  indent per §9.)

Blank lines are permitted *between* two non-blank lines of a source atom. Trailing blank lines —
those between the last non-blank line and the terminating dedent or end-of-file — are not part of
the atom and are discarded.

The captured lines (in order, with trailing blank lines removed) are converted to a single `text`
string by joining them with `LF` as a separator and **no** trailing `LF`: `n` captured lines yield
a `text` field of the form `line_0 LF line_1 LF … LF line_{n-1}`. Because a source atom begins only
on a non-blank line and its trailing blank lines are discarded, `text` never begins or ends with
`LF`. A value that requires a leading or trailing `LF` therefore cannot be carried by a source atom
and must be written as a literal atom (§15). A blank line between two non-blank lines contributes a
zero-length segment, yielding two consecutive `LF`s in `text`.

For each non-blank captured line, exactly the indentation of the first source-atom line is stripped
from the start of the line. Any surplus leading spaces are preserved.

For each captured line, trailing whitespace — any trailing run of characters with the Unicode
`White_Space` property, not only spaces — is stripped. (Source-atom lines are not ordinary
lines, so E108 does not apply to them; trailing whitespace is silently removed rather than
being an error.) This is the one place where TEL treats whitespace other than `U+0020` SPACE
specially: invisible trailing characters on source-atom lines would otherwise survive as
unintended content. A value whose lines must end in such characters is carried by a literal
atom (§15) instead.

Line content is otherwise captured literally. In particular, the sigil has no special meaning inside
a source atom.

Source-atom lines are subject to the normal line rules (§5): in CRLF mode, the `CR` preceding each
`LF` is part of the line terminator and is not part of the line content. The `LF` characters that
appear in `text` are the synthetic separators introduced by the join above; they are not the
document's literal line terminators.

Source-atom lines are not compounds and are never members of a tabulated block. A source atom always
terminates any surrounding tabulated block.

After a source atom ends, parsing resumes normally. The next non-source-atom line is evaluated for
indentation relative to the compound that introduced the source atom, as if the source atom lines
were not present.

A document separator (§6.1) is at column zero, which is below the indent of any source-atom line, so
it terminates the source atom by dedent and is then recognised as a separator. A source atom is
therefore never split across a document separator.

## 15. Literal Atoms

If a line immediately follows a compound line with no intervening blank line, and its indent is
exactly three greater than that compound line's indent, then it begins a literal atom, provided:

- the preceding compound does not already have a source atom or literal atom

A literal atom is represented in the presentation model as `Atom.Literal(text)` and is appended to
the end of the atom sequence of the immediately preceding compound.

A compound may have at most one literal atom. Introducing a literal atom when the preceding compound
already has a source or literal atom is invalid (**E114**).

The opening literal-atom line is called the **delimiter line**. It is not part of the payload.

The remainder of that opening line, from its first non-space character up to but excluding the
line terminator, with any trailing spaces removed, is the **delimiter**. The delimiter line
therefore consists of the document margin, the opening indentation, and the delimiter.

The delimiter SHOULD consist only of ASCII characters other than whitespace. This is a
recommendation to authors, not an enforced constraint: the delimiter is taken verbatim, and the
closing delimiter line is matched exactly, whatever characters the delimiter contains.

A delimiter is never empty: a line containing only spaces is a blank line per §5, has no
defined indent, and so never satisfies the "indent exactly three greater" trigger above. It
contributes no structural effect.

The literal payload begins immediately after the line terminator of the delimiter line.

The **closing delimiter line** is the first line after the delimiter line whose content is
identical to the delimiter line's content: the same margin, the same opening indentation, and
the same delimiter, and nothing else. Lines are compared on their content as defined in §5 (in
CRLF mode, the `CR` belonging to a line terminator is not part of a line's content). The
content to match is fully determined once the delimiter line has been read; no margin stripping
or indentation processing is applied to candidate lines.

The payload is the character sequence strictly between the `LF` that terminates the delimiter
line and the `LF` that immediately precedes the closing delimiter line, preserved verbatim. In
CRLF mode, the `CR` that precedes that final `LF` is part of the payload — the terminator of
the last payload line contributes its `CR`, and interior payload line breaks likewise retain
their `CR`s.

For example, a literal atom with delimiter `END`, opened three indent levels below its compound
line `inner`, looks like this (note that the closing delimiter line mirrors the opening one
exactly, and that the payload's leading spaces are preserved because no indentation is stripped):

```text
outer
  inner
        END
        leading-space-preserved-content
        END
  sibling-of-inner
```

Accordingly, an empty literal payload (a delimiter line immediately followed by the closing
delimiter line) is permitted.

Because the closing delimiter line carries the opening indentation, a payload line consisting of
the bare delimiter at a different indentation (for example, `END` at column zero in the document
above) is ordinary payload content and does not terminate the atom. Only a payload line whose
content exactly equals the delimiter line's content cannot be represented with that delimiter; a
different delimiter must be chosen (§22.2).

The literal payload preserves leading spaces, trailing spaces, internal spaces, and all other
content exactly.

If the end of file is reached before a closing delimiter line is encountered, the document is
invalid (**E115**). A closing delimiter line MAY be the last line of the document, with or
without a trailing line terminator.

A document separator (§6.1) appearing within a literal-atom payload is ordinary payload content and
does not terminate the document: a literal atom is terminated only by its closing delimiter line.
The two-sigil sequence is preserved verbatim, like any other payload content. (A document separator
has no leading spaces, so it can never match a closing delimiter line, which always begins with at
least the opening indentation.) Consequently, a literal atom is never split across a document
separator; only the true end of input (not a separator) can leave a literal atom unclosed, which
is **E115**.

The sigil has no special meaning inside a literal atom.

The line-ending mode rules of §4 do not apply anywhere inside a literal atom payload: any `CR`,
bare `LF`, or `CR LF` sequence between the delimiter line and the closing delimiter line is
payload content and does not violate the mode (E120 is never raised against payload content).

Literal atom payload content is raw: it is not subject to any TEL parsing rules. Indentation,
trailing spaces, and all other content are preserved exactly. The only termination condition is
a line whose content equals that of the delimiter line.

Literal-atom lines are not compounds and are never members of a tabulated block. A literal atom
always terminates any surrounding tabulated block.

After the closing delimiter line (and its terminator, if any), parsing resumes normally. The next
non-literal-atom line is evaluated for indentation relative to the compound that introduced the
literal atom, as if the literal atom lines were not present.

## 16. Tabulations and Tabulated Blocks

A **tabulation line** introduces a **tabulated block**: a run of zero or more compound lines
(called **rows**) sharing a fixed column layout. The tabulation line declares the columns; the
rows below fill them.

### 16.1 Tabulation Line

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
  immediately after that hard space MUST be the sigil (i.e., M\_{i+1}) (**E119**). The heading text
  MUST NOT itself contain the sigil (**E119**).
- If M_i is immediately followed by two or more spaces (a hard space), the character immediately
  after those spaces MUST be the sigil (i.e., M\_{i+1}), and the heading for M_i is the empty string
  (**E119** if not).
- Any other character immediately following M_i (including a non-space character) is invalid
  (**E119**).

The column heading for M_0 labels the keyword and pre-column area of rows. The column heading for
M_i (i ≥ 1) labels column i and is positioned within column i's span on the tabulation line.

Column headings are preserved in the `Tabulation` node as an ordered list parallel to
`markerOffsets`. An empty string heading is permitted.

Examples:

- `# ID  # Name  # Age` — three markers; headings `["ID", "Name", "Age"]`
- `#  # Name  # Age` — M_0 followed by hard space then M_1; headings `["", "Name", "Age"]`
- `# ID  #  # Age` — M_1 followed by hard space then M_2; headings `["ID", "", "Age"]`
- `# foo  # # bar` — invalid (**E119**): heading for M_1 would contain the marker
- `# foo  #  bar  # baz` — invalid (**E119**): M_1 followed by hard space not immediately preceding
  a marker

### 16.2 Tabulated Block

A **tabulated block** begins immediately after a tabulation line and continues through each
subsequent non-blank line until a blank line is encountered or the end of the document is reached.
Lines within a tabulated block (other than the tabulation line itself) are called **rows**.

In the presentation model, a tabulated block is represented as a `Block` (§17) whose `tabulation`
field holds the tabulation line and whose `compounds` list holds the rows.

A second tabulation line appearing before any intervening blank line — whether or not any rows
have appeared under the first — terminates the current `Block` and begins a new `Block` with
the new tabulation. Both tabulation lines are preserved (the first block then simply has no
rows). The preceding block's `trailingBlankLines` is zero, indicating that no blank lines
separate the two tabulated sub-blocks.

Every non-blank row MUST be an ordinary compound line (comments cannot appear inside a tabulated
block, since the blank line that would have to precede a comment per §11.1 would itself
terminate the block). Every row MUST have the same indent as the tabulation line (**E116**).
Rows MUST NOT have child lines (**E112**).

**Row structure.** Each row consists of a keyword and zero or more **pre-column atoms**, followed
by zero or more **column values**. The whole row is parsed by the phrase-separation rules of
ordinary lines (§10.3): the keyword and pre-column atoms occupy the portion of the row before
the first hard-space run, and each column value is a phrase in hard-space mode. The
column-based grammar below adds geometric constraints on top of §10.3 — hard-space runs, and
therefore column boundaries, must align with the marker offsets `M_i` declared on the
tabulation line — but does not alter how the row's phrases are determined.

**Spacing constraint.** Every contiguous run of two or more space characters (a hard space) on
a row MUST end at position M_i − 1 for some column i that is present on the row (**E117**).
Equivalently, no two consecutive space characters may appear at any other position on the row:
not within the keyword, within pre-column atoms, or within a column value.

This rule implies:

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
permitted. A row MUST NOT have trailing spaces (**E108**).

**Column values and typing.** For type assignment (§20.2), a row is an ordinary compound line:
its keyword, pre-column atoms, and column values are the line's keyword and inline atoms, in
that order. The column grammar of this section governs only the spacing and width of the row's
text, not its semantics. A column that is absent from a row therefore contributes no atom, and
a present column with an empty value likewise contributes no atom — semantically, an empty
value is indistinguishable from an absent column. Whether the corresponding member is then
supplied by its default, treated as absent, or an error is determined by the constraint check
of §20.2 (step 5).

**Width constraint.** For each present non-final column i, its value MUST NOT exceed M\_{i+1} − M_i
− 2 code points in width (**E118**). The final column is unbounded.

**Remarks.** Remarks are permitted on rows. A remark on a row is recognised exactly as on an
ordinary line (§11.2): the sigil at the start of a phrase, immediately followed by exactly one
soft space. On a row, a phrase starts after a hard-space run, so a remark is introduced by a
hard space, then the sigil, then one soft space; the remark extends to the end of the line. The
hard space preceding the remark's sigil, and the remark payload itself, are exempt from the
column spacing constraints (a hard space ending at a position other than some M_i − 1 is not
E117 when what follows it is a remark introducer) and are not subject to column-width limits.

If a row violates any of these constraints, the document is invalid (see **E116**, **E117**, and
**E118**).

## 17. Presentation Nodes

The presentation-layer node types — referenced by name throughout §11–§16 — are:

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
  text: string;  // captured lines joined with single LF separators
}

interface Literal {
  delimiter: string;
  text: string;
}
```

These distinctions are presentation-only. Elsewhere in this specification the three atom
interfaces are written qualified by the union they belong to — `Atom.Inline`, `Atom.Source`,
and `Atom.Literal` — to distinguish them from unrelated names.

In the semantic model, all atom presentation forms are just atoms, and the distinction between atom
and compound disappears in favor of typed elements.

A `Block` is the primary structural grouping within a compound's children. It consists of, in order:

- zero or more **comments** (contiguous, with no blank lines between them)
- an optional **tabulation** line, which applies to all compounds in the block
- zero or more **compound children** (rows if the tabulation is present, ordinary children
  otherwise)
- a count of **trailing blank lines**: the number of blank lines that follow the last compound (or,
  if compounds is empty, the last comment) in the block

A block has at most one tabulation. If a tabulation is present, it appears after any attached
comments and before the first compound child. A block with a tabulation and no compound children
arises when a tabulation line is immediately followed by a blank line, by another tabulation
line, or by the end of its parent's children; it is preserved in the presentation model and
contributes nothing to the semantic model.

A block whose `compounds` list is empty and whose `comments` list is non-empty represents a
free-standing comment group (a comment or comments not immediately followed by any compound at the
same level). Such a block has no tabulation.

Each `Atom.Inline` records the number of spaces immediately preceding it on its source line. Each
`Atom.Literal` records the delimiter string used to open and close it, in addition to the payload
text. Only the delimiter text is recorded: the full opening and closing delimiter lines are
reconstructed on serialization from the margin, the compound's indent plus three levels, and the
delimiter (§15).

## 18. Presentation Model and Semantic Model

TEL defines both a presentation model and a semantic model.

### 18.1 Presentation Model

The presentation model is constructed during parsing. When a schema is available, parsing and
type assignment proceed together — the result is a presentation model and a semantic model
produced in a single pass. (The schema also participates in indentation-error recovery; see the
E107 recovery of §19.5.)

The presentation model records:

- the optional interpreter directive
- the optional pragma
- each compound's keyword, atoms, remark, and child blocks, in order
- each block's attached comments, optional tabulation (including marker offsets and headings),
  ordered compounds, and `trailingBlankLines` count
- each atom's presentation form (`Inline`, `Source`, or `Literal`), each inline atom's
  `precedingSpaces` count, and each literal atom's `Literal.delimiter` string
- ordering and structure derived from indentation

A conforming serializer MUST produce output that, when parsed, yields an identical presentation
model — every detail in the list above is preserved exactly. A serializer MAY apply the
following normalizations, since the affected details are not recorded in the presentation
model:

- Blank line content MAY be normalized to empty lines (rather than space-only lines).
- A minimum hard space (exactly two spaces) MAY be used before remark introducers.
- Multiple consecutive trailing blank lines at the end of a block MAY be collapsed to the recorded
  `trailingBlankLines` count.

### 18.2 Semantic Model

The semantic model is derived from the presentation model by applying the type assignment algorithm
(§20.2) during parsing. The result is a tree of `Element` values:

```typescript
type Element = Node | Value;

interface Node {
  keywordIndex: number | null;  // null only for the document root
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
the type assignment algorithm (§20.2). `Node.children` is the ordered list of child elements; for a
`Flag`-typed node, `children` is always empty.

A `Value` represents a `Scalar`-typed element. It is a leaf: it carries the atom text in
`Value.text` and has no children.

Throughout this specification, **element** is the generic term for a semantic-model value (a
`Node` or a `Value`); an unqualified **node** in a semantic context means the `Node` type
specifically. Presentation-layer objects are named by their concrete kinds — compound, comment,
tabulation, block — or collectively called *presentation nodes* (§17).

Every non-root element carries a `keywordIndex`: the position of the element's keyword in the
keyword order of the parent's `Struct` type (§20). The document root has `keywordIndex = null`. The
`keywordIndex` identifies which member (and, for `Select` members, which variant) the element
fills, and is sufficient to recover the keyword string from the schema.

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
   - If the type assigned to C is `Scalar`, create a `Value` with that type and `text` equal to
     the text of the compound's atom (of any presentation form) if present, or the empty string
     if the compound has no atoms.

3. **Atoms.** For each atom A assigned to the current node (in order):
   - If the assigned type is `Scalar`, create a `Value` with that type and `text` equal to A's
     text.
   - If the assigned type is `Flag`, create a `Node` with that type and an empty `children` list.

4. **Ordering.** Atom-derived elements and compound-derived elements for the same member are
   interleaved in the order they were assigned. Atom-derived elements for a member precede
   compound-derived elements for the same member (atoms appear on the parent line, before any
   child lines).

5. **Defaults.** For each required `Field` member with a `Scalar` type and a non-null `default`
   that was not filled by any atom or compound child: create a `Value` with that `Scalar` type
   and `text` equal to the `Field`'s `default`. This `Value` is placed at the position in the child list
   corresponding to the member's position in member order.

The resulting tree is fully determined by the presentation model and the schema. No ambiguity
remains: the type, value, and child order of every element are fixed by the type assignment
algorithm (§20.2).

## 19. Schema-Governed Structure and Error Diagnosis

In addition to parsing errors, a TEL document may be structurally invalid with respect to a schema.

When a schema is available, it is applied during parsing rather than as a separate
post-processing stage: the result is a presentation model and a semantic model constructed
together in a single pass, and the schema is consulted during indentation-error recovery (the
schema-aware **E107 recovery** of §19.5).

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

Similarly, once atoms are assigned to a *repeatable* all-`Flag` `Select` member, no further
atoms may be assigned to subsequent members: every remaining atom is matched against the
Select's variant keywords, since a repeatable member does not advance the atom phase (§20.2
step 3e). A non-repeatable Select member consumes one atom and advances normally.

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
| E104 | §8       | Pragma version parameter is absent or does not have the form `x.y` with non-negative integers          | The version atom (or the `tel` keyword when absent)                                                                              |
| E105 | §8       | Pragma sigil parameter is not a single sigil-valid character (§6)                                       | The sigil atom                                                                                                                   |
| E106 | §9       | Non-blank line begins with fewer than the margin number of spaces                                      | The leading spaces of the line (zero-width at line start if no spaces)                                                           |
| E107 | §9       | Relative indentation after the margin is odd                                                           | The leading spaces of the line; extended through subsequent lines if margin adjustment persists (see Indentation Recovery below) |
| E108 | §9, §16  | Trailing spaces on a non-blank ordinary line or tabulated row                                          | The trailing space characters                                                                                                    |
| E109 | §11.1    | Comment line not preceded by a blank line, another comment, start of document, or lesser-indented line | Zero-width span at the start of the comment line                                                                                 |
| E110 | §13      | Line indent does not match any open compound's indent (reserved; see §19.5)                            | The leading spaces of the line |
| E111 | §13      | Line indent exceeds the preceding non-blank line's indent by more than one                             | The leading spaces of the line                                                                                                   |
| E112 | §13, §16 | Line would become a child of a comment, tabulation, or tabulated row                                   | Zero-width span at the start of the line                                                                                         |
| E113 | §14      | Source atom introduced when the preceding compound already has a source or literal atom                | The first line of the duplicate source atom                                                                                      |
| E114 | §15      | Literal atom introduced when the preceding compound already has a source or literal atom               | The opening delimiter line of the duplicate literal atom                                                                         |
| E115 | §15      | Literal atom reaches end of file before its closing delimiter line                                     | The opening delimiter line                                                                                                       |
| E116 | §16      | Tabulated row has an indent different from the tabulation line                                         | The leading spaces of the row                                                                                                    |
| E117 | §16      | Hard space on a tabulated row does not end at a column start boundary (including consecutive spaces within a keyword, pre-column atom, or column value) | The misaligned hard-space run                                                                                                    |
| E118 | §16      | Column value exceeds the maximum width for that column                                                 | The overflowing column value                                                                                                     |
| E119 | §16.1    | Malformed tabulation line heading                                                                      | The malformed heading region (from the marker to the next marker or end of line)                                                 |
| E120 | §4       | `CR` not immediately followed by `LF`, or line-ending mode inconsistency                               | The `CR` character (or `CR LF` pair that violates the established mode)                                                          |
| E121 | §8.1     | Schema identifier is not a valid URL or bare BASE-256-encoded schema signature                         | The schema identifier atom                                                                                                       |
| E122 | §8       | Pragma line has extra atoms beyond the expected parameters, or contains a remark                       | The first extra atom, or the remark introducer                                                                                   |
| E123 | §4       | Document is not a well-formed UTF-8 byte sequence (one error per maximal ill-formed subsequence)       | The replacement `U+FFFD` code point in the decoded text                                                                          |

A document separator (§6.1) is always well-formed and carries no error code; it simply ends the
current document and begins the next.

Schema errors (E2xx) and validation errors (E3xx) arise from violations of the schema language
rules (§20) and document conformance constraints (§21). Their trigger conditions and diagnostic
spans are catalogued at the ends of §20.1 (Schema Validity Constraints) and §21.6 respectively.

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
| E104 | If the version parameter is absent or cannot be parsed as `x.y` at all, parse with the latest known version. If it has the correct format but names an unknown version, use the most recent minor version of the same major version if one is known; if the major version itself is unknown, use the latest known version overall. |
| E105 | Ignore the invalid sigil and use the default sigil (`#`) instead.                                                                                                                                                                                                                                                                   |
| E106 | If the line has exactly one fewer leading space than the current margin, insert a synthetic leading space and parse the line at the current indentation level normally. If the line has two or more fewer leading spaces than the current margin, reset the margin to the line's actual indentation level from that point forward.  |
| E107 | Parse the line's keyword; check which of the two candidate indent levels (±1 space) makes the keyword valid according to the schema; place the line at the chosen candidate depth. See indentation recovery algorithm below.                                                                                                        |
| E108 | Ignore trailing spaces and parse the remainder of the line normally.                                                                                                                                                                                                                                                                |
| E109 | Ignore the missing preceding blank line (or other required predecessor) and treat the comment as normally attached to the following node.                                                                                                                                                                                           |
| E110 | Same indentation recovery as E107.                                                                                                                                                                                                                                                                                                  |
| E111 | The over-indented line is skipped (omitted from the presentation model) and parsing continues with the following line at the original expected indent.                                                                                                                                                                              |
| E112 | Same indentation recovery as E107: the line cannot be a child of its apparent parent (a comment, tabulation, or row), so treat it as if indented one level less and use the schema to validate the adjusted placement.                                                                                                              |
| E113 | Ignore the duplicate source atom; use the first one encountered.                                                                                                                                                                                                                                                                    |
| E114 | Ignore the duplicate literal atom; use the first one encountered.                                                                                                                                                                                                                                                                   |
| E115 | Treat the unclosed literal atom's payload as everything from the opening delimiter line to the end of file (excluding the final `LF`, if any).                                                                                                                                                                                      |
| E116 | Interpret the tabulated row according to its actual hard-space positions regardless of alignment with column markers. Suppress any further alignment errors (E117, E118) on the same row.                                                                                                                                           |
| E117 | Same as E116.                                                                                                                                                                                                                                                                                                                       |
| E118 | Same as E116.                                                                                                                                                                                                                                                                                                                       |
| E119 | Report the error and continue parsing, but disable column-alignment checking for the remainder of the current tabulated block.                                                                                                                                                                                                      |
| E120 | Treat any malformed sequence of consecutive `CR` and `LF` characters as a single line break if it contains at most one `CR` and at most one `LF`; treat it as two line breaks if either `CR` or `LF` appears more than once in the sequence.                                                                                        |
| E121 | Ignore the invalid schema identifier and continue parsing as if no schema identifier were specified. The document is treated as untyped.                                                                                                                                                                                            |
| E122 | Ignore the extra atoms and any remark on the pragma line. Parse the pragma using only the first three atoms (version, schema identifier, sigil).                                                                                                                                                                                    |
| E123 | Replace each maximal ill-formed byte subsequence with a single `U+FFFD` REPLACEMENT CHARACTER (per Unicode §3.9, "maximal subpart" practice) and continue parsing the resulting code-point sequence.                                                                                                                                 |

#### Schema Errors

Schema errors (E2xx) have no per-error recovery strategy: they are properties of a schema, not
of the document being parsed against it. A schema that triggers any E2xx condition is invalid
(§23), and the error is reported against the schema document itself.

#### Validation Error Recovery

All validation errors (E301 through E311) are self-contained: they do not have cascading effects on
the remainder of the type assignment or validation process. An implementation MUST record the error
and continue processing remaining nodes as if the erroneous node were absent or were assigned the
most plausible available type. Specific recovery notes:

- **E311** (`Flag` compound with atoms or children): ignore the atoms and children of the `Flag`
  compound; treat it as a bare keyword with no content.

#### Indentation Recovery (E107, E111)

When a line's relative indentation after the margin is odd (E107), the line sits between two
valid indentation positions: one space shallower and one space deeper. This specification defines two
recovery rules, selected by whether a schema is available at parse time.

**Schema-aware recovery (when a schema is available).** Let `s` be the number of spaces after
the margin. Let `shallower = ⌊s / 2⌋` and `deeper = shallower + 1`. The two candidate depths
correspond to two candidate parent structs: at `shallower`, the parent is the most recent
compound (or the document root if `shallower = 0`) that would accommodate a peer at that
depth; at `deeper`, the parent is the compound at depth `shallower` (the most recent open
compound at that depth).

For each candidate, the parser computes a **validity bit** for the line's keyword `K`:

- A candidate is **valid** if the parent compound exists, its resolved type (after
  `Reference` resolution per §20.2) is a `Struct`, and `K` appears in that `Struct`'s
  keyword order (Field keywords plus the variant keywords of any `SelectRef` member, per
  §20).
- A candidate is **invalid** otherwise. This subsumes the cases where the parent doesn't
  exist (e.g. the line is at the very start of the document and the deeper candidate would
  require an open compound that isn't there); the parent is a comment, tabulation, or row
  (which cannot have children, by §13 and §16); the parent's resolved type is `Scalar` or
  `Flag` (also cannot have children); or `K` is not declared at that position.

The recovery then proceeds:

1. Record an E107 error whose span covers the line's leading whitespace.
2. Choose the placement:
   - if only `shallower` is valid: place the line at `shallower`;
   - if only `deeper` is valid: place the line at `deeper`;
   - if both are valid OR both are invalid: place the line at `shallower` (shallower-wins
     tiebreak).
3. Parsing continues from the next line at the chosen depth.

This procedure is deterministic: a given (line, ancestor-stack, composed schema) triple
always produces the same placement.

**Schema-independent recovery (when no schema is available).** When the parser has no schema
(an untyped document, per the absent-invocation-and-document-schema row of §8.2), the
schema-aware validity check cannot be performed. The parser falls back to the **shallower-wins
rule**:

1. Record an E107 error whose span covers the line's leading whitespace.
2. Treat the line as if its relative indentation were `⌊spaces / 2⌋` (integer division), i.e.
   the shallower of the two adjacent even levels.
3. Parsing continues from the next line as if the recovery had not occurred.

The schema-independent rule is also the fallback embedded in step 2 of the schema-aware rule:
when both candidates are invalid (or both valid), the line is placed at `shallower`. A parser
that prefers a single code path may implement only the schema-independent rule; the
schema-aware rule is then a strict refinement that picks `deeper` only when `shallower` would
necessarily mis-place the line (the keyword is not declared at `shallower`'s parent but IS
declared at `deeper`'s parent).

**E111 over-indentation.** When a line is indented more than one level deeper than the previous
compound line, the over-indented line is recorded as an **E111** error and omitted from the
presentation model. Parsing continues with the next line at the originally expected indent. This
keeps the parser deterministic without requiring schema-aware indent inference.

**E110.** §13 also defines **E110** for the case of a dedent that cannot find a matching
ancestor indent. The recursive parsing algorithm used here cannot produce that case (every
dedent encountered during the recursive descent terminates a `parse_blocks` invocation at the
matching ancestor level). E110 is therefore retained in the error catalogue as a reserved code
for non-recursive implementations — for example, ones that track an explicit ancestor stack and
match dedents against it. Such an implementation surfaces E110 in place of the recursive
parser's silent terminations; the recovery is then the same as E107 (treat the line at the
shallower of the two candidate indent levels). Under the canonical recursive parser of this
specification, E110 never fires.

## 20. Schema Language

A schema is expressed using the following types:

```typescript
interface Schema {
  name: string;
  document: Struct;
  layers: Layer[];
  sigil: Sigil | null;
  records: RecordDefinition[];
  scalars: ScalarDefinition[];
  selects: SelectDefinition[];
}

interface Layer {
  name: string;
  overlay: Struct;
  records: RecordDefinition[];
  scalars: ScalarDefinition[];
  selects: SelectDefinition[];
}

type Definition =
  | RecordDefinition
  | ScalarDefinition
  | SelectDefinition;

interface RecordDefinition {
  name: TypeName;
  members: Member[];
  validators: string[];
  description: string | null;
}

interface ScalarDefinition {
  name: TypeName;
  validators: string[];
  description: string | null;
}

interface SelectDefinition {
  name: TypeName;
  variants: Variant[];
  // Layer-only exclusion operations: each names a variant keyword to remove
  // during the merge of §20.3. MUST be empty in a base schema and in the
  // composed schema (E216 otherwise); an `exclude` is consumed by the merge
  // and never survives composition.
  excludes: string[];
  validators: string[];
  description: string | null;
}

type Type = Struct | Scalar | Flag | Reference;

interface Struct {
  members: Member[];
  validators: string[];
}

interface Scalar {
  validators: string[];
}

interface Flag {}

interface Reference {
  name: TypeName;
}

// The members of a Struct. (A SelectDefinition body is not a Struct: its
// children are Variants and, in a layer only, exclude operations — see
// SelectDefinition.excludes above.)
type Member = Field | SelectRef;

// Per-axis declaration state. "default" means no flag was declared on this
// axis (its effective value is the schema-language default — required=true,
// repeatable=false). "loose" means the loosening flag was declared on the
// base side (`optional` or `repeatable`). "tight" means the tightening flag
// was declared on the layer side (`required` or `irrepeatable`).
//
// Effective booleans are derived from polarity:
//   required   = (member.required   != "loose")
//   repeatable = (member.repeatable == "loose")
//
// The tristate is retained (rather than collapsing to booleans during schema
// construction) so that the layer-merge of §20.3 can detect loosening of an
// already-tight axis (E214, E215) regardless of whether the layer's declared
// flag agrees with or contradicts the base's effective value.
type Polarity = "default" | "loose" | "tight";

interface Field {
  required: Polarity;
  repeatable: Polarity;
  keyword: string;
  type: Type;
  default: string | null;
  description: string | null;
}

// References a SelectDefinition at a member position. The named Select's
// variants become the keywords admissible at this position; the SelectRef
// itself has no own keyword. Polarity lives on the use site (here), not
// on the SelectDefinition.
interface SelectRef {
  required: Polarity;
  repeatable: Polarity;
  reference: TypeName;
}

interface Variant {
  keyword: string;
  type: Type;
  description: string | null;
}

// A TypeName is a PascalCase identifier (§20.7) naming a RecordDefinition,
// ScalarDefinition, SelectDefinition, or a built-in type. Distinct from a
// kebab-case `identifier` (§20.7), which is used for non-type names
// (schema name, layer name, field keywords, variant keywords, validator
// names).
type TypeName = string;
```

`Schema.name` is a kebab-case identifier (§20.7) for the schema. It is a human-readable label used
to identify the schema in source form; it is **not** the same as the schema identifier carried in
a document's pragma (§8.1), which is either a URL or a BLAKE3-256 content hash of the schema's BinTEL.

`Schema.document` is the root `Struct` that defines the type of the document root compound. It
is `Struct`-typed directly (not `Type`-typed) by analogy with `Layer.overlay`: every schema must
define a root struct, and no other `Type` variant is meaningful at the document root. A root
`Struct` with an empty member list is valid; it types only the empty document (any content
raises E306), which is occasionally useful as a base for layers to extend.

`Schema.layers` is an ordered list of `Layer` values defining optional schema extensions. The empty
list is the normal case for a schema with no layers. Layer composition is defined in §20.3.

`Schema.sigil` is the default sigil for documents that use this schema, or `null` if the schema does
not declare one. When non-null, it MUST be a sigil-valid character (§6) (**E207**). When a document's pragma omits a sigil but provides a schema
identifier that resolves to a schema with a non-null `sigil`, the schema's sigil is used as if it
had been specified in the pragma (§8.3).

`Schema.records`, `Schema.scalars`, and `Schema.selects` are ordered lists of `RecordDefinition`,
`ScalarDefinition`, and `SelectDefinition` declarations respectively. Each binds a PascalCase
`TypeName` (§20.7) to a Struct, Scalar, or Sum (union of variants). Together the three lists
populate the schema's **Definition namespace** — the union of `TypeName`s addressable by a
`Reference` or by a `SelectRef`. A `Reference` resolves to whichever Definition carries the
matching name; structurally, records produce `Struct`s with members, scalars produce `Scalar`s
with validators, and selects produce sums with variants. The three lists are kept distinct in
the data model because their bodies differ, but they share a **single namespace**: no two
Definitions across the three lists may share a name within the composed schema (**E210**).

The Definition mechanism is what makes recursive schemas finitely expressible: the
schema-of-schemas itself (see the tel-schema subsection below) is necessarily recursive, and so
is any schema whose data has cyclical structure. The empty list is the normal case for
non-recursive schemas.

A `Layer`'s `name` is a kebab-case identifier (§20.7) labelling the layer. It MUST be unique
across all layers of a schema (**E204**). `Layer.overlay` is a `Struct` whose members are merged
into the composed schema's root struct by the algorithm in §20.3. `Layer.records`,
`Layer.scalars`, and `Layer.selects` are ordered lists of Definitions introduced by the layer;
together they merge with the base schema's `Schema.records ∪ Schema.scalars ∪ Schema.selects`
and any preceding layers' Definitions to form a single namespace visible to all references in
the composed schema. The empty lists are the normal case for layers that only extend the root
struct.

A `RecordDefinition` has a `name`, a list of `members`, and a list of struct-level
`validators`. The `name` MUST be a `TypeName` (§20.7), unique across the composed namespace
`Schema.records ∪ Schema.scalars ∪ Schema.selects ∪ ⋃ Layer.{records,scalars,selects}`
(**E210**, with same-name records, scalars, or selects in *layers* triggering Definition merge
per §20.3 rather than E210). The `members` field is a list of `Member`s, structurally identical
to those of a `Struct`: a `RecordDefinition` is, in effect, a named `Struct`.
`RecordDefinition.validators` mirrors `Struct.validators` (§21.6) and applies to every instance
of the Definition.

A `ScalarDefinition` has a `name` (subject to the same uniqueness rule above) and a list of
`validators`; it is a named `Scalar`. Layer-merge of same-name `ScalarDefinition`s concatenates
the `validators` lists in declaration order, deduplicated.

A `SelectDefinition` has a `name` (subject to the same uniqueness rule), a non-empty list of
`variants`, and a list of `validators`. It is a named sum type. Each variant has a kebab-case
`keyword` (the source-level keyword by which an instance is written) and a `type`; the type may
be any `Type` (Struct, Scalar, Flag, or Reference). A `SelectDefinition`'s `validators` list
mirrors `Struct.validators` for symmetry: each named validator inspects the chosen variant of an
instance and may reject it under cross-cutting rules. Layer-merge of same-name
`SelectDefinition`s removes variants (via `exclude` operations declared in the layer's
SelectDefinition body) and appends validators; a layer MUST NOT introduce a variant with a
keyword absent from the base SelectDefinition (**E213**).

Each `RecordDefinition`, `ScalarDefinition`, `SelectDefinition`, `Field`, and `Variant` also
carries an optional `description`: free-form human-readable text documenting the type, field, or
variant, or `null` when absent. Unlike a comment (§11.1), a `description` is part of the semantic
model: it survives construction and BinTEL round-trips. It is never validated and places no
constraint on instances. Validators carry no `description`; documentation of what a constraint
checks belongs with the validator, not its use site. On layer-merge of same-name Definitions and
same-keyword fields, a layer's non-null `description` overrides the base's; otherwise the base
`description` is inherited. (Per-variant descriptions are not layer-overridable, as the base
variant list is retained wholesale; see §20.3.)

`Flag` types cannot be aliased through `Definition`; they are referenced by the built-in name
`Flag` instead (§20.5).

A `Reference` is a `Type` whose semantic content is delegated to the Definition it points at by
name. During type assignment (§20.2) a value position whose schema type is `Reference(N)` is
treated as if its type were the resolved Definition: a `RecordDefinition` resolves to the
`Struct` formed from its `members` and `validators`; a `ScalarDefinition` resolves to the
`Scalar` formed from its `validators`. A `SelectDefinition` is never the resolved type of a
plain `Reference`: a sum at a member position is always introduced via `SelectRef` rather than
`Field.type`, because a `Field` has a single keyword whereas a sum has none. A `Reference` whose
`name` resolves to a `SelectDefinition` is invalid (**E217**); a `Reference` whose `name` does
not match any Definition in the composed schema is also invalid (**E209**).

A `SelectRef` is a `Member` kind that places a sum at a member position. Its `reference` is the
`TypeName` of a `SelectDefinition`; the SelectRef's polarity (`required`, `repeatable`) lives at
the use site. A `SelectRef` whose `reference` does not match any `SelectDefinition.name` in the
composed schema is invalid (**E209**); one that resolves to a non-`SelectDefinition` (a record
or scalar) is also invalid (**E217**).

Throughout this specification, **Select member** is shorthand for a `SelectRef` member (named
for the `select` keyword that introduces one in schema source, §20.5), and an **all-`Flag`
Select member** is one whose referenced `SelectDefinition`'s variants all resolve to `Flag`.

TEL schemas are themselves representable as TEL documents. The TEL schema that describes the TEL
schema language is therefore self-describing; the schema for schemas has `Schema.name = tel-schema`. The serialization of a schema as a TEL document is governed by that schema. Because
schemas are TEL documents, they have a deterministic BinTEL encoding (see the
[BinTEL Specification](bintel.md)), which is used for schema hashing and identification (§8.1).
The concrete TEL representation of the type model defined above — the keyword vocabulary, member
ordering, and validators used to write a schema as a TEL document — is given in §20.6 and embodied
in the file [`tel-schema.tel`](tel-schema.tel).

A `Struct` has an ordered list of `Member`s. Each member describes one logical child slot of the
struct and is either a `Field` or a `SelectRef`. Both carry the per-axis polarities
`required: Polarity` and `repeatable: Polarity`. Their effective boolean values are derived:

- effective `required` = `(member.required != "loose")` — the slot MUST appear at least once
  unless `optional` was declared
- effective `repeatable` = `(member.repeatable == "loose")` — the slot MAY appear more than once
  only when `repeatable` was declared

**Surface syntax.** A schema document expresses these two polarities via four Flag fields in
the `Field` and `Select` records (§20.5). The default for any Field or SelectRef — neither flag
declared — is `required: "default", repeatable: "default"`, i.e. the **tight** cardinality
`(exactly one)`. To loosen, a base schema declares one of:

- `optional` — sets `required: "loose"` (effective `required` becomes `false`)
- `repeatable` — sets `repeatable: "loose"` (effective `repeatable` becomes `true`)

To re-tighten a member that an earlier layer declared loose, a later layer declares one of:

- `required` — sets `required: "tight"` (overrides a previous `optional`)
- `irrepeatable` — sets `repeatable: "tight"` (overrides a previous `repeatable`)

`required` and `irrepeatable` may also appear in a base schema; there they are redundant
no-ops that re-state the default, since the default is already tight on both axes. They are not
errors.

How the two axes merge when a layer refines a member is defined normatively by the
`MergePolarity` rule of §20.3 (in short: tightening is always permitted, loosening an
already-effective-tight axis is E214/E215, and an undeclared axis carries through unchanged).
The retention of the tristate, in preference to a simple boolean, is what lets that merge
distinguish a layer redundantly restating the existing state from a layer attempting to flip
it.

Tightening is subtype-producing (§24); loosening is not.

A `Field` member has a single `keyword` and a single `type`. It represents a child whose keyword
and type are fixed.

A `SelectRef` member references a named `SelectDefinition`. After Reference resolution, the
referenced SelectDefinition's `variants` become the keywords admissible at this position; the
SelectRef itself has no own keyword. A `SelectRef` value at a member position looks and behaves
exactly like one of the referenced sum's variants: if the chosen variant has `Struct` type, the
compound child has that struct's members as children; if the variant has `Scalar` type, the
compound child carries a value; if the variant has `Flag` type, the compound child is a bare
keyword with no content.

`Variant.keyword` is the kebab-case keyword by which a child compound of that variant is written
in TEL when explicit. `Variant.type` may be any `Type`.

A member is **atom-assignable** when it can be satisfied by an inline atom rather than only by a
compound child:

- A `Field` is atom-assignable iff its type, after `Reference` resolution (§20.2), is `Scalar` or
  `Flag`.
- A `SelectRef` is atom-assignable iff every variant of the referenced `SelectDefinition`, after
  `Reference` resolution, has `Flag` type.

A member with at least one non-atom-assignable position — a `Field` of `Struct` type, or a
`SelectRef` whose referenced sum has any non-`Flag` variant — can only be filled by a compound
child with an explicit keyword. This definition is used by the type-assignment algorithm
(§20.2) and by the construct operation (§22.2); both refer to it rather than restating it.

A `Scalar` type represents a leaf value constrained by zero or more validators.
`Scalar.validators` is an ordered list of helper-method names; each is invoked on the value text
during validation, and the value is accepted only when every validator returns `Valid`
(AND-conjunction). An empty `validators` list means the Scalar accepts any text. Validator
semantics are defined in §21.

`Field.default` is either `null` (no default) or a string giving the value to be used when the
member is absent. A non-null default MAY only be specified on a `Field` whose type resolves to a
`Scalar` and whose effective `required` is `true`; specifying a non-null default on a
non-required member is a schema error (**E203**). When a required `Field` member whose type is a
`Scalar` with a non-null default is absent from the document, the default value is used as the
semantic value and no E307 error is raised. A required member with a default is therefore one
that may be elided from the source but is always present in the semantic model: `default` gives
the value a required-but-elided member takes, whereas a non-required member without a default is
simply absent.

A `Struct` type MAY likewise carry a `validators` list. A struct validator inspects the entire
struct element (its members' values), enabling cross-field constraints such as "postcode is
required when country is UK". Multiple struct validators apply in AND-conjunction. Struct
validators are invoked AFTER all of the struct's children have been individually validated.
Validator semantics are defined in §21.

Validators on scalars and validators on structs share a single namespace (§21.1): a validator
name resolves to one helper method, which dispatches on the request kind at invocation time.

**Serialising `Field.default`.** When a schema is written as a TEL document, the `default`
field of a `Field` carries an arbitrary string and is serialised by atom-form escalation
(§22.3): the first of inline, source, and literal form that can carry the value faithfully.
This mirrors how any `Scalar` value is serialised at a use site, so a schema author writes a
default exactly as they would write any other scalar value.

A `Flag` type carries no value of its own. Its identity is entirely determined by its keyword
(`Field.keyword` or `Variant.keyword`): in compound position, a `Flag`-typed node is written as
the keyword alone, with no inline atoms; in atom position, the atom text is matched against the
keyword. A `Flag`-typed member SHOULD NOT be `required`, since a required `Flag` member would be
unconditional boilerplate.

**Member ordering recommendation.** The order of members in a `Struct` determines which children can
be serialized as inline atoms (see the `construct` operation in §22.2). To maximize the use of
inline atoms, schema authors SHOULD order members as follows:

1. Required `Field` members with `Scalar` type — especially any "identifying" field such as a
   keyword or name, since placing it first lets the whole field be declared inline with the
   identifier as the first atom (e.g. `field some-keyword SomeType repeatable`).
2. Non-required `Field` members with `Scalar` type, prioritizing those most likely to be
   specified rather than absent. Note (§20.8): only the first non-required `Scalar` member is
   fillable as an inline atom; any subsequent non-required `Scalar` members will require explicit
   compound children.
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
(its keyword and type); a `SelectRef` member contributes one entry per variant of its referenced
`SelectDefinition`, in the declaration order of that SelectDefinition's `variants`. Keywords are
numbered from 0 in this sequence; the position of a keyword in keyword order is its **keyword
index**.

**Identifier naming convention.** Programmatic identifiers defined by this specification — including
helper method names in `Scalar.validators` and `Struct.validators` and the edit operation
identifiers of §22.2 — use **kebab-case**: a sequence of lowercase ASCII words separated by hyphens
(e.g. `update-value`, `attach-remark`). Schemas SHOULD use kebab-case for validator names.

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

A schema is itself a TEL document — specifically, a TEL document conforming to the `tel-schema`
schema (§20.5). It is therefore subject to the same two layers of error reporting as any other
TEL document:

- **Parsing and validation against `tel-schema`.** The E1xx (parsing) and E3xx (validation)
  taxonomies of §19.3 apply to the schema document directly. Structural malformations — a
  `field` line appearing inside a `scalar`'s body, an atom in a position where the schema
  expects a compound child, a malformed identifier in a `keyword` slot, and similar — are
  caught here. The error against tel-schema is the only signal a schema author receives for
  this class of mistake. In particular, an unknown child keyword (such as `field` inside
  `scalar-body`) raises **E306** and a non-Struct compound being given children raises
  **E301**.
- **Semantic validity of the resulting `Schema` value.** Once `tel-schema` has accepted the
  document and `construct_schema` (§20.6) has produced a `Schema` value, the constraints below
  apply. They catch errors that the type assignment against tel-schema cannot see — properties
  of the assembled `Schema` model itself, such as duplicate Definition names, malformed layer
  merges, or references to undefined types.

A schema is invalid if any of the following holds:

- within a single `Struct`, the same keyword appears more than once across all members
  (considering `Field.keyword` and every `Variant.keyword` of the `SelectDefinition`s referenced
  by `SelectRef`s); or, within a single `SelectDefinition`, two or more variants share the same
  keyword (**E201**)
- a `SelectDefinition` has an empty `variants` list (**E202**)
- a `Field` has a non-null `default` but its type does not resolve to a `Scalar`, or its
  effective `required` is `false` (i.e. `Field.required == "loose"`) (**E203**). Absence of a
  non-required member always means the member is absent; defaults are only meaningful for
  required members that may be elided in the source document.
- two or more `Layer`s within a `Schema` share the same `name` (**E204**)
- a layer adds a `Field` or `SelectRef` to a `Struct` whose keyword — or, for a `SelectRef`, any
  variant keyword of the referenced `SelectDefinition` — collides with a keyword already present
  in the merged `Struct` (§20.3) (**E205**)
- a layer declares a `Field` whose keyword matches an existing `Field` in the same `Struct`, but
  the two types (after reference resolution) are neither structurally equal nor both `Struct`
  (§20.3) (**E206**)
- `Schema.sigil` is non-null and is not a sigil-valid character (§6) (**E207**)
- the keyword `tel` appears as a `Field.keyword` or `Variant.keyword` in any `Struct` or
  `SelectDefinition` (**E208**)
- a `Reference` names a `TypeName` that is neither a predefined built-in name (§20.5) nor a
  Definition of the composed schema (**E209**); a `SelectRef.reference` not appearing as a
  `SelectDefinition.name` is also E209
- two or more Definitions in the *base*
  `Schema.records ∪ Schema.scalars ∪ Schema.selects` share the same `name`, or a Definition's
  `name` is one of the predefined built-in names (§20.5) (**E210**). Records,
  scalars, and selects share a single namespace; cross-kind name collisions in the base are also
  E210. Same-name Definitions across layers trigger Definition merge per §20.3 rather than
  E210.
- an `Exclude(K)` operation in a layer SelectDefinition body names a keyword K that does not
  identify any variant of the SelectDefinition being merged — the base's variants less any
  variants already excluded by earlier operations (**E211**)
- an `Exclude(K)` operation would empty a `SelectDefinition` referenced by any `SelectRef` whose
  effective `required` is `true` (**E212**)
- a layer's SelectDefinition contains a `variant` declaration whose keyword is absent from the
  base SelectDefinition (variant addition is forbidden — would widen the sum) (**E213**)
- a layer declares `optional` on an axis whose merged-base polarity is `"default"` or `"tight"`
  (a loosening attempt on the `required` axis; §20.3) (**E214**)
- a layer declares `repeatable` on an axis whose merged-base polarity is `"default"` or
  `"tight"` (a loosening attempt on the `repeatable` axis; §20.3) (**E215**)
- an `Exclude` appears outside a layer's SelectDefinition body — i.e. inside `Schema.document`,
  a `RecordDefinition`, or a base-side `SelectDefinition` (**E216**)
- a `Reference` resolves to a `SelectDefinition` (a sum at a position expecting a single typed
  value), or a `SelectRef.reference` resolves to a `RecordDefinition` or `ScalarDefinition`
  (a non-sum at a position expecting a sum) (**E217**)

#### Schema Errors (E2xx)

| Code | Description                                                                                                               | Span                                           |
| ---- | ------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| E201 | Duplicate keyword within a `Struct` (across `Field` keywords and the `Variant.keyword`s of `SelectRef`-referenced `SelectDefinition`s) or within a `SelectDefinition`'s variants | The second occurrence of the duplicate keyword |
| E202 | `SelectDefinition` has an empty `variants` list                                                                            | The `SelectDefinition`'s name compound         |
| E203 | A `Field` has a non-null `default` but its type is not a `Scalar` or the member is non-`required`                          | The `default` field of the `Field`             |
| E204 | Two or more `Layer`s within a `Schema` share the same `name`                                                              | The second `Layer` with the duplicate `name`   |
| E205 | A layer-added `Field` or `SelectRef` has a keyword (or referenced variant keyword) colliding with a keyword already in the merged `Struct` | The overlapping keyword in the layer           |
| E206 | A layer `Field` (or variant restatement in a layer's SelectDefinition) matches an existing keyword but the base and layer types are neither structurally equal nor both `Struct` | The layer member definition                    |
| E207 | `Schema.sigil` is non-null and is not a sigil-valid character (§6)                                                        | The `sigil` field value                        |
| E208 | The keyword `tel` appears as a `Field.keyword` or `Variant.keyword` in any `Struct` or `SelectDefinition` (also §8)       | The keyword definition containing `tel`        |
| E209 | A `Reference` or `SelectRef` names a `TypeName` that resolves neither to a built-in (§20.5) nor to a Definition in the composed schema | The `TypeName` atom                            |
| E210 | Two or more Definitions in the *base* `Schema.records ∪ Schema.scalars ∪ Schema.selects` share the same `name`, or a Definition uses a predefined built-in name (§20.5) (any cross-kind name collision is also E210; same-name Definitions across layers merge instead) | The second Definition with the duplicate name |
| E211 | `Exclude(K)` in a layer's SelectDefinition names a variant K not present in the SelectDefinition being merged             | The `Exclude` operation's variant keyword      |
| E212 | `Exclude(K)` would empty a `SelectDefinition` referenced by a `required` `SelectRef`                                      | The `Exclude` operation's variant keyword      |
| E213 | A layer's SelectDefinition introduces a variant whose keyword is absent from the base SelectDefinition (variant addition widens the sum) | The offending variant declaration |
| E214 | A layer declares `optional` against an axis whose merged-base polarity is `"default"` or `"tight"` (loosening attempt on `required`) | The offending field/select declaration         |
| E215 | A layer declares `repeatable` against an axis whose merged-base polarity is `"default"` or `"tight"` (loosening attempt on `repeatable`) | The offending field/select declaration         |
| E216 | `Exclude` appears outside a layer's `SelectDefinition` body — i.e. inside `Schema.document`, a `RecordDefinition`, or a base-side `SelectDefinition` | The offending `exclude` compound |
| E217 | A `Reference` resolves to a `SelectDefinition`, or a `SelectRef.reference` resolves to a `RecordDefinition` / `ScalarDefinition` (kind mismatch between member shape and resolved Definition) | The offending `TypeName` atom |

### 20.2 Type Assignment Algorithm

Type assignment translates the presentation model into the semantic model by ascribing a type to
every atom and compound in the tree. It proceeds as a recursive descent over the tree, guided
by the schema.

**Reference resolution.** Wherever this algorithm asks "is T a Struct?", "is T a Scalar?", etc.,
the question is asked of the type T after **reference resolution**: if T is a `Reference(N)`, T
is replaced by the `Struct` formed from the `members` of the matching `RecordDefinition`, or by
the `Scalar` formed from the `validators` of the matching `ScalarDefinition`. If `N` is one of
the predefined built-in type names (§20.5), the Reference resolves directly to the built-in:
`Flag` resolves to the `Flag` type, and `String`, `Identifier`, `Sigil`, and `TypeName` each
resolve to a `Scalar` whose single validator is the corresponding built-in validator (§21.5).
A `Reference(N)` resolving to a `SelectDefinition` is **E217** (a sum cannot inhabit a
single-typed position).

`SelectRef` resolution is parallel: a `SelectRef(N)` resolves to the `variants` of the matching
`SelectDefinition`; a `SelectRef` resolving to a `RecordDefinition` or `ScalarDefinition` is
**E217**. The polarity of the SelectRef remains attached to the use site; only the variants are
drawn from the named SelectDefinition.

Reference and SelectRef resolution are single-step (Definitions never name another
Reference/SelectRef as their direct content); no chasing of resolution chains is required.

**Recursive types and depth limit.** A Definition may contain `Reference`s or `SelectRef`s to
itself or to other Definitions that ultimately refer back to it (e.g., a `Tree` RecordDefinition
whose `children` Field is `Reference(Tree)`). Such cycles are well-formed: the schema describes
an arbitrarily deep structure, and any particular document instantiates it to finite depth. Type
assignment processes compounds lazily — each descends one level before resolving the
next Reference — so circularity is naturally bounded by the document's actual nesting depth.

Nevertheless, a conforming implementation MUST defend against pathological inputs by enforcing
a recursion-depth limit during type assignment. The RECOMMENDED limit is **256** levels of
compound nesting. Exceeding this limit is a parser-internal resource error: the implementation
reports it with a clear diagnostic (e.g., "document nesting exceeds the configured limit of
256") and stops type assignment. This is not assigned a TEL error code (E1xx/E2xx/E3xx) because
it is not a property of the document itself — a deeper limit would accept the same input.
Implementations MAY expose the limit as a configurable parameter.

**Atom-assignable members.** The atom-assignable predicate used throughout this section is the
one defined in §20 (after `Reference` resolution). A member that is not atom-assignable may only
be satisfied by compound children (written with an explicit keyword), not by inline atoms.

**Document root.** The document root is a virtual compound with type `Schema.document`. It
has no atoms, so members of the root struct — atom-assignable or not — can be filled only by
compound children (§19.1).

**Type assignment for a compound N with type T:**

1. Dispatch on T (after reference resolution):

   - If T is a `Flag`, N MUST have no atoms and no compound children; otherwise the document is
     invalid (**E311**). Type assignment of N is complete.
   - If T is a `Scalar`, N is a leaf: its semantic value is the text of its single atom (of any
     presentation form — inline, source, or literal), or the empty string if it has none
     (§18.3). If N has more than one atom, the document is invalid (**E302**); if N has any
     compound children, the document is invalid (**E301**: only a `Struct`-typed compound may
     have children). Type assignment of N is complete; validation of the value follows per §21.
   - If T is a `Struct`, proceed with steps 2–5.

2. Construct the keyword map K by iterating T in keyword order: for each entry (keyword, type) at
   member index i, map keyword → (i, type). (Schema validity ensures no duplicate keywords within
   the same struct.)

3. **Atom phase.** Let `pos` = 0. For each atom A in N.atoms, in order:

   a. Advance `pos` while `pos` < len(T.members) AND the member M at `pos` has effective
   `required` = `false` (i.e. M is non-required), AND at least one of the following holds:
   - M is **not atom-assignable** (e.g. a `Field` whose type resolves to a `Struct`, or a
     `SelectRef` whose referenced SelectDefinition has any non-`Flag` variant). Such a member
     cannot consume any atom and so is always skipped.
   - M is atom-assignable and is **Flag-shaped** (a `Field` of resolved type `Flag`, or a
     `SelectRef` whose referenced SelectDefinition's variants all resolve to `Flag`), AND the
     text of A does not match any of M's keywords (the `Field`'s `keyword` for a Field, any
     variant's `keyword` for a SelectRef's referenced SelectDefinition). Skipping is permitted
     because A clearly is not meant to fill M; A may match a later member.

   A member with a `Scalar`-typed field is **never** skipped: a Scalar accepts any text, so
   there is no "doesn't match" condition that would justify skipping. Each advanced-past member
   is recorded as absent (subject to the required-member check in step 5).

   Worked example. Suppose `T.members = [Field(Flag a), Field(Flag b, required=false),
   Field(Scalar c)]` and `N.atoms = ["a", "xyz"]`:
   - Atom `a` at `pos=0`: M is Field(Flag a). Effective `required` is the default ("default" →
     true). No skipping. A matches `a`; assign; increment `pos` to 1.
   - Atom `xyz` at `pos=1`: M is Field(Flag b, required=false). Atom-assignable Flag-shaped,
     atom text doesn't match `b`. Skip M; `pos = 2`. Now M is Field(Scalar c); Scalar consumes
     any text. Assign `xyz` to `c`.

   b. If `pos` ≥ len(T.members), the document is invalid (**E302**: more atoms than assignable
   member positions).

   c. Let M = T.members[pos]. M MUST be atom-assignable; if it is not, the document is invalid
   (**E303**: atom in non-atom-assignable member position).

   d. Assign A to M:
   - If M is a `SelectRef`, the matched variant is the one (from the referenced
     SelectDefinition) whose keyword equals A's text; if no variant's keyword matches, the
     document is invalid (**E304**).
   - If M is a `Field` with `Flag` type, A's text MUST equal M.keyword; if it does not, the
     document is invalid (**E305**).
   - If M is a `Field` with `Scalar` type, the type of A is M.type regardless of A's text
     (validation against the named helper method is a separate step, described in §21).

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

   e. Apply this type-assignment procedure to C with type childType, dispatching per step 1.

5. **Constraint check.** For each member M in T.members:

   a. Let fill_count = (number of atoms assigned to M) + (number of compound children assigned to
   M).

   b. If M.`required` and fill_count = 0: if M is a `Field` whose type resolves to a `Scalar` and
   whose `default` is non-null, the default value is used as the semantic value and no error is
   raised; otherwise, the document is invalid (**E307**: required member absent and no default
   available).

   c. If not M.`repeatable` and fill_count > 1, the document is invalid (**E308**: non-repeatable
   member filled more than once).

### 20.3 Schema Layering

A `Schema` MAY include one or more `Layer` values in its `layers` list. Each layer describes an
incremental refinement of the schema. The refinements that a layer may perform correspond
exactly to the operations that produce a **subtype** of the base — additional information on
the record (product) side, and fewer alternatives on the select (sum) side. A composed schema
is a subtype of its base schema in the Liskov sense: every document valid under the composed
schema, projected back to the base schema's keywords, is valid under the base. §24 gives the
formal subtype relation `<:` and proves that every layer-permitted operation is
subtype-producing; this subsection states the permitted operations and the merge procedure
in prose.

#### Permitted Operations

A layer MAY apply the following operations to the base, each of which preserves subtyping. The
list is organised by the structural duality between records and selects.

**Record-side (product) operations:**

- **Add a Field** to a Struct (root Struct or RecordDefinition body). The new keyword MUST NOT
  collide with any keyword already present in the merged Struct (**E205**).
- **Add a SelectRef** to a Struct. The referenced SelectDefinition's variant keywords MUST NOT
  collide with any keyword already present in the merged Struct (**E205**).
- **Refine a Struct in place** (Field merge). When a layer declares a `Field` whose keyword
  matches an existing `Field` and the types are structurally equal (or both `Struct`, which
  are merged recursively), the layer's declaration MAY tighten the polarity on either axis by
  declaring `required` (tightens the `required` axis from `"loose"` to `"tight"`) and/or
  `irrepeatable` (tightens the `repeatable` axis from `"loose"` to `"tight"`). The merge also
  re-runs at any nested Struct level (additional Fields on the layer side are appended). A
  type mismatch is **E206**.
- **Refine a SelectRef in place.** When a layer declares a SelectRef whose `reference` matches
  an existing SelectRef in the same Struct, polarity is merged per `MergePolarity`; the
  `reference` itself does not change.
- **Refine a RecordDefinition in place** (RecordDefinition merge). When a layer declares a
  `record` whose name matches an existing `RecordDefinition`, the layer's members are merged
  into the existing Definition's members using the same algorithm as Struct merge.

**Select-side (sum) operations:**

- **Exclude a variant** from a SelectDefinition. Inside a layer's `select N` body, an
  `Exclude(K)` operation removes the variant with keyword K from the merged SelectDefinition.
  K MUST identify a variant of the base SelectDefinition (**E211**); the exclusion MUST leave
  at least one variant present if any composed-schema `SelectRef` whose effective `required` is
  `true` references the SelectDefinition (**E212**).
- **Refine a SelectDefinition in place** (SelectDefinition merge). When a layer declares a
  `select` whose name matches an existing `SelectDefinition`, the layer's `Exclude(K)`
  operations and `validate` lines apply against the base. Variant additions in this position
  are forbidden (**E213** — would widen the sum).

**Common-to-both operations:**

- **Add a Definition** (record, scalar, or select) to the composed namespace, when the name
  does not already exist in the merged namespace.
- **Refine a ScalarDefinition in place.** A same-name `scalar` in a layer appends validators to
  the base's `validators` list (in source order, deduplicated).
- **Append validators** to a Struct, RecordDefinition, or SelectDefinition. A layer's
  `validate K` lines at any such position are appended (in source order, deduplicated) to the
  base's `validators` list; validators apply in declaration order and AND-conjuncted (§21.1),
  so any layer-added validator runs *after* the base's validators.

The duality is: record-side adds members (extension; subtype-producing for products);
select-side excludes variants (narrowing; subtype-producing for sums). Both refine the
definition; the directions are mirror-image as type theory requires.

#### Forbidden Operations

The following operations break subtyping and are NOT permitted in a layer; an implementation
detecting any of them MUST report the corresponding error:

- Remove a Field from a Struct (no syntax — structurally prevented).
- Add a variant to an existing SelectDefinition (**E213** — would widen the sum). Introducing a
  *fresh* SelectDefinition with a fresh name is permitted; what is forbidden is a `variant K`
  appearing inside a layer's `select N` body when K is not already a base variant of N.
- **Loosen `required`** — declaring `optional` for an axis whose merged polarity is `"default"`
  or `"tight"` (**E214**).
- **Loosen `repeatable`** — declaring `repeatable` for an axis whose merged polarity is
  `"default"` or `"tight"` (**E215**).
- Remove a validator from a Struct, Scalar, RecordDefinition, or SelectDefinition (validators
  are append-only across layers; a layer's `validate K` adds K, never removes an existing K).
- Change a `Field`'s `default`, or a `Field`'s declared `Type` to a structurally different
  type that is not a Struct-to-Struct refinement.

#### Composed Schema Identity

A composed schema is identified by the base schema's `name` together with the ordered sequence of
layer `name`s applied to it. Two schemas with the same base `name` but different layer sequences
are distinct schemas. Layer order is part of identity even when reordered layers produce a
semantically equivalent composed Struct: the BinTEL signature (BinTEL §8) encodes the layer
order, so two compositions of the same set of layers in different orders have distinct
signatures.

#### Composed Definition Namespace

The composed schema's Definition namespace is built by walking, in order, the base schema's
`Schema.records`, `Schema.scalars`, and `Schema.selects`, then for each layer in layer order
its `Layer.records`, `Layer.scalars`, and `Layer.selects`. Definition merge (above) applies
whenever a later declaration shares a name with an earlier one. Records, scalars, and selects
share the namespace: a layer-introduced Definition whose name collides with an existing
Definition of a *different kind* is **E210** in the composed schema. After composition,
`Reference`s and `SelectRef`s in the base, in any layer, or in any merged member may resolve to
any Definition (of the appropriate kind) in the composed namespace.

#### Layer Body

A `Layer` value has `name`, `overlay`, `records`, `scalars`, and `selects` fields (see §20). In
TEL source, a layer's `overlay` is OPTIONAL — a layer that introduces or refines only
Definitions without modifying the document root MAY omit `overlay` entirely. When `overlay` is
absent it is treated as an empty `Struct` (no members).

#### Merge Algorithm

The function `MergeStruct(base: Struct, layer: Struct): Struct` produces a new Struct that
incorporates the layer's members into the base:

1. Begin with a copy of `base.members` in member order.
2. Construct the keyword map K for the base struct by iterating it in keyword order: for each
   entry `(keyword, type)` at member index i, map keyword → (i, members[i]).
3. For each member L declared by the layer at this Struct position, in source order:

   a. **Field members.** If L is a `Field` with keyword W:
      - Look up W in K.
      - **Found:** Let `(i, M) = K[W]`. M MUST be a `Field`, and M.type and L.type (after
        Reference resolution) MUST either be structurally equal or both be `Struct`; otherwise
        the layer is invalid (**E206**). The merged Field at index i has:
        - `keyword` = W (unchanged);
        - `type` = `MergeStruct(M.type, L.type)` when both are `Struct`, or the (structurally
          equal) base type otherwise — a non-`Struct` match is a polarity-only refinement;
        - per-axis polarities computed by `MergePolarity(M.required, L.required)` and
          `MergePolarity(M.repeatable, L.repeatable)`;
        - `default` = base's default (a layer may not change the default; see Forbidden
          Operations).
      - **Not found:** Append L as a new member at the end of the member list. Add W → (new
        index, L) to K. (Polarity merge does not apply: a freshly-introduced Field carries its
        declared polarity directly.)

   b. **SelectRef members.** If L is a `SelectRef` referencing `N`:
      - Resolve `N` in the composed Definition namespace to a SelectDefinition (E209/E217 if it
        cannot be resolved to a SelectDefinition).
      - For each variant keyword W of the referenced SelectDefinition, look up W in K. If any W
        matches an existing entry that is *not* a SelectRef referencing the same `N`, the
        layer is invalid (**E205**). If every matched W resolves to a SelectRef referencing
        the same `N`, the layer's SelectRef refines the existing one: per-axis polarities are
        merged via `MergePolarity`, and the merged SelectRef replaces the base member at that
        position.
      - Otherwise (no overlap at all), append L as a new member at the end of the member list.
        For each variant V of `N`, add V.keyword → (new index, L) to K.

   c. **Exclude in a Struct position.** An `exclude` operation MUST NOT appear inside a Struct
      body (root or RecordDefinition body) — only inside a layer's SelectDefinition body
      (**E216**).

4. Return the resulting member list as the merged Struct's `members`. The merged Struct's
   `validators` is the concatenation of the base's `validators` with any new validator names
   contributed by the layer at this Struct position (in source order, deduplicated). Validators
   are append-only across layers; a layer MUST NOT remove a base validator.

**`MergePolarity(base: Polarity, layer: Polarity): Polarity`** is defined per axis:

| base \ layer  | `"default"` | `"loose"`            | `"tight"`   |
| ------------- | ----------- | -------------------- | ----------- |
| `"default"`   | `"default"` | **E214 / E215**      | `"tight"`   |
| `"loose"`     | `"loose"`   | `"loose"` (redundant) | `"tight"`   |
| `"tight"`     | `"tight"`   | **E214 / E215**      | `"tight"`   |

The error code is E214 when the axis is `required`, E215 when it is `repeatable`. The rule
captures the subtyping direction: tightening (or restating) the cardinality is always allowed;
loosening an already-tight or default cardinality is rejected.

**`MergeRecord(base, layer): RecordDefinition`** applies `MergeStruct` to the Definitions'
member lists and merges `validators` by the append-and-deduplicate rule.

**`MergeScalar(base, layer): ScalarDefinition`** merges the `validators` lists by the
append-and-deduplicate rule.

**`MergeSelect(base, layer): SelectDefinition`** merges a layer's SelectDefinition body into the
base SelectDefinition:

1. Begin with a copy of `base.variants` in declaration order.
2. For each declaration L in the layer's SelectDefinition body, in source order:
   - If L is a `variant` declaration with keyword W: W MUST identify an existing variant in the
     current variant list; if not, the layer is invalid (**E213** — variant addition is
     forbidden). If W is present and the layer's `Variant.type` is structurally equal to the
     base variant's type, the operation is a no-op restatement (permitted); a type mismatch is
     **E206**.
   - If L is an exclusion `Exclude(W)` (an entry W in the layer SelectDefinition's `excludes`):
     W MUST identify an existing variant in the current variant list; if not, the layer is
     invalid (**E211**). Remove the variant from the list.
3. After all layer operations apply, the variant list MUST be non-empty *if* any
   composed-schema SelectRef whose effective `required` is `true` references this
   SelectDefinition; otherwise **E212**. (A SelectDefinition referenced only by non-required
   SelectRefs MAY be emptied; the referencing SelectRefs are then effectively unreachable in
   composed documents.)
4. The merged SelectDefinition's `validators` is the concatenation of the base's `validators`
   with any new validator names contributed by the layer's `validate` lines (in source order,
   deduplicated).

A layer attempting to merge a Definition of one kind onto a Definition of another kind (e.g. a
layer's `select N` onto a base `record N`) is **E210**.

#### Composing Layers

To apply a sequence of layers `[L₁, L₂, …, Lₙ]` to a base schema with root Struct R, record
list T (for `records`), scalar list S, and select list U:

1. Let `R₀ = R`, `T₀ = T`, `S₀ = S`, `U₀ = U`.
2. For each `Lₖ` in turn:
   - For each `record def` in `Lₖ.records`, in source order:
     - If `def.name` matches a name in `Tₖ₋₁`, replace that RecordDefinition with
       `MergeRecord(existing, def)`. If it matches a name in `Sₖ₋₁` or `Uₖ₋₁`, the schema is
       invalid (**E210**). Otherwise append `def` to `Tₖ`.
   - For each `scalar def` in `Lₖ.scalars`, in source order:
     - If `def.name` matches a name in `Sₖ₋₁`, replace that ScalarDefinition with
       `MergeScalar(existing, def)`. If it matches a name in `Tₖ₋₁` or `Uₖ₋₁`, the schema is
       invalid (**E210**). Otherwise append `def` to `Sₖ`.
   - For each `select def` in `Lₖ.selects`, in source order:
     - If `def.name` matches a name in `Uₖ₋₁`, replace that SelectDefinition with
       `MergeSelect(existing, def)`. If it matches a name in `Tₖ₋₁` or `Sₖ₋₁`, the schema is
       invalid (**E210**). Otherwise append `def` to `Uₖ`.
   - Set `Rₖ = MergeStruct(Rₖ₋₁, Lₖ.overlay)`.
3. The final `Rₙ` is the root Struct of the composed schema; `Tₙ`, `Sₙ`, `Uₙ` are its
   Definition lists.

#### Layer Validity Constraints

The schema validity constraints, including those checked at layer-composition time (**E204**,
**E205**, **E206**, and **E210**–**E217**), are catalogued in §20.1; the algorithms above define
where in composition each is detected.

### 20.4 BinTEL

The binary encoding of the semantic model, BinTEL, is defined in the companion
[BinTEL Specification](bintel.md). BinTEL provides deterministic serialization of typed TEL
documents and defines the schema signature and value hash constructions used for schema
identification (§8.1) and compatibility checking (§8.2).

When a BinTEL document is to be carried in a text-oriented context, it MAY be encoded as Unicode
text using [BASE-256](base256.md), defined as a companion specification. The BASE-256 textual
form is character-for-byte with the BinTEL byte sequence and is recovered losslessly by the BASE-256
decoder.

### 20.5 The tel-schema Schema

The concrete TEL representation of the schema model defined in §20 is itself a schema, identified
by `Schema.name = tel-schema`. The full document is supplied as the file
[`tel-schema.tel`](tel-schema.tel) at the root of this repository; this subsection specifies the
keyword vocabulary used by that document and states the self-describing closure property.

**Vocabulary.** The following keywords are used in a schema TEL document. The keywords are
themselves kebab-case identifiers (they appear in user-written TEL source); the `name` of each
top-level Definition (a `record`, `scalar`, or `select`) is a **PascalCase `TypeName`** (§20.7),
not a kebab-case identifier. The `name` of a `record`, `scalar`, `select`, or `layer`, and the
`name` of the top-level `Schema`, are typically written **implicitly** — i.e. as the first
inline atom of the parent compound (`record Field`, `scalar Email`, `select Status`,
`layer auth`, …) rather than as an explicit `name <value>` child — by the atom/compound
interchangeability rule of §19.1. The explicit `name <value>` child compound form is also
accepted; both forms produce the same `name` value in the `Schema`/`Layer`/Definition.
`tel-schema.tel` uses the implicit form throughout.

| TEL keyword     | §20 construct                                  |
| --------------- | ---------------------------------------------- |
| `name`          | `Schema.name`, `Layer.name`, or a Definition's `name` (parent context determines which). |
| `document`      | `Schema.document`                              |
| `layer`         | `Schema.layers[i]`                             |
| `sigil`         | `Schema.sigil`                                 |
| `record`        | A `RecordDefinition`: `Schema.records[i]` at schema root, or `Layer.records[i]` inside a `layer` compound. The first inline atom is the record's `TypeName`. |
| `scalar`        | A `ScalarDefinition`: `Schema.scalars[i]` at schema root, or `Layer.scalars[i]` inside a `layer`. First inline atom is the `TypeName`; the body is one or more `validate <name>` lines. |
| `select`        | At top level (or inside `layer`): a `SelectDefinition` — `Schema.selects[i]` or `Layer.selects[i]`. First inline atom is the `TypeName`; body is `variant`, `validate`, and (in layers only) `exclude` lines. **At a member position** (inside a `record` body, the `document` block, or a layer's `overlay` block): a `SelectRef` — the first inline atom is the `TypeName` of the referenced SelectDefinition; polarity is per use site. There is no inline-anonymous form; every select is named. |
| `overlay`       | `Layer.overlay` — the struct whose members are merged into the composed document root by the algorithm in §20.3. |
| `field`         | A `Field` member. Lives inside a `record` body, the `document` block, or a layer's `overlay` block. |
| `optional`      | Loosens to `required: "loose"` (Flag, base-side). |
| `required`      | Tightens to `required: "tight"` (Flag, layer-side override of `optional`). Permitted but redundant in a base, since the default is already tight. |
| `repeatable`    | Loosens to `repeatable: "loose"` (Flag, base-side). |
| `irrepeatable`  | Tightens to `repeatable: "tight"` (Flag, layer-side override of `repeatable`). Permitted but redundant in a base, since the default is already tight. |
| `keyword`       | `Field.keyword`, `Variant.keyword`. Carried as the first inline atom of a `field` or `variant` compound (or, less commonly, as an explicit `keyword <text>` child compound). Kebab-case. |
| `variant`       | A `Variant` of a `SelectDefinition`. First inline atom is the variant's kebab-case `keyword`; second inline atom is the `TypeName` of its `type`. |
| `type`          | The type-name field of a `Field`, a `Variant`, or a `SelectRef`. The value is a `TypeName` resolving (via §20.2 reference resolution) to either a user-declared Definition or a built-in type (`Flag`, `String`, `Identifier`, `Sigil`, `TypeName`). |
| `validate`      | Inside a `scalar` body, names a scalar validator. Inside a `record` body, a `select` body, the `document` block, or an `overlay`, names a struct-level (or select-level) validator (§21.6). The shared-namespace rule of §21.1 means the same name MAY be used in different contexts. |
| `default`       | `Field.default` — the value used when a required Scalar-typed field is absent from the document. Valid only on required Scalar-typed fields (E203 otherwise). |
| `description`    | The optional free-form `description` of the enclosing `Field`, `Variant`, `RecordDefinition`, `ScalarDefinition`, or `SelectDefinition`. A `String`-typed child compound (typically carrying a source atom, §14, for prose with spaces or multiple lines). Never validated; not permitted on a validator. |
| `exclude`       | A layer-only operation (§20.3) that excludes a variant from the merged SelectDefinition. Its inline atom is the kebab-case keyword K of the variant to exclude. Permitted only inside a `select` body within a `layer` compound (E216 otherwise). |

**Predefined type names.** The names `Flag`, `String`, `Identifier`, `Sigil`, and `TypeName`
are predefined (in `TypeName` form, i.e. PascalCase): `Flag` resolves to the built-in `Flag`
type, and the other four resolve to built-in `Scalar`s carrying the corresponding built-in
validators (§21.5). User schemas MAY NOT declare a Definition with any of these names
(collision is **E210**).

**Reserved keywords.** Only `tel` is universally reserved across all TEL documents (**E208**, §8).
The other keywords listed above are part of the tel-schema vocabulary and have meaning only when
a TEL document is being parsed as a *schema document* — i.e. when its schema is the tel-schema.
They do not constrain user-defined schemas: a user schema may freely define `Field.keyword` or
`Variant.keyword` values such as `name`, `document`, `layer`, `record`, `scalar`, `select`, etc.,
because the validity check applied to a user document is against the user schema's keyword set,
not against the schema-language vocabulary.

**Member ordering and inline syntax.** Member order in `Field` is `keyword`, `type` (both
required Scalars; `type` carries a `TypeName`), then the four loosen/tighten flags (`optional`,
`required`, `repeatable`, `irrepeatable`), then `default` (optional Scalar), then `description`
(a second optional Scalar which, being a non-first non-required Scalar, is always written as a
compound child per §20.8). The atom-phase
rules of §20.2 / §20.8 let a typical field declaration fit a single line: the first atom is the
keyword, the second atom is the type-name, each subsequent flag-matching atom toggles its flag,
and any remaining non-flag atom fills `default`. For example,

```tel
field country String unknown
```

declares a `country` field of type `String` (the built-in scalar) with default value `unknown`.
The field remains required on both axes — it may be elided from a document, in which case the
default supplies its value (a default is only permitted on a required member, per **E203**).
The convention is **flags before default**: `default` is the last optional Scalar and
so consumes the first non-flag atom after the type-name. Variants follow the same pattern but
carry only `keyword` and `type`, e.g. `variant active Flag`. There are no marker keywords:
position determines meaning.

A `SelectRef` at a member position has the inline shape `select <TypeName> [polarity]`:

```tel
select Status optional
```

introduces a non-required SelectRef referencing the `Status` SelectDefinition.

**References.** A `Field.type`, `Variant.type`, or `SelectRef.reference` resolves through the
composed namespace described above. If the name is `Flag`, `String`, `Identifier`, `Sigil`, or
`TypeName` it short-circuits to the built-in. Otherwise it must match a `record`, `scalar`, or `select`
declared somewhere in the composed schema (base or any layer); failing that, the schema is
invalid with **E209**, or with **E217** if the kind doesn't match the position (a `Field.type`
resolving to a `SelectDefinition`, or a `SelectRef.reference` resolving to a `RecordDefinition`
or `ScalarDefinition`). References may form cycles via `record` or `select` definitions in
their resolved bodies — the natural case for recursive data.

**Self-describing closure and bootstrap.** [`tel-schema.tel`](tel-schema.tel) MUST be a valid TEL
document when parsed under the schema it itself defines. To break the regress that would
otherwise prevent a schema document from being parsed at all, **every conforming TEL parser MUST
embed the `tel-schema` schema as a built-in**, available before any external schema has been
resolved. When a TEL document's pragma identifies `tel-schema` as its schema, the parser uses
the built-in form rather than performing schema retrieval. The built-in MUST produce the same
`Schema` model that would result from parsing the canonical `tel-schema.tel` under itself, and
MUST produce a byte-identical BinTEL encoding of that schema and the same 256-bit BLAKE3 value
hash (§3 of the BinTEL Specification). The hash is normative — two conforming implementations
MUST agree on it.

The pinned value, computed against the canonical
[`tel-schema.tel`](tel-schema.tel) in this repository, is:

| Form       | Value                                                                |
| ---------- | -------------------------------------------------------------------- |
| BLAKE3-256 | `d4289b0fc6b7f666c9269a135d509ff3973bcea734fbe777b8f907045d3df8a9`   |
| BASE-256   | `ÔШқďÆҷǶfωȦҚГѝPƟỳẗĻώƧ4ûçwᾸùćĄѝĽӸΩ`                                  |

The BinTEL document root encoding of `tel-schema.tel` is 1641 bytes; the raw bytes are recorded
in [`demo/tel-schema.bintel.hex`](demo/tel-schema.bintel.hex) and the hash in
[`demo/tel-schema.hash`](demo/tel-schema.hash). The same value is pinned in §3 of the BinTEL
Specification.

**Verifying the built-in.** An implementation's built-in `tel-schema` Schema value (the
"axiom") is a hand-written construction; it is easy to introduce silent drift between the
axiom and the canonical [`tel-schema.tel`](tel-schema.tel). Conforming implementations
SHOULD therefore include two self-consistency checks:

- **Structural-equality check.** Parse `tel-schema.tel` against the axiom and run
  `construct_schema` (§20.6) on the result; assert that the constructed `Schema` is
  structurally equal to the axiom (modulo the built-in scalars `Identifier`, `TypeName`,
  `Sigil`, `String`, which an implementation may inject into the axiom but which
  `construct_schema` will not produce from the document).
- **Value-hash check.** Encode the axiom (or, equivalently, the parsed-and-constructed
  document) as BinTEL and compute its 256-bit BLAKE3 digest; assert that the result equals the
  pinned value above. This is the content-addressed counterpart to the structural check.

Pinning both invariants in tandem makes axiom drift very hard to introduce undetected.

### 20.6 Schema Construction from the Semantic Model

Type assignment (§20.2) produces a tree of `Element` values typed by the tel-schema schema. To
obtain a `Schema` interface instance, an implementation traverses this tree and populates the
fields of the `Schema` model:

1. **Schema root.** Create a `Schema` value. Iterate the root element's children **in source order**
   (the order in which they appear in the document):
   - For the `name` child (Scalar with validator `identifier`), set `Schema.name` to its `text`.
   - For the `sigil` child, set `Schema.sigil` to its `text` (or leave `null` if absent).
   - For each `record` child, append a `RecordDefinition` to `Schema.records` constructed per
     step 2. The resulting list preserves source order.
   - For each `scalar` child, append a `ScalarDefinition` to `Schema.scalars` constructed per
     step 2b. The resulting list preserves source order.
   - For each `select` child (at schema root, i.e. **outside** any `record`/`document`/`overlay`
     body), append a `SelectDefinition` to `Schema.selects` constructed per step 2c. The
     resulting list preserves source order.
   - For the `document` child, set `Schema.document` to the `Struct` built from the `document`
     element's children per step 6. (In `tel-schema.tel` the `document` field is typed by the
     `Body` record — the shared struct shape also used by `overlay`; construction resolves that
     reference into the directly `Struct`-typed `Schema.document` of the data model.)
   - For each `layer` child, append a `Layer` to `Schema.layers` constructed per step 4. The
     resulting list preserves source order — layer composition (§20.3) applies layers in this
     order.
2. **`RecordDefinition` construction.** From a `record` element: take the `name` child (or first
   inline atom, which MUST be a `TypeName`) as `RecordDefinition.name`; build
   `RecordDefinition.members` and `RecordDefinition.validators` from the element's remaining
   children per step 6; set `RecordDefinition.description` from the optional `description` child's
   text, or `null` if absent.
2b. **`ScalarDefinition` construction.** From a `scalar` element: take the `name` child (or
   first inline atom, a `TypeName`) as `ScalarDefinition.name`; for each `validate` child within
   the element, append the child's inline-atom text to `ScalarDefinition.validators`, in source
   order; set `ScalarDefinition.description` from the optional `description` child's text, or
   `null` if absent.
2c. **`SelectDefinition` construction.** From a top-level `select` element: take the `name` child
   (or first inline atom, a `TypeName`) as `SelectDefinition.name`; for each `variant` child,
   append a `Variant` to `SelectDefinition.variants` per step 3; for each `validate` child,
   append the child's inline-atom text to `SelectDefinition.validators`; for each `exclude`
   child (permitted only inside a layer's `select` body — otherwise **E216**), append its
   inline-atom text (a variant keyword) to `SelectDefinition.excludes`, to be consumed by
   `MergeSelect` during layer composition; set `SelectDefinition.description` from the optional
   `description` child's text, or `null` if absent.
3. **Member construction.** A `field` element becomes a `Field`; a `select` element at a member
   position (inside a `record` body, `document`, or `overlay`) becomes a `SelectRef`; a
   `variant` element (inside a top-level `select`) becomes a `Variant`. The four loosen/tighten
   Flag children (`optional`, `required`, `repeatable`, `irrepeatable`), where applicable,
   collectively determine the two `Polarity` fields:
   - `required` =
     - `"tight"` if the `required` Flag is present;
     - else `"loose"` if the `optional` Flag is present;
     - else `"default"`.
   - `repeatable` =
     - `"tight"` if the `irrepeatable` Flag is present;
     - else `"loose"` if the `repeatable` Flag is present;
     - else `"default"`.

   The tightening flag (`required`, `irrepeatable`) wins when both flags on an axis are present
   — redundant declarations are benign no-ops.

   Within a `Field`:
   - `keyword` child → `Field.keyword` (kebab-case identifier).
   - The four loosen/tighten Flag children compute `Field.required` and `Field.repeatable`.
   - The `type` Scalar child or atom → `Field.type` as a `Reference(TypeName)` (per step 5).
   - The optional `default` Scalar child or atom → `Field.default` (a string), or `null` if
     absent.
   - The optional `description` Scalar child → `Field.description` (a string), or `null` if
     absent.

   Within a `SelectRef`:
   - The four loosen/tighten Flag children compute `SelectRef.required` and
     `SelectRef.repeatable`.
   - The first inline atom (or the `name` / `type` child compound; the schema-of-schemas uses
     the first inline atom) is a `TypeName` and becomes `SelectRef.reference`.

   Within a `Variant`:
   - `keyword` child or first inline atom → `Variant.keyword` (kebab-case).
   - `type` child or second inline atom → `Variant.type` as a `Reference(TypeName)`.
   - The optional `description` Scalar child → `Variant.description` (a string), or `null` if
     absent.
4. **`Layer` construction.** From a `layer` element: take the `name` child as `Layer.name`;
   build `Layer.overlay` from the `overlay` element's children per step 6 (treating an absent
   `overlay` as an empty Struct); construct `Layer.records` from each `record` child within
   the layer (step 2); `Layer.scalars` from each `scalar` child (step 2b); `Layer.selects` from
   each `select` child at the layer's top level (step 2c, which inside a `layer` body permits
   `exclude` children).
5. **`Type` construction.** Every `Field.type` and `Variant.type` is a `Reference` whose `name`
   is the inline-atom (or `type` child-compound) text, a `TypeName`. Resolution of a `Reference`
   happens at type assignment time (§20.2) and selects either a `RecordDefinition`'s `Struct`,
   a `ScalarDefinition`'s `Scalar`, or one of the five built-in types (`Flag`, `String`,
   `Identifier`, `Sigil`, `TypeName`). A `Reference` resolving to a `SelectDefinition` is
   **E217**.
6. **`Struct` construction.** Given the children of a Struct-shaped compound (the `document`
   element, a `layer` element's `overlay`, or a `record` element's body), produce a `Struct`
   (or, for `record`, a `RecordDefinition` whose members and validators are taken from this
   step):
   - Each `field` child contributes one `Member::Field` constructed per step 3.
   - Each `select` child at this position contributes one `Member::SelectRef` constructed per
     step 3.
   - Each `validate` child contributes its inline-atom text to the resulting `validators` list,
     in source order.
   - An `exclude` child is **not** permitted in this position (**E216**); `exclude` lives only
     inside a layer's `select` body, where it is consumed by `MergeSelect` (§20.3).

Schema construction MUST be deterministic: two implementations applied to the same input MUST
produce identical `Schema` values. After construction, the resulting schema is checked against
the validity constraints of §20.1 and §20.3; any failure is reported as the corresponding
**E2xx** error.

### 20.7 Identifier Grammars

This specification uses two ASCII identifier kinds, distinguished by initial-letter case.

**Kebab-case identifier.** Used for non-type names: schema names, layer names, field keywords,
variant keywords, and validator names.

```
identifier ::= prime* lower-letter ( '-'? lower-letter-or-digit )*
prime ::= "'"
lower-letter ::= [a-z]
lower-letter-or-digit ::= [a-z] | [0-9]
```

That is:

- An identifier MAY begin with one or more prime (`'`) characters.
- After any leading primes, it MUST continue with a lowercase ASCII letter.
- It MAY then contain lowercase letters, digits, and hyphens.
- Hyphens MUST NOT appear consecutively (no `--`), and MUST NOT appear at the start or end of
  the identifier.
- The empty string, and a string consisting only of primes, are not valid identifiers.

This grammar is enforced by the built-in `Identifier` validator (§21.5).

**PascalCase `TypeName`.** Used for the `name` of every Definition (record, scalar, select) and
for every `Reference` / `SelectRef` target.

```
type-name        ::= upper-letter ( letter-or-digit )*
upper-letter     ::= [A-Z]
letter-or-digit  ::= [A-Z] | [a-z] | [0-9]
```

That is:

- A `TypeName` MUST begin with an uppercase ASCII letter.
- It MAY contain uppercase letters, lowercase letters, and digits.
- It MUST NOT contain hyphens, underscores, or non-ASCII characters.
- The empty string is not a valid `TypeName`.

This grammar is enforced by the built-in `TypeName` validator (§21.5). The two grammars are
disjoint: every well-formed identifier begins with a lowercase letter, every well-formed
TypeName begins with an uppercase letter, so a single atom's lexical form tells the reader
which kind it is.

### 20.8 Inline-Atom Position Constraints

The atom phase in §20.2 allows skipping past non-required `Flag`-shaped members when an atom does
not match their keyword. It does **not** allow skipping past non-required `Scalar` members,
because a `Scalar` accepts any atom text — there is no "doesn't match" condition that would justify
a skip. As a consequence, when a struct's atom-assignable members include two or more non-required
`Scalar` `Field`s, only their prefix can be filled positionally with inline atoms; later optional
Scalars in the same struct must be filled via explicit compound children. Schema authors should
either order their members so this limitation does not arise, or expect users to write the later
optional Scalars as child compounds (e.g. `field-name value`).

## 21. Validation

Type assignment (§20.2) ascribes a `Type` to every element. For `Flag` types, the structure of the
document is sufficient to determine validity. For `Scalar` and `Struct` types, the value or
structure MAY additionally be checked against one or more named **validators** declared in the
schema.

### 21.1 Validators

A **validator** is a named helper method that examines a value and returns `Valid` or
`Invalid`. A validator can be applied to a `Scalar` value (in which case it inspects the atom's
text), a `Struct` element (in which case it inspects the struct's children), or a
`SelectDefinition` instance (in which case it inspects the chosen variant). Struct and
SelectDefinition requests share the `kind = "struct"` shape (§21.2) — they are the same wire
form, distinguished by the parent type at the call site.

- A `Scalar` member's `validators` list (§20) names the validators applied to that scalar's
  value.
- A `Struct`'s `validators` list (§20) names the validators applied to that struct.
- A `SelectDefinition`'s `validators` list (§20) names the validators applied to instances of
  that sum (whichever variant is chosen).

When a value or struct has more than one validator, they apply in **AND-conjunction**: every
validator MUST return `Valid` for the value to be accepted. The validators are invoked in the
order they are listed; an implementation MAY short-circuit on the first `Invalid` response or
invoke every validator regardless (the document is invalid in either case if any returns
`Invalid`).

Validators live in a **single shared namespace**: a name like `non-empty` refers to one
helper, which is dispatched on the request kind (scalar or struct) at invocation time. A
helper that supports only one kind returns `Invalid` with an appropriate message when invoked
on the other.

This specification mandates only the four built-in validators required by `tel-schema` itself
(§21.5). Every other validator is application-defined; a parser is configured with a callback
(§21.4) that resolves each validator name to a concrete check.

As illustrations:

- A scalar validator named `ipv4` accepts dotted-quad IPv4 address literals. A schema field
  whose scalar `validators` list contains `ipv4` will be invoked once per atom text encountered
  for that field; an invalid octet produces an `Invalid` response with a `Diagnostic::Scalar`
  pointing into the offending part of the text.
- A struct validator named `postcode-required-when-uk` examines an address struct's children
  and rejects the struct if `country == "UK"` but `postcode` is absent. It produces a
  `Diagnostic::Struct` whose `fields` map points at the missing `postcode`.

### 21.2 Request and Response

The request and response of a helper method invocation are defined by the following types:

```typescript
type ValidationRequest =
  | { method: string; kind: "scalar"; value: string }
  | { method: string; kind: "struct"; element: StructElement };

type ValidationResponse = Valid | Invalid;

interface Valid {}

interface Invalid {
  diagnostic: Diagnostic;
}

type Diagnostic =
  | { kind: "scalar"; message: string; span?: { start: number; end: number } }
  | { kind: "struct"; message: string; fields?: { [keyword: string]: Diagnostic } };
```

`StructElement` is the semantic-model element (§18.2) being validated — its keyword index, its
schema type (after Reference resolution), and its child elements. The validator MAY traverse
its children to inspect values, sub-structs, and flag presence.

**Kind matching.** A helper method MUST respect the request's kind: a `{ kind: "scalar" }`
request returns `Valid` or `Invalid` whose `diagnostic.kind == "scalar"`; a `{ kind:
"struct" }` request returns `Valid` or `Invalid` whose `diagnostic.kind == "struct"`. Mismatched
kinds are a contract violation.

**Diagnostic shape.**

- `Diagnostic::Scalar` carries a `message` and an OPTIONAL `span`. When present, `span` is a
  half-open range `[start, end)` of zero-based code-point indices into the value's input
  text. `start` and `end` MUST both be present or both absent — partial spans are forbidden.
- `Diagnostic::Struct` carries a `message` and an OPTIONAL `fields` map keyed by child keyword
  (Field keyword or Select variant keyword). The nested `Diagnostic` for each entry MUST
  match the schema-declared type of that child: a `Diagnostic::Scalar` for a Scalar child, a
  `Diagnostic::Struct` for a Struct child, or a `Diagnostic::Scalar` with no `span` for a Flag
  child. Spans never appear in `Diagnostic::Struct`; structures address content by keyword
  path, not by offset.

**Top-level message.** An `Invalid` response carries exactly one top-level `Diagnostic`, with a
human-readable `message` describing the failure as a whole. Per-field detail is conveyed via
the recursive `fields` map (for struct diagnostics) or via the optional `span` (for scalar
diagnostics).

### 21.3 Reporting and Span Resolution

When a helper method returns `Invalid`, the implementation reports **E310** with the returned
`Diagnostic` translated to document-level offsets:

- For a `Diagnostic::Scalar` with a `span`, the implementation MUST translate the value-relative
  offsets to document offsets by adding the document offset of the start of the value's input
  text. If the value is the body of a compound's inline atom, the offset is the atom's
  beginning in the document; for source and literal atoms, the offset is the first content
  code point of the atom's payload after the delimiter handling defined in §14–§15.
- For a `Diagnostic::Scalar` with no `span`, the implementation reports the error against the
  span of the enclosing scalar value's text in the document.
- For a `Diagnostic::Struct`, the implementation reports the error against the keyword span of
  the enclosing struct compound. If the `fields` map is non-empty, the implementation SHOULD
  recursively descend into each entry: a nested `Diagnostic::Scalar` is reported against the
  keyword'd child compound's value text (with the optional span applied as above); a nested
  `Diagnostic::Struct` is reported against the child compound's keyword, and recursed into.

An implementation rendering diagnostics to a user SHOULD present the top-level `message` as
the primary headline and the recursive structure as collapsible detail; the choice of
presentation is not normative.

**Malformed validator responses.** A helper method MUST return a `Diagnostic` whose `kind`
matches the request's `kind`: a Scalar request gets a `Diagnostic::Scalar`; a Struct request
gets a `Diagnostic::Struct`. A diagnostic MUST also satisfy the structural rules of §21.2 —
spans are forbidden in `Diagnostic::Struct`; each entry in a `fields` map carries a
`Diagnostic` whose kind matches the schema-declared type of the keyed child; spans on
`Diagnostic::Scalar` MUST be valid half-open code-point ranges into the value's text (i.e.,
`0 ≤ start ≤ end ≤ codepoint_count(value)`).

When an implementation detects that a returned diagnostic violates any of these rules, it
MUST treat the response as a **contract violation**:

1. Discard the malformed diagnostic.
2. Substitute a synthetic `Diagnostic::Scalar { message: "validator <method> returned a
   malformed diagnostic", span: None }` (for Scalar requests) or a synthetic
   `Diagnostic::Struct { message: "validator <method> returned a malformed diagnostic",
   fields: {} }` (for Struct requests).
3. Report E310 against the value or struct as if the validator had returned the synthetic
   diagnostic.

This rule lets a parser cope with buggy validator callbacks without aborting validation of
the rest of the document. Implementations MAY additionally log the contract violation for
debugging.

### 21.4 Helper Method Binding

A TEL parser that wishes to enable validation MUST be provided with a **callback function**
that conforms to the helper method interface: given a `ValidationRequest`, it returns a
`ValidationResponse`. The parser invokes this callback for each scalar value and each struct
element whose schema declares one or more validators. How the callback is supplied is
determined by the host language or environment (a function parameter, a trait implementation,
an interface injection).

This specification does not prescribe a wire protocol, service discovery mechanism, or
serialization format for helper method invocation:

- A parser embedded in an application MAY implement the callback directly in the host
  language.
- An IDE, text editor, or LSP server MAY delegate helper method calls to an external service
  (REST, RPC, subprocess), but the mechanism by which the editor discovers and configures
  such a service is outside the scope of this specification.

If no callback is provided, the parser MUST skip validation entirely (no E310 errors are
raised). All other parsing and validation proceeds normally.

If the callback does not recognise a validator name, it MUST return `Invalid` with a
diagnostic identifying the unknown name; the affected value or struct then fails validation
(**E310**). An unknown validator MUST NOT be silently treated as `Valid`: a schema that names
a validator expresses a constraint, and a consumer that cannot check the constraint cannot
claim the document satisfies it.

### 21.5 Built-in Validators

This specification does not mandate a portable validator library — applications choose which
validators they implement. However, four validators — `string`, `identifier`, `sigil`, and
`type-name` — are referenced by the `tel-schema` schema itself, via the built-in `TypeName`s
`String`, `Identifier`, `Sigil`, and `TypeName` (§20.5; the fifth built-in, `Flag`, is a Type
and carries no validator). These four MUST be implemented by any TEL parser that wishes to
parse schema documents at all. All are **scalar** validators (kind = `"scalar"`); they return
`Invalid` with `Diagnostic::Scalar` on failure.

The validator names defined by this specification are kebab-case (per §20.7 / §21.1's shared
namespace), even where they share a base lexeme with a capitalized `TypeName`. The
correspondence is:

| Built-in `TypeName` | Validator name (`identifier` kind) | Meaning at a use site |
| ------------------- | ---------------------------------- | --------------------- |
| `String`            | `string`                           | Unconstrained scalar value |
| `Identifier`        | `identifier`                       | Kebab-case identifier (§20.7) |
| `Sigil`             | `sigil`                            | Single sigil character |
| `TypeName`          | `type-name`                        | PascalCase type name (§20.7) |
| `Flag`              | n/a (Flag is a Type, not a Scalar) | Flag-typed member     |

`type-name` validates `TypeName` atoms (Definition names and `Reference` / `SelectRef`
targets). Schemas MAY use `type-name` directly to validate any user-defined scalar that must
carry a type name.

**`identifier`.** Accepts a string that conforms to the kebab-case identifier grammar of §20.7
(including any leading primes). On failure, returns a `Diagnostic::Scalar`
whose `message` describes the first violation encountered (e.g. "leading hyphen", "consecutive
hyphens", "empty identifier", "non-ASCII or uppercase character") and whose `span` covers the
offending portion of the input (`[0, len)` if the input is malformed end-to-end).

**`type-name`.** Accepts a string that conforms to the PascalCase `TypeName` grammar of §20.7.
On failure, returns a `Diagnostic::Scalar` whose `message` describes the first violation (e.g.
"leading lowercase letter", "hyphen in TypeName", "empty TypeName", "non-ASCII character") and
whose `span` covers the offending portion of the input.

**`sigil`.** Accepts a single-character string whose character is sigil-valid (§6). On failure,
returns a `Diagnostic::Scalar` covering the offending input.

**`string`.** Accepts any input string without further constraint; equivalent to "no
validation". The `string` validator MUST always return `Valid`. It exists so a schema can
declare a field whose validator is the unconstrained string type without the application
needing to define a custom validator.

Implementations MAY provide additional validators beyond these. The four built-in validators
are the minimum required for `tel-schema` parsing to function. None of the four supports the
`struct` kind;
invoked on a struct request, they return `Invalid` with
`Diagnostic::Struct { message: "validator not applicable to struct values", fields: {} }`.

### 21.6 Struct and Select Validators

A `Struct` (whether it is the root, a `RecordDefinition`'s body, or an `overlay`'s body) MAY
carry a `validators` list (§20). A `SelectDefinition` MAY likewise carry a `validators` list,
for cross-cutting checks over which variant has been chosen. When the struct or select appears
in a document, validation invokes each named validator AFTER all of the element's children
have themselves been validated. This sequencing means a struct/select validator can rely on
its children having well-typed values and on any per-child validators having already passed;
if any child validator returned `Invalid`, the struct/select validator MAY still be invoked
(the implementation chooses — see §21.4).

Struct validators are particularly useful for **cross-field constraints**: rules that span
multiple members of a struct, such as "postcode required when country is UK", "start date
must precede end date", or "at most one of A, B, C is present". Such constraints cannot be
expressed by scalar validators alone, because each scalar validator sees only its own value.

Select validators inspect the chosen variant: rules such as "this variant is only valid when
the surrounding context X is true" or "variant K requires capability Y". A select validator
sees only one variant per invocation (the one the document chose); its `Diagnostic::Struct`'s
`fields` map (if present) is keyed by variant keywords, addressing the chosen variant's
content.

A `Diagnostic::Struct` returned by a struct or select validator MUST satisfy the structural
rules of §21.2: spans are forbidden at the struct level; per-field detail is conveyed via the
recursive `fields` map. The implementation translates the diagnostic per §21.3.

The error code raised for a failing struct or select validator is the same **E310** used for
scalar validators (one code, three shapes of diagnostic) — the `kind` of the diagnostic and
the parent type distinguish the cases.

#### Validation Errors (E3xx)

| Code | Description                                                                                           | Span                                                                                                     |
| ---- | ----------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| E301 | A compound has compound children but its type is not a `Struct`                                       | The compound's keyword                                                                                   |
| E302 | More atoms on a compound than there are assignable member positions (for a `Scalar`-typed compound: more than one atom) | The first excess atom                                                                                    |
| E303 | Atom appears at a member position that is not atom-assignable                                         | The atom                                                                                                 |
| E304 | Atom text matches no variant keyword of the SelectDefinition referenced by a `SelectRef` member        | The atom                                                                                                 |
| E305 | Atom text does not match a `Field` member's `Flag` keyword                                            | The atom                                                                                                 |
| E306 | Compound keyword is not recognized for its parent type                                                | The compound's keyword                                                                                   |
| E307 | Required member absent, and member is not a `Field` with a `Scalar` type with non-null `default`      | Zero-width span at the end of the parent compound's last child (or at the parent keyword if no children) |
| E308 | Non-repeatable member is filled more than once                                                        | The keyword of the second occurrence                                                                     |
| E309 | Compound children of the same member are not contiguous                                               | The keyword of the non-contiguous child (the second group's first child)                                 |
| E310 | A scalar value or struct element failed validation by a named validator                               | As resolved by §21.3 from the returned `Diagnostic`                                                       |
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

When a machine relocates a compound — via a `reorder-*` operation (§22.2), or by a `delete`
followed by re-insertion of the same compound — its containing block's structure is preserved
where possible: if the relocation leaves the block with no remaining compounds, the block (and
any attached comments) moves with the compound to the new location.

When a machine inserts a compound constructed from purely semantic information, no comment is
attached to it initially; it is placed into an existing block or a new empty block as appropriate.

### 22.2 Machine Operations

A machine MUST perform only operations drawn from the following set. Each operation preserves all
presentation-layer details that are not directly affected by the operation: remarks,
`trailingBlankLines` counts, `precedingSpaces` on inline atoms, and tabulation marker offsets are
all retained unless the operation explicitly targets them.

**Operation addressing.** Operations identify their target by a **path**. Two path kinds exist:

- A **compound path** is a sequence of `(block_index, compound_index)` pairs descending from the
  document root to a specific `Compound`. Used by `update-value`, `attach-remark`,
  `remove-remark`, `delete`, `replace`, `set-flag`, `unset-flag`, `insert`, `insert-before`,
  `insert-after`, `reorder-within-group`, and `reorder-groups`. An empty compound path refers to
  the (virtual) document root.
- A **block path** is a compound path of length 0 or more (locating the parent compound, or the
  document root when empty) plus a `block_index` selecting one of that parent's child blocks.
  Used by `insert-into-block` and `resize-tabulation`, which target an entire block rather than
  a compound within it.
- An **atom path** is a compound path (locating the owning compound) plus a keyword index
  (§20) and an occurrence index, numbered from 0 across that member's fillings in semantic
  order (atoms before compound children, per §18.3). It addresses a `Scalar` or `Flag` element
  realized as an inline atom on the owning compound's line. Used by `update-value` when its
  target is atom-realized rather than a compound of its own.

`construct` is a constructor rather than a tree-mutation operation: it produces a fresh compound
from semantic data, with no path argument; the caller subsequently uses `insert`, `insert-before`,
`insert-after`, or `insert-into-block` to place the result. `reorder-groups` takes a compound
path identifying the parent struct plus two member indices: the group to move, and the group
before or after which it is placed.

Operations that move or remove a compound MUST update any in-flight paths that referenced that
compound's position; the caller is responsible for invalidating cached paths after a mutation.

**Operation failure and no-ops.** Several operations state preconditions: a `delete` target
must not fill a `required` member, an `update-value`'s new string must validate, a `replace`
must satisfy its validity conditions, and `set-flag`/`unset-flag` must respect the member's
`repeatable`/`required` constraints. An operation whose precondition does not hold is
**rejected**: it MUST NOT be applied, both models are left unchanged, and the implementation
reports the diagnostic (E3xx, or a structural error such as E306/E309) that the resulting
document would have exhibited. An operation that would produce a state identical to the
current one — an `update-value` writing the identical string, an `attach-remark` with the
identical remark, a `reorder-*` to the current position — is applied as the identity: it
succeeds and changes nothing.

**Atom-form safety predicates.** Three predicates determine whether a `Scalar` value can be carried
*faithfully* — i.e. parsed back to the identical string — in each atom form. They are used by every
operation that writes a scalar value (this section) and by canonical serialization (§22.3).

- A value is **inline-safe** if all of the following hold:
  - it contains no `LF` character;
  - it contains no run of two or more consecutive `U+0020` SPACE characters (no hard space embedded
    in the value);
  - it does not begin or end with `U+0020` SPACE;
  - it does not begin with the document's sigil character immediately followed by `U+0020` SPACE.
    (The value is emitted at the start of a phrase, so a leading sigil-then-soft-space would be
    parsed as the start of a remark, §11.2. An *internal* space-then-sigil is safe: a value that
    contains a space is emitted with a two-space separator, which puts its phrase into hard-space
    mode where soft spaces are content, so the sigil is not at a phrase boundary — §10.3, §11.2.)
- A value is **source-safe** if all of the following hold:
  - the value is non-empty and contains no empty line — equivalently, it does not begin or end with
    `LF` and contains no run of two or more consecutive `LF` characters. A source atom carries one
    indented line per `LF`-separated segment and cannot represent an empty line: a blank line
    neither begins nor continues a source atom, and trailing blank lines are dropped (§14). In
    particular, a value with a trailing `LF` is **not** source-safe and requires a literal atom;
  - no line of the value ends with a character having the Unicode `White_Space` property
    (source atoms strip trailing whitespace, §14);
  - the value's first line does not begin with `U+0020` SPACE (a source atom strips the indentation
    of its first line from every captured line, §14, so a leading space on the first line could not
    be recovered).
- The **effective delimiter line** of a literal atom is the line comprising the margin, the
  opening indentation at the position where the atom is written, and the delimiter D (§15). A
  value is **literal-safe with respect to a delimiter line L** if L does not appear as a line
  within the value. Because the effective delimiter line depends on the atom's position,
  literal-safety does too; checking that D does not appear as the trailing content of any line
  of the value is a position-independent sufficient condition.

Every value is literal-safe with respect to *some* delimiter line (a colliding delimiter can
always be lengthened until its delimiter line no longer appears as a line, §22.3), so the literal
form is a universal fallback that every value can use.

**Atom-form safety invariant.** Whenever an operation writes a `Scalar` value into an atom, it MUST
emit the value in an atom form for which the value satisfies the corresponding safety predicate
above: an inline atom only if the value is inline-safe, a source atom only if it is source-safe, and
a literal atom only with a delimiter whose delimiter line makes the value literal-safe. The
*Literal atom delimiter invariant* below is the literal-safe case of this general rule.

**Sigil invariant.** A machine MUST NOT change the document's sigil. The sigil in effect when the
document was parsed is preserved exactly in any reserialized output.

**Literal atom delimiter invariant.** The effective delimiter line of a literal atom MUST NOT
appear as a line within the atom's payload. When a machine updates the value of a literal atom,
it MUST check whether the atom's effective delimiter line appears verbatim as a line in the new
payload. If it does, the editor MUST choose a new delimiter whose effective delimiter line does
not appear as a line in the new payload before writing the updated atom.

This specification does not mandate a delimiter-selection algorithm for editor use. Two common
strategies are:

- **Dash-extension.** Start from a short fixed delimiter such as `---`; if its delimiter line
  appears as a line in the payload, lengthen by one `-` (try `----`, `-----`, …) until no
  collision remains. This is the strategy used by canonical serialization (§22.3) and is
  RECOMMENDED for any tool that values diff-stability.
- **High-entropy token.** Generate a random or UUID-derived ASCII identifier and use it as the
  delimiter (after verifying the no-collision invariant). This single-pass option suits writers
  that have no need for short, human-readable delimiters.

Any other deterministic strategy that respects the no-collision invariant is also conforming.
The choice is an application concern; only canonical serialization (§22.3) is normatively bound
to the dash-extension algorithm.

**`delete`** — Remove a compound that is not `required`. Any remark attached to the compound is
removed with it. If the compound's block becomes empty (no remaining compounds), the block and its
attached comments are also removed.

**`replace`** — Substitute a compound for another at the same position in the same block.
A replacement is valid if and only if:

- the replacement compound's keyword identifies the same schema member as the original — that
  is, either both keywords map (via the parent's keyword map K) to the same `Field` member, or
  both map to (possibly different) `Variant`s of the same `Select` member; and
- the replacement compound is itself well-typed under the type that K maps it to (the new
  keyword's type after Reference resolution); in particular, a `Scalar`-typed replacement MUST
  have its value validate against the target Scalar's helper method (§21).

The replacement retains the original compound's remark and its position within the block.
Attached comments on the block are preserved. If the replacement targets a `Select` member and
uses a different variant from the original, the keyword in the presentation layer is updated
accordingly and the body of the new compound MUST match the new variant's type.

**`construct`** — Create a new compound from purely semantic information, with no presentation-layer
context. The constructed compound carries no remark and has no attached comments. No blank lines
appear between its children. No tabulation is added. The canonical presentation form is determined
by iterating the struct's members in member order:

1. Starting from the first member, each non-repeatable `Field` member whose type is `Scalar` is
   serialized as an inline atom, in member order, for as long as consecutive members satisfy this
   condition and the value can be represented as an inline atom (see the atom-form escalation
   algorithm in §22.3).
2. If the next member after the initial run of non-repeatable scalars is an all-`Flag` `Select`,
   each present flag is serialized as an inline atom.
3. Otherwise, if the next member is a `repeatable` `Field` whose type is `Scalar`, each
   occurrence is serialized as an inline atom — unless any occurrence is not inline-safe, in
   which case **every** occurrence of that member is serialized as a compound child. (Mixing
   the two forms would reorder occurrences: atoms precede compound children in semantic order,
   per §18.3, so serializing occurrences 1 and 3 inline but occurrence 2 as a child would
   permute the sequence.)
4. All remaining children — including any `Field` members whose type is `Struct`, mixed-variant
   `Select` members, and any members beyond the first repeatable scalar — are serialized as compound
   children with explicit keywords.

**Atom-form escalation.** When serializing a `Scalar` value to TEL source, the atom form is
selected by atom-form escalation, the deterministic algorithm defined in §22.3. `construct`
MUST apply that algorithm exactly, so that two implementations producing the same compound
from the same semantic value emit byte-identical text: the algorithm picks the **first** form
among inline → source → literal that the value can faithfully carry, so the choice is a
function of the value's text alone.

If a value cannot be an inline atom, the Scalar is serialized as a compound child with an
explicit keyword and the appropriate atom body, rather than as an inline atom on the parent
line.

Each inline atom whose value contains a `U+0020` SPACE uses two preceding spaces (a hard-space run,
`precedingSpaces = 2`), which switches the parser into hard-space mode (§10.3) so the value's soft
spaces are preserved as content rather than splitting the value into separate atoms; an inline atom
whose value contains no space uses a single preceding space (`precedingSpaces = 1`). Each compound
child is indented by one level (two spaces) relative to its parent.

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
presentation details of the compound are retained — except that the atom **form** is subject to the
*Atom-form safety invariant*: the existing form is kept only while the new value remains safe for
it. If the new value is not safe for the current form, the operation MUST re-select a form by
escalating along inline → source → literal: it keeps the current form when the value still satisfies
that form's safety predicate, and otherwise advances to the first later form whose predicate the
value satisfies (choosing a delimiter per the *Literal atom delimiter invariant* when escalating to
a literal atom). Escalation never moves to an earlier form: an `update-value` whose new value would
be inline-safe but whose atom is currently a literal atom leaves it a literal atom. This best-effort
preservation of the current form is the one intended divergence from atom-form escalation
(§22.3), which always picks the first safe form.

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
current column values and any values about to be added. The new offsets are computed by the
**minimal-offsets algorithm** below; this is normative — two conforming implementations applied
to the same block produce identical `markerOffsets`. After resizing, all existing row content
MUST be re-padded with spaces to align to the new column positions. The `headings` list is
updated in parallel with `markerOffsets`: existing headings are preserved in place and re-padded
within their updated column spans; no heading text is added or removed by this operation.

**Minimal-offsets algorithm.** Given a tabulated block whose tabulation declares markers
M_0 … M_n (so `markerOffsets` has `n + 1` entries and `markerOffsets[i]` is the offset of M_i,
§16.1):

1. For each `i` in `0 … n`, compute `w_i = max(v_i, h_i + 2)`, where `v_i` is the maximum
   code-point width of any value that will appear in column `i` after the operation completes
   (taking the maximum across every existing row plus every planned new row; for `i = 0`, the
   width of each row's keyword-and-pre-column-atom portion) and `h_i` is the code-point width
   of the heading for M_i. The `+ 2` reflects that a heading begins two positions after its
   marker, at M_i + 2 (§16.1), whereas a row value begins at M_i (§16.2).
2. `markerOffsets[0]` is the tabulation line's indentation offset after removing the document
   margin: twice its indent (§9); `0` only for a block at indent zero.
3. For each `i` in `1 … n`, `markerOffsets[i] = markerOffsets[i−1] + w_{i−1} + 2`. The `+ 2`
   is the mandatory hard-space separator; this is the smallest offset satisfying the width
   rule of §16.1 (`M_{i+1} − M_i − 2`).
4. The result is the unique smallest sequence of offsets that fits every value and heading
   without violating the hard-space minimum-gap rule of §16.1.

This procedure is deterministic and produces identical offsets across implementations.

### 22.3 Canonical Document Serialization

A **canonical serialization** of a semantic model produces a single, deterministic TEL text
representation. Canonical serialization is defined only for documents that carry a schema — i.e.
those whose semantic model exists. An untyped document (§8.2) has no semantic model and
therefore no canonical form; its presentation model is its only stable representation.

Canonical serialization follows the same conventions as the `construct` operation
(§22.2) for individual compounds, extended to the entire document:

- The document margin is zero.
- No interpreter directive is included.
- A pragma line is included, specifying the TEL version of the serializer and the schema identifier.
  The sigil is not specified in the pragma (the default `#` is used).
- When the schema is identified by both a URL and a signature (as a URL with a fragment, per §8.1),
  canonical serialization emits the **bare BASE-256-encoded signature** alone — the URL component is
  omitted. The signature is content-addressed and stable across resolver changes; a URL is a
  presentation-layer convenience that does not affect the semantic model. When the schema is
  identified only by URL (no signature was available at serialization time), the URL is emitted
  verbatim. When the schema was supplied only by invocation (§8.2) and carries neither URL nor
  signature, the serializer MUST compute the schema's signature (§8.1) and emit it as a bare
  BASE-256 signature; the canonical form always carries a schema identifier.
- No comments or remarks are included anywhere in the document.
- No tabulation lines are included; all compounds are serialized as ordinary (non-tabulated) lines.
- No blank lines appear between children at any level.
- The root node has no inline atoms (the document root is a virtual struct with no atom positions),
  so every root-level member is serialized as a compound child.
- At every non-root level, **atom-form escalation** — the algorithm defined by this rule and
  cited by that name throughout this specification — is applied to each Scalar value,
  using the atom-form safety predicates defined in §22.2:

  1. **Inline atom** — used if the value is **inline-safe**.
  2. **Source atom** — used if the value is not inline-safe but is **source-safe**.
  3. **Literal atom** — used in every other case, with a delimiter chosen so the value is
     **literal-safe** with respect to its delimiter line (see the dash-extension rule below).

  The **first** form in the order inline → source → literal whose safety predicate the value
  satisfies determines the form; forms further down the list are not consulted. This makes the
  choice deterministic across implementations even when a value would technically be representable
  in more than one form.
- Each inline atom whose value contains a `U+0020` SPACE uses two preceding spaces (a hard-space
  run, `precedingSpaces = 2`) so the parser keeps the value's soft spaces as content (§10.3); an
  inline atom whose value contains no space uses a single preceding space (`precedingSpaces = 1`).
- Each compound child is indented by one level (two spaces) relative to its parent.
- Literal atoms use the delimiter `---` unless the payload contains the atom's effective
  delimiter line (§22.2) as a line. In that case, the delimiter is
  lengthened by one `-` at a time (`----`, `-----`, …) until the delimiter line no longer appears
  as a line in the payload. This dash-extension algorithm is normative for canonical
  serialization; it is what makes property P3 (canonical determinism) hold for payloads that
  contain a colliding line.
- Line endings use LF mode.

Two documents with identical semantic models, serialized canonically by the same version of the
specification, MUST produce identical text output.

### 22.4 Round-Trip Properties

The canonical text serialization (§22.3) and the BinTEL encoding (BinTEL §7) together with
parsing and BinTEL decoding satisfy the following invariants. Conforming implementations MUST
preserve these invariants; they are the contract between the spec and any tool that round-trips
TEL data. In the statements below, `≡` denotes semantic-model equality — the two sides are
identical trees of `Element` values (§18.2) — while `=` denotes byte equality of serialized
output.

**P1. Semantic round-trip via canonical text.** For every well-typed semantic model M produced
by parsing a TEL document under a schema S,

```
type-assign(parse(canonical-serialize(M, S)), S)  ≡  M
```

That is: serialising M canonically and re-parsing under the same schema reproduces M exactly.
Presentation-layer details that are not part of the semantic model — comments, remarks,
tabulation, atom form, blank lines, sigil — are not required to round-trip; the semantic content
is. In particular, a Scalar value carried as a source atom round-trips through canonical
serialisation as the same `text` string (per the LF-join rule of §14), so a multi-line scalar
value is preserved byte-for-byte across the cycle.

**P2. BinTEL round-trip.** For every well-typed semantic model M and the same schema S,

```
bintel-decode(bintel-encode(M, S), S)  ≡  M
```

The BinTEL encoder (§7 of the BinTEL Specification) and decoder (§7.8) are mutual inverses on
well-typed semantic models. As with P1, only the semantic content is preserved.

**P3. Canonical-text determinism.** For every well-typed semantic model M and schema S, and any
two conforming implementations `f` and `g` of canonical serialization,

```
f(M, S)  =  g(M, S)
```

(byte-equal): the canonical serialization of M under S is a single, well-defined byte string,
independent of which conforming implementation produces it. This follows from §22.3 and the
fact that the schema fixes both member order and atom-form escalation (§22.3). Note also that
P1 makes `canonical-serialize` injective on well-typed semantic models: two distinct models
cannot canonically serialize to the same text.

**P4. BinTEL determinism.** For every well-typed semantic model M and schema S, and any two
conforming implementations `f` and `g` of the BinTEL encoder,

```
f(M, S)  =  g(M, S)
```

(byte-equal), which combined with the canonical child order of §7.2 (BinTEL spec) makes the
**value hash** (§3 of the BinTEL Specification) a function of the semantic model alone — two
implementations that agree on M and S MUST produce identical value hashes.

A conforming implementation that fails any of P1–P4 is non-conforming. A test suite for a TEL
implementation SHOULD include cases that exercise each property over a representative set of
documents (including pathological cases: multi-line scalar values requiring source/literal
form, repeatable Fields with multiple atoms, layered schemas, exclude operations, and
Reference cycles).

### 22.5 Concurrent Edit Composition

When two agents independently apply sequences of machine operations (§22.2) to the same
baseline document, the resulting documents diverge. This subsection specifies a merge
procedure that always produces a schema-valid result and exhibits strong eventual consistency
for the semantic model. The procedure exploits TEL's presentation-only constructs (remarks,
attached comments — §11.1, §11.2) to record conflicts without distorting the semantic content.

**Operation ordering.** Every operation MUST carry a **Lamport timestamp** (a monotonically
increasing integer per agent) and an **agent identifier** (a stable kebab-case string unique
to the originating agent). The total order over operations is `(timestamp, agent_id)` —
lexicographic on the pair, with `timestamp` compared as integers and `agent_id` compared
lexicographically by code point.
This ordering is deterministic and depends only on the operation set, not on the order of
arrival.

**Merge function.** Given a baseline document `B₀` and two operation sequences `O_A`, `O_B`
each derived from `B₀`, the merge produces `(D, R)`:

1. Form the combined ordered sequence `S = sort(O_A ∪ O_B)` by the order above.
2. Starting from `B₀`, process each operation `op` in `S`:
   a. Determine whether `op` can be applied to the current document: its target (path or
      keyword) must still resolve, and applying it must leave the document schema-valid
      (no E2xx or E3xx errors against the schema in effect for the document).
   b. If yes, apply `op` normally.
   c. If no, **demote** `op`: do not apply it; instead, attach a **conflict remark** to the
      affected compound (or, if the target was removed, a **conflict comment** to the
      nearest surviving ancestor block) describing what was attempted. The remark or comment
      records the operation's agent, timestamp, and a short description.
3. Return `D` (the resulting document) and `R` (the list of demoted operations, for
   downstream audit).

**Path rebasing.** Operation paths are positional (§22.2) and were captured against the state
each agent observed, but the merge applies operations to an evolving document in which
operations from the other agent may already have inserted, deleted, or reordered compounds.
Before the test in step 2a, the merge engine MUST **rebase** each of `op`'s paths: every
insertion, deletion, or reordering already applied in this merge adjusts the affected
`(block_index, compound_index)` components of later operations' paths (the merge engine is the
"caller" of §22.2 and carries the path-invalidation responsibility stated there), so that a
rebased path addresses the same compound it addressed when the operation was constructed,
whenever that compound survives. A path whose referent has been deleted or replaced does not
resolve, and the operation is demoted per step 2c. Operations from the same agent are already
sequential — each was constructed against the state produced by its predecessors — so rebasing
adjusts only for the other agent's interleaved operations.

**Conflict remark format.** When a demoted operation's target still exists, a remark is
attached to the target compound:

```
<sigil> Merge conflict (agent <agent_id>, ts=<timestamp>): <operation summary>
```

When the target has been removed by a prior operation in the merge, a comment is attached
to the nearest surviving block:

```
<sigil> Merge conflict (agent <agent_id>, ts=<timestamp>): <operation summary> — target was removed
```

In both formats, `<sigil>` is the document's effective sigil as determined by §8.3 — not a
hard-coded `#`. Merge engines MUST use the document's sigil when writing conflict markers; using
`#` against a document whose sigil is (say) `;` would emit a compound keyword `#` and trigger
**E306** at parse time rather than producing a valid remark. For example, if a document's sigil
is `;`, a conflict remark attached to a compound at indent 1 is written as:

```
  keyword atoms  ; Merge conflict (agent alice, ts=42): update-value attempted
```

and a free-standing conflict comment whose target was removed appears as:

```
  ; Merge conflict (agent alice, ts=42): update-value — target was removed
```

**Properties.**

- **Total.** The merge function is total: it always returns a `(D, R)`. There is no "merge
  failed" outcome at the document level. Pathological inputs are recorded as conflict
  remarks; the document remains schema-valid.
- **Convergent.** Two agents that have observed the same set of operations produce
  identical canonical text (§22.3), because §22.3 strips remarks and comments. The
  presentation layer may differ in the number and placement of remarks, but the semantic
  content is identical.
- **Forensic.** Every operation that could not be applied is recorded as a remark or
  comment carrying the agent, timestamp, and a description. An auditor or a later editor
  can review the conflicts and apply them manually.
- **Schema-driven.** The "would this leave the document schema-valid?" check uses the
  existing §20 / §21 error machinery; no new validation surface is needed.

**Operation subset compatibility.** The merge procedure is well-defined for all §22.2
operations. For some operations, conflicts are particularly common:

- Concurrent `update-value` on the same compound: both operations apply and neither is
  demoted (each resolves and each leaves the document schema-valid); the higher-ordered
  operation's value survives — last write wins.
- Concurrent `delete` and `update-value` on overlapping subtrees: the `delete` wins. An
  `update-value` ordered before the `delete` applies and is then erased with the subtree; one
  ordered after the `delete` no longer resolves and is demoted.
- Concurrent `insert-before` / `insert-after` at the same position: both succeed (each
  inserts a new element); their relative order in the resulting document is determined by
  the Lamport order of the two operations.
- Concurrent `reorder-*` and `replace`: the higher-ordered operation is demoted when the
  lower-ordered one has left its target unresolvable or the result schema-invalid.

A schema-aware agent MAY pre-emptively coordinate to avoid conflicts (locking, lease-based
ownership, etc.), but this specification does not mandate any coordination protocol.
Coordination is the application's concern; the merge procedure above is the contract for
the case where coordination fails or is absent.

## 23. Invalidity Conditions

A TEL document is **invalid** if any condition identified by an **E1xx** (parsing) or **E3xx**
(validation) error code in this specification is triggered. A schema is invalid if any **E2xx**
condition is triggered. The diagnostic tables are §19.3 (E1xx), §20.1 (E2xx), and §21.6 (E3xx);
recovery strategies for parsing errors are given in §19.5. Every error code referenced anywhere
in this specification appears in exactly one of these three tables.

A document is **schema-valid** with respect to a schema when parsing and type assignment
against that schema trigger no E2xx or E3xx condition. This is the predicate used by the
machine operations (§22.2) and the concurrent-edit merge (§22.5) to decide whether an edit may
be applied.

## 24. Formal Type System and Subtyping (Informative)

This section is **informative**, not normative. It gives a formal account of TEL's type system,
defines a subtype relation `<:` over the types of §20, states the inference rules for that
relation, sketches that the layer composition rules of §20.3 produce subtypes of the base
schema, and shows that the **Liskov Substitution Principle** holds: a document valid under a
subtype is, after projection, a valid document under the supertype.

Conformance to this specification does not require an implementation to compute `<:`
explicitly: §20.2, §20.3, and §21 are stated as concrete algorithms and discharge every
constraint a conforming parser must check. §24 exists to give schema authors and tool builders a
precise shared vocabulary for what "compatible schemas" means.

### 24.1 Type Grammar

The recursive type structure of §20, written compactly:

```
T  ::=  Struct(M*, V*)        — record / product
     |  Scalar(V*)            — leaf value
     |  Flag                  — presence-only
     |  Reference(N)          — named recursive type

M  ::=  Field(K, r, p, T, d?)
     |  Select(r, p, X+)

X  ::=  Variant(K, T)

K, V     ::=  identifier      (per §20.7)
N        ::=  type-name       (per §20.7)
d        ::=  text            (default value, optional)
r, p     ::=  true | false    (required, repeatable)
```

`Select(r, p, X+)` is a Select member (§20) — a `SelectRef` with the variants of its referenced
`SelectDefinition` inlined; inlining is harmless in this informative account, which never needs
Definition sharing.

`r` and `p` are the *effective* boolean values of the tristate polarities of §20: `r` is
`true` iff `member.required != "loose"`, and `p` is `true` iff `member.repeatable == "loose"`.
The `"default"`/`"loose"`/`"tight"` distinction matters only during layer merge (§20.3); the
composed type's meaning depends only on the effective booleans, which is what this grammar
records.

A **schema context** Δ is a finite map from Definition names to Definition bodies:

```
Δ : N → (M*, V*)
```

So `Δ(N) = (members, validators)` when the schema has `record N\n  …\n  validate …` (or
analogously the `validators` list alone for a `scalar N` Definition). The composed Δ is the
merge of the base schema's `Schema.records ∪ Schema.scalars ∪ Schema.selects` with each
layer's `Layer.records ∪ Layer.scalars ∪ Layer.selects`, per §20.3.

The `Exclude(K)` operation of §20.3 is a layer-only construct that operates on Δ during
composition; it does not appear in a composed type and so is not part of T.

### 24.2 Membership

The membership judgment `Δ ⊢ d : T` is read "under context Δ, semantic-model element d is
of type T". It is defined by induction on T:

```
[Mem-Flag]            ―――――――――――――――   (no premise; Flag has only "present")
                      Δ ⊢ flag-node : Flag

[Mem-Scalar]          For every v in V*, validator v applied to text returns Valid.
                      ―――――――――――――――――――――――――――――――
                      Δ ⊢ scalar-node(text) : Scalar(V*)

[Mem-Struct]          d has children c_1, …, c_n matching M* per §20.2 atom + compound phases
                      (member-fill, required, repeatable, contiguity).
                      For each c_i, Δ ⊢ c_i : type-of-c_i.
                      For every v in V*, validator v applied to d returns Valid.
                      ―――――――――――――――――――――――――――――――
                      Δ ⊢ struct-node(c_1, …, c_n) : Struct(M*, V*)

[Mem-Reference]       Δ ⊢ d : Struct(members, validators)        Δ(N) = (members, validators)
                      ―――――――――――――――――――――――――――――――
                      Δ ⊢ d : Reference(N)
```

These rules correspond exactly to the type-assignment algorithm of §20.2 and the validation
rules of §21. A document is valid under a schema iff `Δ ⊢ d : Schema.document` (treating
the schema-document Struct as the root type).

### 24.3 Subtype Relation

The subtype relation `T₁ <: T₂` (under Δ, when needed) is defined by the following
inference rules. The intuition is: `T₁ <: T₂` means any element of type T₁ contains
enough information to satisfy any consumer that expects type T₂.

```
[Sub-Refl]            T <: T

[Sub-Trans]           T₁ <: T₂        T₂ <: T₃
                      ――――――――――――――――――――――――
                      T₁ <: T₃

[Sub-Flag]            Flag <: Flag

[Sub-Scalar]          V₂ ⊆ V₁                          (sub has at least super's validators)
                      ――――――――――――――――――――――――
                      Scalar(V₁) <: Scalar(V₂)

[Sub-Struct]          There is a strictly increasing map φ from M₂'s positions into M₁'s
                      positions such that M₁[φ(j)] <:_M M₂[j] for every position j of M₂.
                      (M₂ matches an order-preserving subsequence of M₁; order matters
                      because membership [Mem-Struct] consumes members in member order.)
                      V₂ ⊆ V₁                          (sub has at least super's validators)
                      ――――――――――――――――――――――――
                      Struct(M₁, V₁) <: Struct(M₂, V₂)

[Sub-Ref-L]           Δ ⊢ Struct(Δ(N).members, Δ(N).validators) <: T
                      ――――――――――――――――――――――――
                      Δ ⊢ Reference(N) <: T

[Sub-Ref-R]           Δ ⊢ T <: Struct(Δ(N).members, Δ(N).validators)
                      ――――――――――――――――――――――――
                      Δ ⊢ T <: Reference(N)
```

**Member subtyping (`<:_M`)** has two cases, one per member kind:

```
[Sub-Field]           K₁ = K₂                           (same keyword)
                      T₁ <: T₂                          (covariant in type)
                      r₂ ⟹ r₁                           (sub at least as required)
                      p₁ ⟹ p₂                           (sub at most as repeatable)
                      ――――――――――――――――――――――――
                      Field(K₁, r₁, p₁, T₁, d₁) <:_M Field(K₂, r₂, p₂, T₂, d₂)

[Sub-Select]          For every Variant(K, T₁) ∈ X₁, there exists Variant(K, T₂) ∈ X₂
                      such that T₁ <: T₂.            (sub's variants ⊆ super's variants)
                      r₂ ⟹ r₁                           (sub at least as required)
                      p₁ ⟹ p₂                           (sub at most as repeatable)
                      ――――――――――――――――――――――――
                      Select(r₁, p₁, X₁) <:_M Select(r₂, p₂, X₂)
```

**Reading the rules.**

- **Records are subtyped by extension.** A Struct with more members is a subtype: every
  member required by the supertype must be present (and subtype-compatible) in the
  subtype; the subtype may have additional members the supertype knows nothing about.
- **Sums are subtyped by narrowing.** A Select with fewer variants is a subtype: every
  variant offered by the subtype must be offered by the supertype, but the supertype may
  accept variants the subtype never produces.
- **Scalars are subtyped by tightening.** A Scalar with more validators is a subtype:
  values that satisfy the subtype's validators automatically satisfy the supertype's.
- **`required` is tightenable.** Going from `required: false` to `required: true` is the
  subtype direction: any value satisfying the stricter rule also satisfies the lax one.
- **`repeatable` is tightenable in the opposite direction.** Going from `repeatable:
  true` to `repeatable: false` is the subtype direction: a non-repeatable cardinality
  (0 or 1) is a special case of a repeatable cardinality (0 or more).

On recursive Definitions, [Sub-Ref-L] and [Sub-Ref-R] unfold references without bound; the
relation is interpreted **coinductively** (equivalently, with an assumption set of pairs of
Definition names that short-circuits repeated unfolding), so `<:` is well-defined over a
cyclic Δ.

[Sub-Field] places no constraint on defaults: `d₁` and `d₂` are unrelated in the rule.
Layer composition never produces a pair that differs in `default` (§20.3 forbids changing
it), so default-divergent subtypes, though admitted by the relation, do not arise from
layering, and changing a default MUST NOT be inferred to be compatibility-safe.

The rules are sound: each premise mirrors a constraint that would distinguish valid
elements of one type from valid elements of the other. Reflexivity and transitivity are
immediate from the structure.

### 24.4 Layer Composition Produces Subtypes

**Theorem (Layer Subtyping).** Let `S_base` be a schema with document Struct `D_0` and
Definition context `Δ_0`. Let `L` be a layer satisfying the validity constraints of §20.3.
Let `(D_1, Δ_1)` be the result of applying `L` to `(D_0, Δ_0)` per §20.3. Then:

```
Δ_1 ⊢ D_1 <: D_0   (with Δ_0 viewed as a subset of Δ_1)
```

That is, **applying a layer always produces a subtype of the base**.

**Proof sketch.** The permitted operations of §20.3 fall into seven subtype-producing
categories. (Each of §20.3's permitted operations belongs to at least one category; adding a
new Definition extends Δ without changing any existing type, and layers only ever *append*
members, so [Sub-Struct]'s order-preservation premise always holds.) The categories are:

1. **Add Field** — `D_1` has all of `D_0`'s members plus a new Field. By [Sub-Struct],
   `D_1 <: D_0` because every member of `D_0` still appears in `D_1` (with identical
   `<:_M`-self).
2. **Add SelectRef** — As above, additional Select member. `D_1 <: D_0`.
3. **Definition merge (recursive Struct extension)** — when a layer adds a Field whose
   keyword matches an existing Field of Struct type, the resulting Struct's members are
   the union of base and layer. By [Sub-Struct] applied recursively, the new Struct is a
   subtype of the old, so the Field is subtype-compatible by [Sub-Field].
4. **Exclude variant** — `D_1`'s Select has strictly fewer variants than `D_0`'s. By
   [Sub-Select], `D_1 <: D_0`.
5. **Tighten `required`** — `D_1` has the same Field/Select but with `required: false →
   true`. By [Sub-Field] / [Sub-Select] (premise `r₂ ⟹ r₁`), the tightening is subtype-
   producing.
6. **Tighten `repeatable`** — `D_1` has the same Field/Select but with `repeatable: true
   → false`. By [Sub-Field] / [Sub-Select] (premise `p₁ ⟹ p₂` where the subtype is the
   one with `p₁ = false`), the tightening is subtype-producing.
7. **Add struct or scalar validator** — `V_1 ⊇ V_0`. By [Sub-Struct] or [Sub-Scalar],
   the type is a subtype.

§20.3 forbids the supertype-producing operations: removing a Field, adding a variant to
an existing Select, loosening `required` from true to false (E214), loosening
`repeatable` from false to true (E215), dropping a validator. By construction, no
permitted layer operation moves in the supertype direction.

By transitivity ([Sub-Trans]), the iterative application of layers yields a chain of
subtypes:

```
D_n <: D_{n-1} <: … <: D_1 <: D_0
```

The composed schema is a subtype of the base. ∎

### 24.5 The Liskov Substitution Theorem

**Theorem (LSP).** If `T₁ <: T₂` and `Δ ⊢ d : T₁`, then there exists a **projection**
`π_{T₂}(d)` such that `Δ ⊢ π_{T₂}(d) : T₂`.

**Projection** discards the parts of `d` that aren't addressable from `T₂`:

```
π_{T₂}(d) = case (T₂, d) of:

  Flag, flag-node                  → flag-node
  Scalar(V₂), scalar-node(text)    → scalar-node(text)
                                     (validators in T₂ are checked separately)
  Struct(M₂, V₂), struct-node(c*)  → struct-node(c'*) where c'* is
                                     { π_{type-of-m_i-in-T₂}(c_i)
                                       | c_i is a child whose keyword appears in M₂'s
                                         keyword order (a Field member's keyword, or a
                                         variant keyword of a Select member) }
  Reference(N), d                  → π_{Struct(Δ(N).members, Δ(N).validators)}(d)
```

In words: at every Struct, drop children whose keywords don't appear in T₂'s members;
recurse into the surviving children at the type T₂ ascribes to them. Scalars and Flags
pass through unchanged.

**Proof sketch.** By induction on T₂:

- **Flag.** π is the identity. The result is the same flag-node, of type Flag. ✓
- **Scalar(V₂, _).** π is the identity on the text. By [Sub-Scalar], `V₂ ⊆ V₁`; since
  `d` satisfied every validator in `V₁`, it satisfies every validator in `V₂` (subset).
  So `π_{Scalar(V₂)}(d) : Scalar(V₂)`. ✓
- **Struct(M₂, V₂).** For each `m₂ ∈ M₂`, [Sub-Struct] gives a matching `m₁ ∈ M₁` with
  `m₁ <:_M m₂`. The corresponding child in `d` has a type that's a subtype of `type-of-m₂`
  by [Sub-Field] or [Sub-Select]. By IH on the child, `π_{type-of-m₂}(child) :
  type-of-m₂`. Validators in `V₂ ⊆ V₁` were satisfied by `d`. Required/repeatable
  constraints carry: if `m₂` required the member (`r₂ = true`), then `r₂ ⟹ r₁` gives
  `r₁ = true`, so the member is present; and since `p₁ ⟹ p₂`, if `m₂` is non-repeatable
  (`p₂ = false`) then `m₁` is non-repeatable too, so at most one occurrence is present. ✓
- **Reference(N).** Inductive: substitute the resolved Struct. ✓

In every case the projection yields a valid `T₂` element.   ∎

### 24.6 Consequences for Tooling

LSP gives the schema ecosystem a useful guarantee:

- A **document written against a subtype schema** can be consumed by any tool that
  understands the supertype schema, as long as the tool reads through the supertype's
  schema (so it implicitly performs the projection by ignoring unknown fields).
- The **schema-signature compatibility rule** of §8.2 (a document's signature is
  compatible with a consumer's signature when the consumer's decoded hash sequence is a
  subsequence of the document's) is now grounded: §24.4 establishes that the composed-with-fewer-layers schema is
  a supertype of the composed-with-more-layers schema, so a document written against the
  longer composition can be consumed (after projection) by a tool expecting the shorter
  composition.
- **`construct` operations** (§22.2) can target the supertype's schema: a freshly
  constructed compound that satisfies the supertype's required-set is automatically a
  valid subtype value at any position where the subtype permits the same members. (The
  reverse — using a supertype-validated compound at a subtype position — is NOT
  generally safe; the subtype may demand additional fields the supertype didn't supply.)
- **The implementation never needs to compute `<:` explicitly.** The type-assignment
  algorithm (§20.2) directly checks element membership; the layer composition algorithm
  (§20.3) directly produces the subtype. Subtyping is a property of how these algorithms
  fit together, not a separate runtime check.

### 24.7 What This Type System Does NOT Cover

- **Variance of validators.** The subtype relation requires `V₂ ⊆ V₁` — the subtype has
  at least the supertype's validators. It does not require that any validator
  *strictly* tightens the value space (a validator that always returns Valid satisfies
  the subset relation trivially). A schema author who wants a meaningful subtype
  refinement is expected to add validators that genuinely restrict; the type system
  does not check this.
- **Cross-document subtyping.** Two distinct schemas (different base names) are not
  subtypes of each other under this relation, even if their structural shapes happen to
  match. Subtyping is meaningful only within a single base-schema family (the same
  `Schema.name`, plus an ordered subset of layers).
- **Behavioural / semantic subtyping.** The relation here is purely structural. Two
  Scalars with the same validators are subtype-equivalent even if their *names* suggest
  different semantics (`postal-code` vs `phone-number`). Behavioural distinctions are
  the application's concern.

## 25. Completeness of this Specification

This v1.0 specification is complete for single-document and single-agent use. The error
taxonomy comprises **E101–E123** (parsing; E110 is reserved, §19.5), **E201–E217** (schema),
and **E301–E311** (validation); every code is referenced at the point
in the body where its trigger condition is defined and appears exactly once in the diagnostic
tables of §19.3, §20.1, and §21.6. Worked examples —
including TEL documents shown with their presentation model, semantic model, and BinTEL byte
sequence — are recorded in [`demo/`](demo/). Round-trip properties (P1–P4) are stated in §22.4.
Concurrent-edit composition is stated in §22.5. Schema compatibility is defined by the subtype
relation of §24 and decided on signatures per §8.2.
