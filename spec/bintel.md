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

The **value hash** of a TEL document is the 256-bit BLAKE3 digest of its BinTEL document-root
encoding (§7.1) — that is, the bytes produced by the recursive node encoding of the document
root, with the magic number, schema signature, and (in self-contained mode, §6.2) embedded
schema body excluded. This is the general method for hashing any semantic TEL value, including
schema documents (which are themselves TEL documents). 256-bit BLAKE3 corresponds to hash-size
index `s = 7` of the [Palimpsest Specification](palimpsest.md) (§3.1).

The value hash is **mode-independent**: encoding the same semantic content under the same
composed schema in external-schema mode (§6.1) and in self-contained mode (§6.2) produces
byte-identical document-root encodings, and therefore identical value hashes. Encoding mode is
a transport choice; it does not affect document identity.

For a scalar whose `ScalarDefinition` declares an `encoding` (§20 / §21.7 of the TEL
Specification), the encoded value bytes — and therefore the value hash — are produced by the
bound codec. Codec laws C1–C4 (TEL §21.7) keep the hash a well-defined function of the semantic
model: two implementations binding the same encoding name for the same schema MUST produce
identical bytes (C4), and each accepted text has exactly one byte representation (C3).

The value hash of a schema document — the BLAKE3 digest over its full document-root encoding,
including any `layer` children — is distinct from the **component hashes** used in a schema
signature. A schema signature decomposes the schema into a base component (the schema document
with all `layer` children removed) and one component per layer; each component is encoded as a
standalone BinTEL document root and hashed separately. The two procedures and their distinct
purposes are described in §8.1.

When used in a schema identifier (see §8.1 of the TEL Specification), the ordered sequence of
component hashes (the base hash followed by each layer hash) is combined into a **schema
signature** per §8 below. The signature is encoded as [BASE-256](base256.md) for textual
representation. A schema with no layers has a single-component signature comprising the 32-byte
base-component hash followed by a one-byte cadence trailer (§8), giving 33 bytes total, encoded
as 33 BASE-256 characters.

### Normative Test Vector

The value hash of [`tels.tel`](../tels.tel) — the schema-for-schemas defined in §20.5
of the TEL Specification — is:

```
BLAKE3-256: d440b01e327c62c41ac641047f2c4d8df3cbe94abb24db33f189226b7b8b7ad3
BASE-256:   ÔŀưḞ2żbτȚÆAĄſЬMẍỳϋῩJλḤӛ3ñẉḢkŻẋzǓ
```

A conforming implementation that encodes the canonical `tels.tel` (1741 BinTEL bytes; raw
bytes recorded in [`demo/tels.bintel.hex`](../demo/tels.bintel.hex)) and hashes the
resulting document-root encoding MUST produce this value byte-for-byte. The same value appears
in §20.5 of the TEL Specification; the two specifications are pinned to this single vector.
`tels` declares no encodings, so this vector's derivation involves no codec.

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

Two properties are normative, so that whether a byte sequence is a valid BinTEL document is a
property of the bytes and not of the decoder that reads them:

- **Width.** The representable range is `[0, 2^64 − 1]`. A conforming decoder MUST accept every
  value in that range and MUST reject any encoding that exceeds it: an integer occupying more than
  ten bytes, or whose tenth byte is greater than `0x01`, is **B02**. Ten bytes carry 70 bits, of
  which only the low 64 are available.
- **Minimality.** An encoder MUST emit the shortest sequence representing the value — exactly the
  output of the algorithm above, in which no encoding ends in a `0x80` continuation byte carrying
  no information. A decoder MUST reject a non-minimal ("overlong") encoding, such as `80 00` for
  zero, as **B02**. Without this rule a single integer would have unboundedly many encodings, and
  neither the byte-determinism of §7 nor the value hash of §3 would be well defined.

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

A BinTEL document is a self-framing byte sequence: a **document length** field immediately after
the magic number gives its extent, so a reader knows where the document ends without decoding it.
A BinTEL document represents **exactly one** TEL semantic model and is **schema-bound**: every
BinTEL document carries a non-empty schema signature that identifies the schema used to interpret
its keyword indices. Untyped TEL documents (the absent-schema row of §8.2 of the TEL
Specification) cannot be encoded as BinTEL.

Because the extent is declared, the byte immediately following a document's last byte begins a
**continuation** — whatever the producer put there, which MAY be another BinTEL document, MAY be
content in some other format, and MAY be nothing at all. §6.3 defines how a reader treats it. This
mirrors the document-separator model of §6.1 of the TEL Specification, where the content following
a separator is likewise the caller's to interpret.

The length field is what makes the continuation usable. Without it the end of a document could be
found only by walking its whole structure — and that walk needs the *composed schema*, since
keyword indices determine each node's shape (§7.7). A reader that cannot resolve a document's
schema would therefore be unable even to skip it. With the length field, framing is
**schema-independent**: any reader can delimit, forward, count, or skip BinTEL documents while
resolving no schema at all, and the cost is constant rather than proportional to document size.

A BinTEL document MAY appear in one of two **modes**, distinguished by its leading magic number:

- **External-schema mode** (§6.1, magic `B2 C4 B5 BB`) — the document carries only a schema
  signature; the schema body itself MUST be obtained out-of-band via the resolution protocol of
  §8.2 of the TEL Specification.
- **Self-contained mode** (§6.2, magic `B2 C4 B5 BC`) — the document carries both the schema
  signature and the schema body inline. The embedded schema body is interpreted under the
  hardwired `tels` axiom (§20.5 of the TEL Specification); a receiver carrying only that
  axiom can fully decode a self-contained BinTEL document with no external resolution.

The two modes produce **identical document-root encodings** for the same semantic content and
composed schema. The value hash (§3) is therefore unchanged whether a document is encoded in
external-schema or self-contained mode — encoding mode is a transport choice and does not affect
document identity.

### 6.1 External-Schema Mode

A BinTEL document in external-schema mode consists of the following fields in order:

1. **Magic number**: the 4 bytes `B2 C4 B5 BB`. When the document is carried in BASE-256
   textual form (§9), these bytes appear as the four Greek letters at positions `0xB2`, `0xC4`,
   `0xB5`, and `0xBB` of the BASE-256 alphabet defined in the
   [BASE-256 Specification](base256.md) — namely the characters `β` (`U+03B2` Greek small
   beta), `τ` (`U+03C4` Greek small tau), `ε` (`U+03B5` Greek small epsilon), and `λ`
   (`U+03BB` Greek small lambda). An external-schema BinTEL document therefore begins with the
   literal string `βτελ` in BASE-256 textual form — visually evocative of "binary TEL" (`β` for
   binary, `τελ` the Greek root for *tel*-) and, because none of the bytes is below `0x80`,
   unlikely to be mistaken for the start of an ASCII or UTF-8 text file.
2. **Document length**: an integer (§4) giving the number of bytes that follow this field — that
   is, the combined length of fields 3 and 4. It does **not** include the magic number or the
   length field itself, so it needs no self-referential fixed point and an encoder computes it in
   a single pass. The full length of the document is `4 + len(varint) + document_length`, and the
   continuation (§6.3) begins at that offset. A decoder MUST verify that decoding fields 3 and 4
   consumes exactly `document_length` bytes; a disagreement between the declared and structural
   extents is a framing error (**B16**).
3. **Schema signature**: the byte length of the signature (integer), followed by the signature
   bytes. The schema signature (whose construction is defined in the Schema Signature section
   below) identifies the composed schema (base plus layers) used to type the document. The
   signature is a palimpsest at the BinTEL-pinned parameters `(H, k_i, k_r) = (32, 4, 2)` (see
   §8 and the [Palimpsest Specification](palimpsest.md)). The byte length MUST therefore satisfy
   either `length == 33` (for a schema with no layers, `n = 1`) or `length == 37 + 2·(n − 2)`
   for some `n ≥ 2` — equivalently, `length ∈ {33, 37, 39, 41, 43, …}` (33 alone for `n = 1`,
   then 37 and every odd integer above). Note that `length == 35` is **not** valid under these
   pinned parameters: the initial cadence `k_i = 4` introduces a one-time +4-byte step between
   `n = 1` (33 bytes) and `n = 2` (37 bytes), after which each additional layer adds `k_r = 2`
   bytes. A length of zero or any length not matching this pattern is a framing error (B03).
4. **Document root**: encoded using the node encoding described in the Node Encoding section
   below (root form). The encoding terminates exactly when the recursive procedure of §7.8 has
   consumed the last byte of the document root; there is no trailing tag or length within it.

The document ends at the last byte counted by field 2. A decoder MUST check the two extents
against each other (B16 on disagreement); whether any bytes beyond that point are an error
depends on the reading mode of §6.3.

### 6.2 Self-Contained Mode

A BinTEL document in self-contained mode consists of the following fields in order:

1. **Magic number**: the 4 bytes `B2 C4 B5 BC`. In BASE-256 textual form (§9), these bytes
   appear as the characters at positions `0xB2`, `0xC4`, `0xB5`, and `0xBC` of the BASE-256
   alphabet — `β` (`U+03B2`), `τ` (`U+03C4`), `ε` (`U+03B5`), and `μ` (`U+03BC` Greek small
   mu). A self-contained BinTEL document therefore begins with the literal string `βτεμ` —
   the trailing `μ` (for *monolithic*) distinguishes self-contained mode from external mode's
   `βτελ`. As with §6.1 every byte is `≥ 0x80`, so the document cannot be mistaken for ASCII
   or UTF-8 text.
2. **Document length**: identical in meaning to §6.1 field 2 — the number of bytes following this
   field, here the combined length of fields 3, 4 and 5 (**B16** on disagreement with the
   structural extent).
3. **Schema signature**: identical in structure and constraints to §6.1 field 3 — a length
   varint followed by a palimpsest at the BinTEL-pinned parameters `(H, k_i, k_r) = (32, 4, 2)`.
   The signature carried here MUST be the composed signature obtained from the embedded schema
   body (field 4 below) under §8. A decoder MUST recompute the signature from the embedded body
   and verify equality byte-for-byte; mismatch is fatal (B11).
4. **Embedded schema body**: the byte length of the schema body (integer), followed by that many
   bytes. The bytes are the bare document-root encoding (§7.1) of the schema document, with the
   root struct's member list taken to be `tels.document.members` (an axiomatic property
   of any conforming implementation, per §20.5 of the TEL Specification). No nested magic
   number and no nested signature appear: framing is provided by the outer schema_bytes_len
   varint, and the implicit governing schema is `tels`.

   The embedded schema body MAY contain `layer` compounds. A decoder reconstructs the composed
   schema by stripping the `layer` compounds to obtain the base schema, treating each `layer`
   compound as a TELS `Layer` Definition (§8.1), and applying the layers in source order
   per §20.3 of the TEL Specification.

   The embedded schema body is governed by `tels`, which declares no encodings; decoding
   the embedded body therefore never requires a codec binding, and the bootstrap of §7.8 is
   unaffected by codecs. Only the document root (field 5) may contain encoded scalars, governed
   by the schema the body defines.
5. **Document root**: encoded using the node encoding (§7.1), under the composed schema
   obtained from field 4. The bytes are identical to those of §6.1 field 4 for the same
   semantic content and the same composed schema; the embedded-schema preamble is the only
   wire-form difference between the two modes.

A conforming decoder MUST check the declared and structural extents against each other (B16 on
disagreement), and treats any bytes beyond the document per §6.3. A decoder MUST NOT begin
decoding the document root until the embedded schema's signature has been recomputed and
verified equal to the carried signature (B11 on mismatch); it MUST NOT emit a partial result
when verification fails.

A receiver that already has the embedded schema cached or known (e.g., the signature equals
the built-in `tels` signature, or matches an entry of an in-memory library) MAY skip
decoding the embedded body — advancing the cursor by `schema_bytes_len` bytes — and use the
known schema, provided it has previously verified that schema's signature.

### 6.3 Continuation and Document Streams

The document length of §6.1 field 2 / §6.2 field 2 fixes where a document ends. The bytes from
that point to the end of input are the **continuation**. BinTEL assigns the continuation no
meaning: it is the caller's to interpret, exactly as the content after a TEL document separator is
(§6.1 of the TEL Specification).

A conforming decoder MUST offer both of the following reading modes, and MUST make the extent of
the continuation available to its caller in both:

- **Single-document decoding** reads exactly one document, starting at the first byte of the
  input, and returns it together with the continuation — as a byte offset, a remaining-input
  slice, or an equivalent. Bytes beyond the document are neither decoded nor validated and need
  not be BinTEL. This is the supported way to prefix arbitrary, possibly non-BinTEL, content with
  a BinTEL header.

- **Stream decoding** yields the documents of the input in order. It is defined by recursion on
  single-document decoding: decode one document, then apply the same procedure to its
  continuation, until the continuation is empty. A continuation that is empty yields no further
  document; a continuation that is non-empty but does not begin with a recognised magic number is
  **B01** for that position.

Each document in a stream is independent. Documents in one stream MAY be in different modes (§6.1
and §6.2 may be interleaved freely) and MAY be typed by different schemas: every document carries
its own signature, and a self-contained document carries its own schema body. Nothing is inherited
from a preceding document.

A **whole-document** reader — one whose contract is that its input is exactly one document and
nothing else — MUST additionally reject a non-empty continuation as a framing error (**B08**).
This is a property of the reader's contract rather than of the byte sequence: the same bytes are a
well-formed two-document stream to a stream decoder and a B08 error to a whole-document reader,
just as trailing content is valid to a single-document TEL parser and a second document to a
streaming one.

Because framing is schema-independent (§6), a reader MAY traverse a stream — counting, skipping,
splitting or forwarding its documents — while resolving no schema and decoding no document root.
Only the magic number, the document length, and the signature length need be read to skip a
document, and none of the three depends on a schema.

## 7. Node Encoding

BinTEL encodes the **semantic model** of a TEL document (§18 of the TEL Specification). The
semantic model is a tree of `Element` values: `Node`s (Struct- or Flag-typed) and `Value`s
(Scalar-typed). Every presentation-layer atom and every presentation-layer compound contributes
exactly one element (§18.2 of the TEL Specification); the semantic model does not distinguish between an atom and a
compound that fill the same schema member.

### 7.1 Encoding by Element Type

**Document root.** The root is a virtual struct with no parent keyword. Its keyword order is the
keyword order of `Schema.document` (the root Struct of the composed schema, §20 of the TEL
Specification); every keyword index appearing among the root's children is a position in that
keyword order. The root is encoded as:

1. The number of root child nodes (integer).
2. Each root child node, in canonical order (§7.2), using the struct, scalar, or flag encoding
   below.

**Struct node** (schema type is `Struct`):

1. The keyword index of this node (integer).
2. The number of child nodes (integer).
3. Each child node, in canonical order (§7.2), using the struct, scalar, or flag encoding,
   recursively.

**Scalar node** (schema type is `Scalar`). Two forms, selected by the resolved `Scalar` type's
`encoding` (§20 and §21.7 of the TEL Specification). The schema determines the form; no tag is
encoded.

*Text scalar* (`encoding` is null):

1. The keyword index of this node (integer).
2. The byte length of the UTF-8 encoding of the value string (integer).
3. The UTF-8-encoded bytes of the value string.

*Encoded scalar* (`encoding` is non-null):

1. The keyword index of this node (integer).
2. The byte length of `encode(text)` — the bytes returned by the bound codec's encoder applied
   to the value string (integer).
3. Those bytes, verbatim.

The framing of the two forms is identical — a varint byte length followed by exactly that many
value bytes — so a value's extent in the stream is always determined by the length prefix
alone, without consulting any codec; only the interpretation of the value bytes differs. An
encoder MUST be configured with a codec binding (TEL §21.7) resolving every encoding named by
the composed schema; if any name is unresolved, or any value is rejected by its codec's encoder
(the E312 condition — the document is invalid), the encoder MUST fail without emitting a
document. BinTEL never encodes an invalid document.

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
2. For each member `mᵢ`, emit every element that fills `mᵢ`, in this order:
   - first, the **atom-derived elements**: each inline atom on the parent compound's line that the
     type assignment algorithm (§20.2 of the TEL Specification) assigned to `mᵢ`, in atom order;
   - then, the **compound-derived elements**: each compound child whose keyword corresponds to
     `mᵢ` (either `mᵢ.keyword` if `mᵢ` is a `Field`, or any `mᵢ.variants[j].keyword` if `mᵢ` is a
     `Select`), in source order.
3. **Defaults.** If `mᵢ` is a required `Field` with `Scalar` type, has a non-null `default`, and
   was not filled by any atom or compound child, emit a single Scalar element at `mᵢ`'s position
   carrying `mᵢ.type.default` as its value.

This is exactly the order in which the semantic model already holds a node's children (§18.3 of
the TEL Specification): canonical order **preserves** the semantic model's child order rather than
imposing a new one, which is why `bintel-decode(bintel-encode(M))` reproduces `M` as an identical
tree (property P2, §22.4 of the TEL Specification) and not merely an equivalent one.

Because every member contributes its elements consecutively and the relative ordering between
members follows the schema-defined member order, the encoding is independent of the source
ordering of independent member groups — two documents whose only difference is the order of
distinct member groups produce identical BinTEL bytes.

### 7.3 Atom-Derived Elements

Per §18.3 of the TEL Specification, an inline atom on a compound's line corresponds to a child
element of that compound. The element's type is the type assigned by the atom phase of §20.2 of the TEL Specification:

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
`Struct` during type assignment (§20.2 of that specification). Reference types do not appear in BinTEL: every node
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
presentation model (§18.1 of the TEL Specification) rather than in BinTEL.

A decoder receiving a Scalar node with a zero-length value MUST therefore treat it as a
semantically present Scalar with an empty text. There is no encoding for "absent Scalar with no
default": such a member is reported at type-assignment time as an E307 error against the
document, and BinTEL never encodes an invalid document.

For an *encoded* scalar the value bytes are `encode(text)`: empty *text* need not produce
empty *bytes*, and a zero-length value is well-formed only if the bound codec's decoder
accepts the empty byte sequence (law C3, TEL §21.7), in which case it denotes the unique text
that encodes to it — not necessarily the empty text. The explicit-empty/defaulted-empty
conflation above concerns the *text* and applies to encoded scalars unchanged.

### 7.6 Default Values

BinTEL encodes the semantic model, in which a required `Scalar` member with a non-null default
is semantically present even when it was absent from the source document. Therefore, when
encoding a document to BinTEL, a missing required scalar whose default is used MUST be encoded
as a scalar node with the default value string. The encoded **value string** is the
post-atom-form-decoded text — the `string` value returned by reading the default scalar's `text`
field from the parsed schema, *not* any atom-form bytes used to express it in the schema
source. Equivalent schemas that declare the default via different atom forms (inline atom,
source atom, or literal atom containing identical textual content) MUST therefore produce
byte-identical BinTEL encodings for the same missing-required-scalar case. This ensures the
BinTEL encoding is identical regardless of whether the member was explicitly written in the
document or filled by its default, and regardless of the atom form used by the schema author
to declare the default. The same principle applies to every Scalar value encoded by §7.1:
BinTEL preserves only the post-atom-decoded text, never atom-form presentation details.

When the member's type is an encoded scalar, the default text passes through the codec exactly
as an explicitly written value would: the encoded value bytes are `encode(default-text)`. A
default rejected by the codec renders any document relying on it invalid (E312, TEL §21.7),
and the encoder MUST fail rather than emit it. The atom-form-independence rule above is
unaffected: the codec consumes the post-atom-decoded text.

### 7.7 Framing

BinTEL frames at two levels, and they are independent. *Around* a document, the length field of
§6.1 field 2 delimits it, which requires no schema. *Within* a document, the schema delimits every
node, which requires no lengths beyond those already carried by scalar values. The outer framing is
what §6.3 relies on; the inner framing is described here.

There are no pad bytes, alignment constraints, or inter-node delimiters between the encoded
elements of a Struct's child list. The schema provides all type information needed to decode the
stream unambiguously: at each child position the decoder consults the parent's keyword order to
determine the child's type (Struct, Scalar, or Flag) — and, for a Scalar, whether an encoding
applies — from the next-read keyword index.

### 7.8 Decoding

A BinTEL decoder consumes the byte sequence defined in §6 and produces the semantic model defined
in §18 of the TEL Specification. The decoder dispatches on the leading magic number (§6.1 field 1
or §6.2 field 1); in external-schema mode it MUST have access to the resolved composed schema
before it begins reading the document root (the composed schema is obtained per §8.2 of the TEL
Specification), while in self-contained mode it obtains the composed schema from the embedded
schema body inline, using the hardwired `tels` axiom (§20.5 of the TEL Specification) as
its bootstrap.

When the composed schema names any encoding, the decoder MUST additionally be configured with
a codec binding (TEL §21.7); it SHOULD resolve each distinct encoding name once, before or upon
first use, and reuse the resolved codecs for every value. The pseudocode below treats
`codec-binding` as ambient configuration. `tels` declares no encodings, so the
self-contained-mode bootstrap never requires a codec.

The decoding algorithm is recursive. The pseudocode below treats `bytes` as a **stateful byte
cursor**: each `next N bytes`, `decode-varint(bytes)`, and similar operation advances the cursor;
reading past end-of-input raises B09, and any input bytes remaining when the document root
completes raise B08.

```
decode-document(bytes, schema_or_resolver):
  read magic = next 4 bytes
  if magic == [B2, C4, B5, BB]:        // external-schema mode (§6.1)
    mode = External
  elif magic == [B2, C4, B5, BC]:      // self-contained mode (§6.2)
    mode = SelfContained
  else: report error (B01)

  read document-length = decode-varint(bytes)
  if bytes-remaining() < document-length: report error (B09)
  body-start = cursor-position()

  read signature-length = decode-varint(bytes)
  read signature-bytes = next signature-length bytes
  verify signature length and cadence XOR per §8.2 (B03 on failure)

  if mode == SelfContained:
    read schema-bytes-length = decode-varint(bytes)
    read schema-bytes = next schema-bytes-length bytes
    // The embedded schema body is a bare document-root encoding under
    // TELS; its keyword indices and member layout are
    // axiomatic to any conforming implementation (§20.5 of the TEL spec).
    schema-doc = decode-struct-body(schema-bytes, tel_schema.document.members)
    if schema-doc is malformed: report error (B12)
    composed-schema = construct_schema_and_compose(schema-doc)  // TEL §20.3
    recomputed-sig = composed-signature(schema-doc) per §8
    if recomputed-sig != signature-bytes: report error (B11)
    schema = composed-schema
  else:
    // Resolution to a composed schema is handled at the §8.2 (TEL spec) layer;
    // this algorithm assumes the schema is already composed.
    schema = schema_or_resolver  // supplied by the caller

  root = decode-struct-body(bytes, schema.document.members)

  // §6.1 field 2: the declared and structural extents must agree exactly.
  if cursor-position() - body-start != document-length: report error (B16)

  // Everything from here to the end of input is the continuation (§6.3); it
  // is returned to the caller, not decoded. A whole-document reader rejects a
  // non-empty continuation as B08; a stream decoder recurses on it.
  return Document { signature: signature-bytes, root, mode,
                    continuation: bytes-from(cursor-position()) }

decode-struct-body(bytes, members):
  child-count = decode-varint(bytes)
  children = []
  repeat child-count times:
    children.push(decode-element(bytes, members))
  return children

decode-element(bytes, parent-members):
  kidx = decode-varint(bytes)
  if kidx >= keyword-count(parent-members): report error (B05)
  (keyword, type) = lookup-by-index(parent-members, kidx)
  resolved-type = resolve(type, schema)   // Reference resolution per TEL §20.2
  switch resolved-type:
    Struct(child-members):
      sub-children = decode-struct-body(bytes, child-members)
      return Struct-element { kidx, keyword, children: sub-children }
    Scalar(validators, encoding):
      value-length = decode-varint(bytes)
      value-bytes = next value-length bytes        // B06 if cursor advances past EOI
      if encoding == null:
        value-text = UTF-8-decode(value-bytes)     // B07 if not valid UTF-8
      else:
        codec = codec-binding(encoding)            // B13 if unresolved
        value-text = codec.decode(value-bytes)     // B14 on CodecFailure
        // OPTIONAL hardening (TEL §21.7): if codec.encode(value-text) != value-bytes: B15
      return Scalar-element { kidx, keyword, text: value-text }
    Flag:
      return Flag-element { kidx, keyword }
```

A decoder MUST NOT distinguish between an element that was encoded from an atom and one that was
encoded from a compound child: §7.2 makes the encoding canonical, so the source distinction is
not recoverable from the BinTEL stream.

Decoding does not re-run validators — BinTEL never encodes an invalid document, and the
decoder trusts the producer for validator conformance exactly as it does for text scalars; the
codec decode (B14) is the only value-level check applied to encoded scalars.

A decoder MAY stop after producing the semantic model; converting it to a presentation model is
outside BinTEL's scope (the source-level distinctions — atom form, remarks, comments, tabulation
— are not in BinTEL). The canonical text serialization defined in §22.3 of the TEL Specification
is one valid presentation form for a decoded semantic model.

## 8. Schema Signature

A schema signature identifies a composed schema as an ordered sequence of components: a base schema
followed by zero or more further components, each either a whole layer or a single **atom** — one
declaration of a layer or of the base, as defined in §20.3 of the TEL Specification. Each
component is identified by its value hash (§3).

A schema document (a TEL document conforming to the `tels` schema; see §20 of the TEL
Specification) defines a base schema and zero or more layers, and each of these decomposes
canonically into atoms. Each component's hash is its value hash (§3): the component is encoded as
a BinTEL document root (§7) and the 256-bit BLAKE3 digest is taken over that root encoding alone,
without the magic number or schema signature.

### 8.1 Per-Component Encoding

The base schema and each layer are encoded as standalone BinTEL document roots using §7. Both
cases reuse the entire composed `tels` namespace (every Definition reachable from
`Schema.records ∪ Schema.scalars ∪ Schema.selects`); only the root Struct differs:

- **Base-schema component** uses the root Struct that TELS itself declares — `Schema.document`
  of the `tels` schema, the Struct written as `tels.tel`'s top-level `document` block, whose
  keyword order is `name`, `sigil`, `record`, `scalar`, `select`, `document`, `layer`. (TELS
  declares no `Document` RecordDefinition; the record shared by the `document` and `overlay`
  *members* is `Body`, which is a different Struct and is not the root here.) The base schema's
  BinTEL encoding is produced by encoding the schema document **with all `layer` compounds
  removed**. That is:
  the encoded element list at the root contains the `name`, `sigil`, `record`, `scalar`,
  `select`, and `document` children, but no `layer` children, even when the original schema
  document declared layers. The base schema is the schema-without-layers.
- **Layer component** uses `Schema.document = Layer` (the TELS `Layer` RecordDefinition).
  The layer's BinTEL encoding treats the `layer` compound's children as the document root of a
  virtual schema whose `document` Struct is the `Layer` Definition from TELS and whose
  Definition namespace is inherited unchanged from the surrounding schema. Concretely: the
  encoded element list at the root contains the layer's `name`, each of its `record` /
  `scalar` / `select` children, and its `overlay` child (if present), in canonical order per
  §7.2. Keyword indices are computed against `Layer`'s keyword order.
- **Atom component** uses `Schema.document = Layer`, exactly as a layer component does, applied
  to a *virtual* `layer` compound whose `name` is the **empty string** and whose only other
  content is the atom under its path (§20.3 of the TEL Specification): a `record N`, `scalar N`,
  or `select N` compound containing only that atom's line(s), or an `overlay` compound
  containing only that member. The empty name encodes as its keyword index followed by `00`
  (§7.5). An `Identifier` cannot be empty (§20.7 of the TEL Specification), so no valid layer's
  encoding coincides with an atom's; and because the encoding mentions neither the containing
  layer nor the atom's position, an atom hashes identically wherever it appears — in a
  developer's working schema before release and in the released layer that later gathers it.
  The base schema's atoms are encoded the same way, its `document` members standing as `overlay`
  members, except for its **head atom** (`name` and `sigil`), which is encoded under
  `Schema.document = Document` as the schema document with every `record`, `scalar`, `select`,
  and `layer` child removed and an empty `document` body.

A conforming implementation of `schema-signature(schema-document)` therefore:

1. Constructs the base-schema document (the schema document minus its `layer` compounds) and
   computes h₀ = BLAKE3-256 of its document-root BinTEL encoding.
2. For each `layer` compound L_i in source order, computes h_{i+1} = BLAKE3-256 of the
   document-root BinTEL encoding of L_i's children under the `Layer` Definition.
3. Combines the sequence (h₀, h₁, …, h_n) into the palimpsest signature per §8.2 below.

A document that names atoms rather than whole layers (§8.1 of the TEL Specification) substitutes
the atom hashes for the corresponding layer hash in step 3; the group hashes of steps 1–2 are
unaffected, and a library that holds a layer can derive the hashes of its atoms.

### 8.2 Signature Construction

A schema signature is a **palimpsest** as defined in the
[Palimpsest Specification](palimpsest.md), constructed at the BinTEL-pinned parameters
`(H, k_i, k_r) = (32, 4, 2)` — equivalently, hash-size index `s = 7`, regular cadence 2 bytes,
initial cadence 4 bytes. The palimpsest framework permits any combination of these parameters;
this specification pins them so that producers and consumers can statically reason about
signature sizes. The pinned values are sufficient for schema libraries of up to `2^32 ≈ 4 × 10^9`
distinct base components without backtracking during decode of the base hash, while keeping
signature size growth to two bytes per additional component, whether a layer or an atom.

**Encoding.** Given an ordered sequence of `n` component hashes `h₀, h₁, …, h_{n−1}` (each
32 bytes, BLAKE3-256), the signature is computed as the palimpsest of those hashes per §4 of
the Palimpsest Specification, with the cadence byte for `(s, k_i − k_r, k_r − 1) = (7, 2, 1)`.
Concretely:

1. Compute the offsets `oᵢ`: `o₀ = 0`, and for `i ≥ 1`, `oᵢ = 4 + 2·(i − 1)` — i.e. the
   sequence `0, 4, 6, 8, 10, …`.
2. Allocate a zero-filled byte array `B` of length `L_data`, where `L_data = 32` if `n = 1` and
   `L_data = 32 + 4 + 2·(n − 2) = 36 + 2·(n − 2)` otherwise.
3. For each `i`, XOR `hᵢ` into `B` at offset `oᵢ`.
4. Form the cadence byte `c` by packing `(s, k_i − k_r, k_r − 1) = (7, 2, 1)`. Bit-by-bit:
   bits 0–1 = `01` (`k_r − 1 = 1`), bits 2–3 = `10` (`k_i − k_r = 2`), bits 4–7 = `0111`
   (`s = 7`). The byte's value is `0x79`.
5. Compute `D = XOR(B[0..L_data − 1])` and append the trailing byte `z = D ⊕ 0x79`.

The signature is `B` followed by `z`, a total of `L_data + 1` bytes. For `n = 1` the signature
is the 32-byte value hash of the base schema followed by the cadence trailer (33 bytes total).
For `n ≥ 2` the signature is `36 + 2·(n − 2) + 1 = 37 + 2·(n − 2)` bytes (37, 39, 41, … for
`n = 2, 3, 4, …`).

**Worked examples.**

- `n = 1`: signature is `h₀[0..31] ‖ z`, where `z = (h₀[0] ⊕ h₀[1] ⊕ … ⊕ h₀[31]) ⊕ 0x79`.
  Length: 33 bytes.
- `n = 3`: body is the XOR of three padded hashes at offsets `0, 4, 6`, length
  `32 + 4 + 2 = 38` bytes; total signature length is 39 bytes.

**Textual form.** When a schema signature appears in textual contexts — most notably the
signature phrase of a TEL pragma (see §8.1 of the TEL Specification) — it is encoded with
[BASE-256](base256.md), producing one Unicode character per signature byte. BASE-256 is chosen
over BASE64-URL or hex because (a) it is the most compact character-per-byte encoding
available — half the length of hex; (b) every character is a Unicode letter or digit, so the
encoded signature is a single word for double-click selection (per Unicode Annex #29); and (c)
the alphabet contains no whitespace or punctuation, so the signature always occupies a single
phrase on the pragma line. Encoders and decoders use the alphabet defined in §4 of the BASE-256
Specification.

**Correctness property.** Decodability rests on the structural property of §4.3 of the
Palimpsest Specification: the first `k_i = 4` bytes of the body equal `h₀[0..3]` uncontested,
and after `h₀` is XORed out, the bytes at offset `o₁ = 4` for the next `k_r = 2` positions equal
`h₁[0..1]` uncontested, and so on. Decoding therefore proceeds deterministically as long as no
two hashes in the candidate library share the same first 4 bytes (for the base lookup) or the
same first 2 bytes (for the lookups within a single base's lineage of layers and atoms); see §6
of the Palimpsest Specification for the probabilistic analysis.

**Decoding.** Given a signature of known byte length `L` and a set of candidate hashes:

1. From `L`, compute `n`: `n = 1` if `L = 33`; otherwise require `L ≥ 37` and
   `(L − 37) mod 2 = 0`, and set `n = 2 + (L − 37) / 2`. Any other `L` is a framing error
   (**B03**).
2. XOR every byte of the signature; the result MUST equal `0x79` (the BinTEL-pinned cadence
   byte). If it does not, that too is **B03**. Both checks are structural and both report B03;
   B04 is reserved for the library-decode failure of step 4, and is never reported for a
   malformed length or a bad cadence byte.
3. Treating bytes `[0..L − 2]` as the palimpsest body, run the recursive search of §5.3 of the
   Palimpsest Specification with `(H, k_i, k_r) = (32, 4, 2)`. Candidates at step 0 are looked
   up by 4-byte prefix; at every subsequent step by 2-byte prefix. A decoder SHOULD draw the
   candidates for the steps after the first from the components of the **lineage** of the base
   recovered at step 0 — its declared layers, their atoms, and any locally registered atoms
   written against it — rather than from its whole library: two bytes distinguish at most
   65 536 prefixes, and once a library of atoms approaches that size the backtracking search
   over a global candidate set grows as `(N / 65 536)ⁿ` (§3.2 of the Palimpsest Specification).
4. If the search returns a valid sequence, decoding succeeds. If no valid sequence is found
   against the candidate library, the signature is malformed (B04). The "more than one valid
   sequence" case requires a BLAKE3 collision among components — a second-preimage attack on
   BLAKE3-256 — and is computationally infeasible under the collision-resistance assumption of
   §8 of the Palimpsest Specification (see also its §10); a decoder that nevertheless encounters multiple satisfying
   sequences MUST also report B04 (treating it as a corruption or integrity failure rather than
   as a regular decoding outcome).

The decoded sequence gives the component hashes in order: h₀ (base schema), then h₁ … h_{n−1}
(layers or atoms, in composition order). A BinTEL decoder uses this sequence to locate and
compose the schema (§20.3 of the TEL Specification) before decoding the document root.

Schema compatibility is defined in §8.2 of the TEL Specification as the subtype relation between
the two *composed* schemas (§24.3 there); no relation between the decoded hash sequences decides
it.

### 8.3 Schema Exchange Between Peers

*This subsection is informative.* It describes how the normative machinery above behaves when
BinTEL documents are exchanged between two parties — a client and a server, or any pair of
peers — whose libraries of schema components differ. BinTEL itself defines no handshake,
request, or negotiation message: the carrier for any exchange described here is the embedding
protocol's concern (§6), and this specification supplies only the vocabulary — signatures,
component hashes, and the subtype check.

**Two independent questions.** Whether a receiver can process a document tagged with signature
`S_doc` splits into:

1. **Decodability.** The document root can be read only under the exact composition `S_doc`
   names, because keyword indices are positions in that composition's keyword order (§5,
   §7.7) and a layer may shift them. In external-schema mode (§6.1) every component hash of
   `S_doc` must therefore resolve — from the receiver's library, cache, or LIRA (§8.2 of the
   TEL Specification) — or decoding fails. In self-contained mode (§6.2) the schema body is
   inline, and a receiver holding only the `tels` axiom can decode any document at all.
2. **Compatibility.** Having decoded, the receiver checks `S_doc <: S_cons` against its own
   invocation schema `S_cons`: the composed schema of `S_doc` must be a subtype of the composed
   schema of `S_cons` by the rules of §24.3 of the TEL Specification (§8.2 there). The direction
   matters: a receiver may consume documents composed with *more* constraints than it expects
   (by projection, §24.5 of the TEL Specification), never fewer; a document that omits an
   optional member the receiver knows about is still a subtype.

**Worked example.** A receiver whose library holds a base schema `foo` and layers `bar` and
`baz` receives documents in external-schema mode:

| Document composition | Decodable? | Compatible with `S_cons = foo`? | Compatible with `S_cons = foo+bar+baz`? |
| -------------------- | ---------- | ------------------------------- | --------------------------------------- |
| `foo`                | yes        | yes (matching)                  | no                                      |
| `foo+bar`            | yes        | yes                             | no                                      |
| `foo+baz`            | yes        | yes                             | no                                      |
| `foo+bar+baz`        | yes        | yes                             | yes (matching)                          |
| `foo+quux`           | no         | —                               | —                                       |

The last row becomes decodable, and compatible with `S_cons = foo`, if the sender uses
self-contained mode. The right-hand column shows why a receiver's invocation schema should be
its *minimum* requirement: a receiver that demands `foo+bar+baz` rejects every document that
omits an optional layer, whereas one that demands `foo` and exploits `bar` and `baz` when the
decoded composition contains them accepts all four decodable rows.

**Receiver guidance.** State the shortest composition you need as the invocation schema. Treat
every further layer in your library as opportunistic: after decoding under `S_doc`, the
members of any layer present in `S_doc` are available in the semantic model, and projection to
`S_cons` discards only what you cannot address anyway.

**Sender guidance.** Send the richest composition the receiver can resolve. A sender holding a
value under `S` can always degrade it to any valid composition `S'` of which `S` is a subtype —
in particular any composition that `S` extends — by projecting (§24.5 of the TEL Specification)
and re-encoding under `S'`; degradation is always available, so the only question is how the
sender learns which `S'` the receiver can resolve.

**Partial decode as a diagnostic.** The palimpsest decode of §8.2 recovers components in order
— `h₀` by four-byte prefix, each subsequent layer by two-byte prefix — and a failure at step
`i` therefore tells the receiver that it holds components `0 … i−1` and lacks component `i`.
The signature alone thus identifies the longest prefix of the sender's composition that the
receiver can name back, which is the natural content of a "please degrade" reply.

**Mechanisms, in increasing cost.** All three sit entirely within the embedding protocol:

- *Self-contained first message.* The sender uses self-contained mode until the receiver
  confirms, by whatever means the embedding provides, that it now holds the signature; the
  receiver caches the verified schema (§8.2 of the TEL Specification, Caching) and both sides
  switch to external-schema mode. No negotiation round-trip is needed; the cost is the schema
  body on early messages. This is the recommended default for peers that cannot assume a
  shared library.
- *Signature probe.* The sender transmits only its intended signature (33–41 bytes for typical
  compositions). The receiver runs the palimpsest decode and replies with the longest prefix it
  holds, or with the signature of the composition it prefers. The sender then degrades to that
  composition. One round-trip; no schema bytes.
- *Capability exchange.* Each peer advertises the component hashes (or full signatures) it
  holds, once per connection. Each sender then picks the richest composition it can form from
  components the receiver holds and of which its own composition is a subtype. Suits long-lived
  sessions with many messages.

None of these changes the wire format of a BinTEL document, and none is required: a receiver
that can resolve through LIRA, or a sender that always uses self-contained mode, needs no
exchange at all.

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
| B01  | Magic number absent or does not match either of the recognised values `B2 C4 B5 BB` (external-schema mode, BASE-256: `βτελ`, §6.1 field 1) or `B2 C4 B5 BC` (self-contained mode, BASE-256: `βτεμ`, §6.2 field 1). |
| B02  | A variable-length integer (§4) extends beyond the end of input, exceeds the pinned range `[0, 2^64 − 1]`, or is not in the minimal form §4 requires. |
| B03  | Schema signature length is not `33` (for `n = 1`) and not `37 + 2·(n − 2)` for any `n ≥ 2` (§6 field 2), **or** the XOR of every signature byte does not equal `0x79` — the BinTEL-pinned cadence byte (§8.2). |
| B04  | Schema signature does not decode against the available hash library (§8.2 decoding); zero or more than one valid hash sequence. |
| B05  | A keyword index read from the stream is outside `[0, keyword-count(parent-members))` (§7.8). |
| B06  | A Scalar value's byte length extends beyond the end of input.                                 |
| B07  | A **text** Scalar value's UTF-8 bytes are not a valid UTF-8 sequence (does not apply to encoded scalars, whose value bytes are codec-defined). |
| B08  | A **whole-document** reader (§6.3) found a non-empty continuation — bytes remaining after the document ended. This is a property of the reader's contract, not of the bytes: a stream decoder treats the same bytes as the next document, and a single-document decoder returns them to its caller. |
| B09  | The document-root decoding procedure of §7.8 requests bytes beyond end of input.              |
| B10  | A `Reference` or `SelectRef` in the composed schema does not resolve usably: either it names no `Definition` at all (the E209 condition at parse time), or it resolves to a `Definition` of the wrong kind — a `Reference` to a `SelectDefinition`, or a `SelectRef` to a record or scalar (the E217 condition). Both are surfaced by the decoder as a configuration error: the composed schema it was given is malformed, so keyword indices cannot be interpreted. |
| B11  | In self-contained mode (§6.2), the composed signature recomputed from the embedded schema body does not equal the carried signature byte-for-byte. |
| B12  | In self-contained mode (§6.2), the embedded schema body does not decode as a valid TEL document under `tels` (structural error during bootstrap; the bytes do not yield a well-formed schema document). |
| B13  | The composed schema declares an `encoding` for a Scalar but the decoder's codec binding (TEL §21.7) does not resolve that name. |
| B14  | An encoded Scalar's value bytes are rejected by the bound codec's decoder — the bytes are not the encoding of any accepted text (including corrupt or non-canonical bytes, per law C3 of TEL §21.7). |
| B15  | An implementation performing the OPTIONAL re-encode verification of TEL §21.7 found `encode(decode(b)) ≠ b` for an encoded Scalar's value bytes — a canonicality violation indicating a non-conforming codec or corrupted input. |
| B16  | The document length declared in §6.1 field 2 / §6.2 field 2 disagrees with the extent the structural decode actually consumed. The two framings of the same document contradict each other, so neither can be trusted. |

**Precedence among the end-of-input codes.** B02, B06 and B09 all describe a decoder running out
of input, and without a rule two conforming decoders would report different codes for identical
bytes. The most specific applicable code MUST be reported:

1. **B06** when a Scalar value's already-decoded byte length extends past the end of input.
2. **B02** when the truncation falls inside a variable-length integer (§4), or when that integer
   violates the width or minimality rules of §4.
3. **B09** otherwise — any other request for bytes beyond end of input.

The same ordering applies in self-contained mode to the embedded schema body: a truncated
`schema_bytes_len` is B02, a body shorter than that length is B09. A document whose declared
length (§6.1 field 2) exceeds the bytes remaining is likewise B09 — the document is truncated —
whereas a declared length that disagrees with a *successful* structural decode is B16.

All BinTEL error codes (B01–B16) are **fatal**: on any such error a conforming decoder MUST
abort decoding and MUST NOT emit any partial result. BinTEL is the authoritative serialisation
of the semantic model — once any byte is found inconsistent with §6 / §7, no remaining bytes
can be trusted to convey their nominal types and lengths. No recovery is specified for BinTEL.

B13 merits a remark: because encoded-scalar framing is length-prefixed (§7.1), a decoder
*structurally could* skip an unknown codec's value bytes and continue. For semantic decoding
this is deliberately forbidden — a schema that names an encoding expresses both a
representation and a constraint, and a consumer that cannot decode the representation cannot
deliver the value (the TEL §21.4/§21.7 unknown-helper principle). Purely structural tools that
never materialise values (e.g. a verbatim re-framer) MAY traverse value bytes opaquely, as
they already do for text scalars.

A decoder MAY perform additional consistency checks beyond those above (for example, checking
that a Struct-typed child's claimed type is actually a Struct after Reference resolution); these
are implementation-specific and are not error conditions defined by this specification.

## 11. Security Considerations

A BinTEL document is a compact, self-framing binary format whose structure is driven entirely by
counts and lengths read from the input. A decoder consuming untrusted bytes MUST therefore treat
every such count as adversarial.

**Resource limits.** The decoding procedure of §7.8 is recursive and its loops are bounded by
values taken from the stream. A conforming decoder MUST bound the following, and MUST fail rather
than exhaust memory or stack:

- **Nesting depth.** `decode-struct-body` recurses once per level of Struct nesting. A decoder
  MUST enforce a maximum depth; the RECOMMENDED limit is **256**, matching the type-assignment
  limit of §20.2 of the TEL Specification, so that every document a conforming TEL parser accepts
  is also decodable.
- **Child counts.** A decoder MUST NOT pre-allocate storage proportional to a declared child
  count before reading the children. Each child consumes at least one byte (a keyword index), so a
  count exceeding the number of bytes remaining is unsatisfiable and can be rejected immediately.
- **Value lengths.** A Scalar value's declared byte length MUST be checked against the bytes
  remaining before any allocation (**B06** when it overruns).
- **Document length.** The document length of §6.1 field 2 is as attacker-controlled as any other
  count. A decoder MUST check it against the bytes remaining before acting on it (**B09** when it
  overruns) and MUST NOT pre-allocate a buffer sized by it. Because it is *declared* rather than
  derived, it MUST NOT be trusted in place of the structural decode: the check that the two agree
  (**B16**) is what stops a forged length from concealing bytes inside a document or exposing
  bytes of the next one.

These limits are properties of a decoder's configuration, not of the document, so exceeding one is
a resource error reported outside the B01–B16 taxonomy — exactly as §20.2 of the TEL Specification
treats its depth limit. A decoder with a larger limit would accept the same bytes. Implementations
MAY expose the limits as configurable parameters.

**Self-contained mode carries executable structure.** In self-contained mode (§6.2) the embedded
schema body determines how every subsequent byte is interpreted, and it arrives from the same
untrusted source as the document. The signature check of B11 confirms only that the body is
*internally consistent* with the signature the document carries — a self-consistent forgery passes
it. A receiver that requires the schema to be one it trusts MUST compare the verified signature
against a set of known schema signatures, and MUST NOT infer trust from successful decoding. The
depth and count limits above apply to the embedded body as well as to the document root.

**A signature is an identifier, not an authenticator.** The schema signature of §8 is a
content-derived identity: it binds a document to a composed schema and detects accidental
corruption, but it carries no key and proves nothing about the origin of the bytes. Authenticity
and integrity against a motivated adversary MUST be supplied at a separate layer.

**Codecs run on attacker-controlled bytes.** A bound codec's `decode` (§7.1) is invoked on value
bytes taken directly from the input. Codec implementations are as exposed as the decoder itself
and MUST be written accordingly; the OPTIONAL re-encode verification (B15) detects
non-canonical input but is not a substitute for a codec that handles malformed bytes safely.

**Streams are unbounded.** A reader consuming a stream (§6.3) accepts an arbitrary number of
documents, each of which may be self-contained and so carry its own schema. A reader accepting a
stream from an untrusted source SHOULD bound the document count, the total bytes consumed, and the
number of distinct schemas it will admit; none of these bounds is expressible in the format, and
all are the reader's responsibility. Skipping a document by its declared length is cheap, but
*decoding* every document in a stream is not.

**BinTEL provides no confidentiality.** The encoding is a transparent serialisation; BASE-256
(§9) is likewise an encoding and not encryption. Neither conceals any part of the document.
