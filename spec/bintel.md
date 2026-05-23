# BinTEL Specification Draft

## Abstract

BinTEL is the binary encoding of the semantic model of a TEL document, as defined by the
[TEL Specification](tel.md). Every well-typed TEL document has exactly one BinTEL encoding; the
mapping is fully deterministic. A schema is itself a TEL document and therefore has a BinTEL
encoding.

BinTEL provides an unambiguous, compact serialization of the semantic model, suitable for hashing,
transmission, and schema identification.

A BinTEL document is defined here as a byte sequence. Where a text-oriented carrier is required —
embedding in a TEL document, transmission over a textual channel, display, or copy-and-paste — a
BinTEL byte sequence MAY be encoded as Unicode text using BASE-256 (see
[BASE-256 Specification](base256.md)). The textual form is character-for-byte with the byte
sequence and is recovered losslessly by the BASE-256 decoder. See §9 for the conformance details.

## 1. Status

This document is a draft specification of BinTEL.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Value Hash

The **value hash** of a TEL document is the SHA-256 digest of its BinTEL representation excluding
the magic number and schema signature — that is, the hash is computed over only the document root
encoding (as defined in the Node Encoding section). This is the general method for hashing any
semantic TEL value, including schema documents (which are themselves TEL documents).

When used in a schema identifier (see §8.1 of the TEL Specification), one or more value hashes —
one per component of the composed schema (the base plus each layer in order) — are combined into
a **schema signature** per §8 below. The signature is encoded as [BASE-256](base256.md) for
textual representation. A schema with no layers has a single-component signature whose bytes are
exactly the 32-byte value hash, encoded as 32 BASE-256 characters.

### Normative Test Vector

The value hash of [`tel-schema.tel`](tel-schema.tel) — the schema-for-schemas defined in §20.5 of
the TEL Specification — is:

```
SHA-256:  df50abce267dc79106d4320f0879fb054236e8dce9efa04872fb5e2a6560fc52
BASE-256: ӟPΫώȦṽÇґĆÔ2ďȈyûąB6ǨӜῩӯƠHrûŞЪeŠǼR
```

A conforming implementation that encodes the canonical `tel-schema.tel` (980 BinTEL bytes; raw
bytes recorded in [`demo/tel-schema.bintel.hex`](demo/tel-schema.bintel.hex)) and hashes
the resulting document-root encoding MUST produce this value byte-for-byte.

## 4. Integer Encoding

All counts and byte-lengths in BinTEL are non-negative integers encoded in a variable-length format.
To encode an integer N:

1. Set B = N & 0x7F (the seven least-significant bits of N).
2. Set N = N >> 7.
3. If N > 0, set bit 7 of B (i.e. B = B | 0x80) and write the byte B; then repeat from step 1.
4. If N = 0, write B as the final byte (bit 7 is clear).

The result is one or more bytes. Every byte except the last has bit 7 set (a **continuation byte**).
The last byte has bit 7 clear. The seven low-order bits of each byte, concatenated from
least-significant (first byte) to most-significant (last byte), reconstruct the original integer.

Decoding: read bytes in sequence; for each byte, take bits 0–6 and OR them into the accumulator at
the current bit offset; advance the bit offset by 7. If bit 7 of the byte is set, read the next
byte; otherwise the integer is complete.

| Value | Encoded bytes (hex) |
| ----: | ------------------- |
|     0 | `00`                |
|     1 | `01`                |
|   127 | `7F`                |
|   128 | `80 01`             |
|   255 | `FF 01`             |
| 16383 | `FF 7F`             |
| 16384 | `80 80 01`          |

## 5. Keyword Index

The **keyword index** of a child element is its zero-based position in the **keyword order** of the
parent's `Struct` type (§20 of the TEL Specification). Keyword order is a flat sequence: each
`Field` member contributes a single entry; each `Select` member contributes one entry per variant,
in declaration order. The keyword index uniquely identifies both the parent member and (for a
`Select`) the specific variant.

A keyword index is encoded as a single variable-length integer (§4). Because the schema determines
the type of every node from its keyword index and its parent's type, BinTEL encodes no type tags.
A decoder reads the keyword index, looks it up in the parent's keyword order to recover both the
keyword and the resolved child type, and proceeds with the corresponding type-specific encoding of
§7.1.

## 6. File Layout

A BinTEL stream represents **exactly one** TEL semantic model. A stream consisting of two or more
concatenated BinTEL encodings is not a conforming BinTEL stream; a producer MUST NOT emit such a
sequence and a decoder MUST treat the bytes following a complete BinTEL document root as a
framing error (§10) rather than as a second document.

A BinTEL stream consists of the following fields in order:

1. **Magic number**: the 4 bytes `B2 C4 B5 BB`. When the document is carried in BASE-256
   textual form (§9), these bytes appear as the four Greek letters at positions `0xB2`, `0xC4`,
   `0xB5`, and `0xBB` of the BASE-256 alphabet defined in the
   [BASE-256 Specification](base256.md) — namely the characters `β` (`U+03B2` Greek small
   beta), `τ` (`U+03C4` Greek small tau), `ε` (`U+03B5` Greek small epsilon), and `λ`
   (`U+03BB` Greek small lambda). A BinTEL stream therefore begins with the literal string
   `βτελ` in BASE-256 textual form — visually evocative of "binary TEL" (`β` for binary, `τελ`
   the Greek root for *tel*-) and, because none of the bytes is below `0x80`, unlikely to be
   mistaken for the start of an ASCII or UTF-8 text file.
2. **Schema signature**: the byte length of the signature (integer), followed by the signature
   bytes. The schema signature (whose construction is defined in the Schema Signature section
   below) identifies the composed schema (base plus layers) used to type the document. The byte
   length MUST satisfy `length ≥ 32` and `(length − 30) mod 2 == 0` (so that the signature
   describes a valid palimpsest at cadence k = 2, per §8.2).
3. **Document root**: encoded using the node encoding described in the Node Encoding section
   below (root form). The encoding terminates exactly when the recursive procedure of §7.8 has
   consumed the last byte of the document root; there is no trailing tag or length.

A conforming decoder MUST verify that all bytes of the input are consumed by this procedure. Any
bytes following the document root are a framing error (§10).

## 7. Node Encoding

BinTEL encodes the **semantic model** of a TEL document (§18 of the TEL Specification). The
semantic model is a tree of `Element` values: `Node`s (Struct- or Flag-typed) and `Value`s
(Scalar-typed). Every presentation-layer atom and every presentation-layer compound contributes
exactly one element (§18.2); the semantic model does not distinguish between an atom and a
compound that fill the same schema member.

### 7.1 Encoding by Element Type

**Document root.** The root is a virtual struct with no parent keyword. It is encoded as:

1. The number of root child nodes (integer).
2. Each root child node, in canonical order (§7.2), using the struct, scalar, or flag encoding
   below.

**Struct node** (schema type is `Struct`):

1. The keyword index of this node (integer).
2. The number of child nodes (integer).
3. Each child node, in canonical order (§7.2), using the struct, scalar, or flag encoding,
   recursively.

**Scalar node** (schema type is `Scalar`):

1. The keyword index of this node (integer).
2. The byte length of the UTF-8 encoding of the value string (integer).
3. The UTF-8-encoded bytes of the value string.

**Flag node** (schema type is `Flag`):

1. The keyword index of this node (integer).

### 7.2 Canonical Child Order

The children of a Struct node MUST be emitted in a deterministic **canonical order** that depends
only on the semantic content, not on the presentation form. This is what makes the value hash
(§3) a function of the semantic model alone — two presentations of the same semantic content
produce identical BinTEL bytes.

Canonical order is defined as follows. Given a Struct node whose schema type has members
`m₀, m₁, …, m_{n-1}` (in member order, §20 of the TEL Specification):

1. Iterate the members in member order.
2. For each member `mᵢ`, emit every element that fills `mᵢ` in source order. The elements that
   fill `mᵢ` are:
   - **Atom-derived elements**: each inline atom on the parent compound's line that the type
     assignment algorithm (§20.2) assigned to `mᵢ`, in atom order.
   - **Compound-derived elements**: each compound child whose keyword corresponds to `mᵢ`
     (either `mᵢ.keyword` if `mᵢ` is a `Field`, or any `mᵢ.variants[j].keyword` if `mᵢ` is a
     `Select`), in source order.
3. **Defaults.** If `mᵢ` is a required `Field` with `Scalar` type, has a non-null `default`, and
   was not filled by any atom or compound child, emit a single Scalar element at `mᵢ`'s position
   carrying `mᵢ.type.default` as its value.
4. Within a member, atom-derived elements precede compound-derived elements (§18.3 step 4 of
   the TEL Specification).

Because every member contributes its elements consecutively and the relative ordering between
members follows the schema-defined member order, the encoding is independent of the source
ordering of independent member groups — two documents whose only difference is the order of
distinct member groups produce identical BinTEL bytes.

### 7.3 Atom-Derived Elements

Per §18.3 of the TEL Specification, an inline atom on a compound's line corresponds to a child
element of that compound. The element's type is the type assigned by the atom phase of §20.2:

- An atom assigned to a `Field` whose type is `Scalar` produces a Scalar element whose value
  is the atom's text.
- An atom assigned to a `Field` whose type is `Flag` (i.e., the atom matches the Field's
  keyword) produces a Flag element.
- An atom assigned to a `Select` member whose variants are all `Flag` (i.e., the atom matches
  one variant's keyword) produces a Flag element at that variant's keyword index.

In every case, the resulting element is encoded by §7.1 the same way the equivalent compound
child would be encoded. An encoder MUST treat atom-derived and compound-derived elements
uniformly when emitting children.

### 7.4 Reference Types

A `Reference` type (as defined in §20 of the TEL Specification) is resolved to its target
`Struct` during type assignment (§20.2). Reference types do not appear in BinTEL: every node
encoded by this section has a schema type of `Struct`, `Scalar`, or `Flag`. A `Reference(N)` is
encoded exactly as the `Struct` named by N.

### 7.5 Empty Scalar Values

A Scalar element whose value is the empty string is encoded as keyword_index + `00` (a varint
length of zero) + no value bytes. The encoding does not distinguish:

- a Scalar Field that was explicitly filled with the empty string in the source document, and
- a missing required Scalar Field whose schema default is the empty string (§7.6).

This conflation is deliberate: BinTEL encodes the **semantic model**, in which both cases
result in the same `Value` element with `text = ""` (§18.2 of the TEL Specification). The
information needed to distinguish "explicitly empty" from "defaulted to empty" is presentation-
layer information; if an application needs to preserve this distinction, it MUST do so in the
presentation model (§18.1) rather than in BinTEL.

A decoder receiving a Scalar node with a zero-length value MUST therefore treat it as a
semantically present Scalar with an empty text. There is no encoding for "absent Scalar with no
default": such a member is reported at type-assignment time as an E307 error against the
document, and BinTEL never encodes an invalid document.

### 7.6 Default Values

BinTEL encodes the semantic model, in which a required `Scalar` member with a non-null default
is semantically present even when it was absent from the source document. Therefore, when
encoding a document to BinTEL, a missing required scalar whose default is used MUST be encoded
as a scalar node with the default value string. The encoded **value string** is the semantic
value of the schema's `Scalar.default` — that is, the post-atom-form-decoding text: the same
string that would be returned by reading the default scalar's `text` field from the parsed
schema. Equivalent schemas that declare the default via different atom forms (inline atom,
source atom, or literal atom containing identical textual content) MUST therefore produce
byte-identical BinTEL encodings for the same missing-required-scalar case. This ensures the
BinTEL encoding is identical regardless of whether the member was explicitly written in the
document or filled by its default, and regardless of the atom form used by the schema author
to declare the default.

### 7.7 Framing

There are no pad bytes, alignment constraints, or inter-node delimiters between the encoded
elements of a Struct's child list. The schema provides all type information needed to decode the
stream unambiguously: at each child position the decoder consults the parent's keyword order to
determine the child's type (Struct, Scalar, or Flag) from the next-read keyword index.

### 7.8 Decoding

A BinTEL decoder consumes the byte stream defined in §6 and produces the semantic model defined
in §18 of the TEL Specification. The decoder MUST have access to the resolved composed schema
before it begins reading the document root (§6 fields 1–2 supply the magic number and the schema
signature; the composed schema is obtained per §8.2 of the TEL Specification).

The decoding algorithm is recursive:

```
decode-document(bytes, schema):
  read magic = next 4 bytes; verify magic == [B2, C4, B5, BB] or report error (B01)
  read signature-length = decode-varint(bytes)
  read signature-bytes = next signature-length bytes
  // Resolution to a composed schema is handled at the §8.2 (TEL spec) layer;
  // this algorithm assumes the schema is already composed.
  root = decode-struct-body(bytes, schema.document.members)
  return Document { signature: signature-bytes, root }

decode-struct-body(bytes, members):
  child-count = decode-varint(bytes)
  children = []
  repeat child-count times:
    children.push(decode-element(bytes, members))
  return children

decode-element(bytes, parent-members):
  kidx = decode-varint(bytes)
  if kidx >= keyword-count(parent-members): report error
  (keyword, type) = lookup-by-index(parent-members, kidx)
  resolved-type = resolve(type, schema)   // Reference resolution per §20.2
  switch resolved-type:
    Struct(child-members):
      sub-children = decode-struct-body(bytes, child-members)
      return Struct-element { kidx, keyword, children: sub-children }
    Scalar:
      value-length = decode-varint(bytes)
      value-bytes = next value-length bytes
      value-text = UTF-8-decode(value-bytes)         // §4.1 of TEL spec applies
      return Scalar-element { kidx, keyword, text: value-text }
    Flag:
      return Flag-element { kidx, keyword }
```

A decoder MUST NOT distinguish between an element that was encoded from an atom and one that was
encoded from a compound child: §7.2 makes the encoding canonical, so the source distinction is
not recoverable from the BinTEL stream.

A decoder MAY stop after producing the semantic model; converting it to a presentation model is
outside BinTEL's scope (the source-level distinctions — atom form, remarks, comments, tabulation
— are not in BinTEL). The canonical text serialization defined in §22.3 of the TEL Specification
is one valid presentation form for a decoded semantic model.

## 8. Schema Signature

A schema signature identifies a composed schema as an ordered sequence of components: a base schema
followed by zero or more layers. Each component is identified by its value hash (§3).

A schema document (a TEL document conforming to the `tel-schema` schema; see §20 of the TEL
Specification) defines a base schema and zero or more layers. Each component's hash is its value
hash (§3): the component is encoded as a BinTEL document root (§7) and the SHA-256 digest is taken
over that root encoding alone, without the magic number or schema signature.

### 8.1 Per-Component Encoding

The base schema and each layer are encoded as standalone BinTEL document roots using §7. The
construction differs slightly between the two cases because a base schema is a complete
tel-schema document while a layer is only a sub-tree.

**Base-schema component.** The base schema's BinTEL encoding is produced by encoding the schema
document **with all `layer` compounds removed**, as a `tel-schema` document. That is: the
encoded element list at the root contains the `name`, `sigil`, `define`, and `document`
children, but no `layer` children, even when the original schema document declared layers. The
base schema is the schema-without-layers.

**Layer component.** A layer's BinTEL encoding is produced by treating the `layer` compound's
children as the document root of a virtual schema whose `document` Struct is the `layer-body`
Definition from tel-schema. Concretely: the encoded element list at the root contains the
layer's `name` (Scalar), each of its `define`-compound children, and its `root` child (if
present), in canonical order per §7.2 (which, by member-order convention, matches the order
listed). The layer's `Struct` is the `layer-body` Definition, so keyword indices are computed
against that Definition's keyword order.

A conforming implementation of `schema-signature(schema-document)` therefore:

1. Constructs the base-schema document (the schema document minus its `layer` compounds) and
   computes h₀ = SHA-256 of its document-root BinTEL encoding.
2. For each `layer` compound L_i in source order, computes h_{i+1} = SHA-256 of the document-root
   BinTEL encoding of L_i's children under the `layer-body` Definition.
3. Combines the sequence (h₀, h₁, …, h_n) into the palimpsest signature per §8.2 below.

### 8.2 Signature Construction

The construction below is a **palimpsest** with byte cadence `k = 2`, as defined in the
[Palimpsest Specification](palimpsest.md). The palimpsest framework permits any byte cadence;
this specification fixes `k = 2`, which is sufficient for schema libraries of up to 65 536
distinct components without backtracking during decode, while keeping signature size growth to two
bytes per layer.

**Encoding.** Given an ordered sequence of n component hashes h₀, h₁, …, h_{n−1} (each 256 bits),
the signature is computed as follows:

1. Let S = 0 (a zero-valued integer of unbounded width).
2. For each hash hᵢ, in order from i = 0 to i = n−1: set S = (S << 16) XOR hᵢ.
3. The result S has a width of 256 + (n−1)×16 bits, or equivalently `30 + 2n` bytes.

Emit the signature as `30 + 2n` bytes, most-significant byte first. For n = 1 (no layers), the
signature is exactly the 32-byte value hash of the base schema.

**Textual form.** When a schema signature appears in textual contexts — most notably the schema
identifier of a TEL pragma (see §8.1 of the TEL Specification) — it is encoded with
[BASE-256](base256.md), producing one Unicode character per signature byte (`30 + 2n`
characters total). BASE-256 is chosen over BASE64-URL or hex because (a) it is the most compact
character-per-byte encoding available — half the length of hex; (b) every character is a Unicode
letter or digit, so the encoded signature is a single word for double-click selection (per Unicode
Annex #29); and (c) the alphabet contains no whitespace or punctuation, so the signature always
occupies a single phrase on the pragma line. Encoders and decoders use the alphabet defined in §4
of the BASE-256 Specification.

**Correctness property.** Because each shift is 16 bits wide but each hash is 256 bits wide, the
lowest 16 bits of S are determined solely by the last hash h_{n−1}. After XORing h_{n−1} out of S
and shifting right by 16 bits, the lowest 16 bits are determined solely by h_{n−2}. This property
holds at every step, enabling unambiguous decoding so long as no two hashes in the candidate
library share the same final 16 bits — see the palimpsest specification for the probabilistic
analysis.

**Decoding.** Given a signature S of known byte length L, and a set of candidate hashes H (the value
hashes of all components defined in the schema file):

1. Compute n = (L − 30) / 2. This is the number of components. L MUST be even and at least 32.
2. Let the output sequence be empty.
3. Repeat n times: a. Let b = S & 0xFFFF (the lowest two bytes of S). b. Find all hashes in H whose
   lowest two bytes equal b. c. For each candidate hash h: compute S′ = (S XOR h) >> 16. d. Recurse
   with S = S′ and the candidate h appended to the front of the output sequence.
4. When n steps have been completed, S MUST be zero. If S ≠ 0, the candidate path is invalid;
   backtrack and try the next candidate.
5. Exactly one valid sequence MUST exist. If no valid sequence is found, or if more than one is
   found, the signature is malformed.

The decoded sequence gives the component hashes in order: h₀ (base schema), h₁ (first layer), …,
h_{n−1} (last layer). A BinTEL decoder uses this sequence to locate and compose the schema before
decoding the document root.

Schema compatibility is defined in §8.2 of the TEL Specification in terms of subsequence
relationships between decoded signature hash sequences.

## 9. Textual Encoding

A BinTEL byte sequence MAY be represented as Unicode text by applying the BASE-256 encoding defined
in the [BASE-256 Specification](base256.md). The textual form has one Unicode character per
input byte; the original bytes are recovered by the BASE-256 decoder, which computes the input
byte as the Unicode code point of each character taken modulo 256.

The choice of textual or byte form is purely a transport concern. No structural rule defined
elsewhere in this specification is altered by the choice:

- The value hash (§3) is computed over the BinTEL document root **as bytes**, not over its
  BASE-256 textual form.
- The schema signature (§8) is constructed and decoded over **bytes**, not over their BASE-256
  textual form.
- The magic number (§6), schema signature length, and all integer and node encodings (§4, §7) are
  defined in terms of bytes and remain unchanged.

A producer that emits BinTEL in textual form MUST apply BASE-256 to the complete byte sequence
defined by §6, including the magic number and schema signature. A consumer that receives BinTEL in
textual form MUST first decode the BASE-256 input back to bytes before applying any rule of this
specification.

## 10. Decoder Error Handling

A conforming BinTEL decoder MUST detect and report each of the following error conditions. Codes
are local to BinTEL; they do not overlap with the E1xx/E2xx/E3xx taxonomy of the TEL
Specification.

| Code | Description                                                                                  |
| ---- | -------------------------------------------------------------------------------------------- |
| B01  | Magic number absent or does not match the bytes `B2 C4 B5 BB` (BASE-256: `βτελ`) (§6 field 1). |
| B02  | A variable-length integer (§4) extends beyond the end of input, or its accumulator overflows the decoder's chosen integer width. |
| B03  | Schema signature length is less than 32 bytes or is not `30 + 2n` for any `n ≥ 1` (§6 field 2). |
| B04  | Schema signature does not decode against the available hash library (§8.2 decoding); zero or more than one valid hash sequence. |
| B05  | A keyword index read from the stream is outside `[0, keyword-count(parent-members))` (§7.8). |
| B06  | A Scalar value's byte length extends beyond the end of input.                                 |
| B07  | A Scalar value's UTF-8 bytes are not a valid UTF-8 sequence.                                  |
| B08  | The document-root decoding procedure of §7.8 terminates with input bytes remaining (framing error per §6). |
| B09  | The document-root decoding procedure of §7.8 requests bytes beyond end of input.              |
| B10  | A `Reference` type appears in the schema but resolves to no `Definition` (E210 condition at parse time; surfaced by the decoder as a configuration error if the composed schema is malformed). |

A decoder SHOULD distinguish a recoverable error (e.g., a malformed scalar at a known position
where the schema permits an absent value) from a fatal error (e.g., a bad magic number that
invalidates the rest of the stream). For the codes above, the following are RECOMMENDED behaviour:

- **B01, B02 (when decoding magic/signature length), B03, B04**: fatal — abort decoding.
- **B05, B06, B07, B09**: fatal at the point of detection — abort decoding. (A scalar with a
  bad UTF-8 sequence cannot be recovered; downstream callers cannot rely on partial decoding.)
- **B08**: fatal — the stream is malformed and any prior decoding output should be discarded.
- **B10**: fatal — the schema is malformed; report the configuration error and abort.

A decoder MAY perform additional consistency checks beyond those above (for example, checking
that a Struct-typed child's claimed type is actually a Struct after Reference resolution); these
are implementation-specific and are not error conditions defined by this specification.
