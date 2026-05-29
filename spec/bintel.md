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
index `s = 7` of the [Palimpsest Specification](palimpsest.md) (§2.1).

The value hash is **mode-independent**: encoding the same semantic content under the same
composed schema in external-schema mode (§6.1) and in self-contained mode (§6.2) produces
byte-identical document-root encodings, and therefore identical value hashes. Encoding mode is
a transport choice; it does not affect document identity.

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

The value hash of [`tel-schema.tel`](tel-schema.tel) — the schema-for-schemas defined in §20.5
of the TEL Specification — is:

```
BLAKE3-256: a9d3e1ed5843888b8c077a12609252e81e62dcccaf9d2cc074bd4d5ec10613ba
BASE-256:   ΩǓῡíXCẈẋẌćzĒŠƒRǨḞbӜỌίƝЬπtẽMŞӁĆГκ
```

A conforming implementation that encodes the canonical `tel-schema.tel` (887 BinTEL bytes; raw
bytes recorded in [`demo/tel-schema.bintel.hex`](demo/tel-schema.bintel.hex)) and hashes the
resulting document-root encoding MUST produce this value byte-for-byte. The same value appears
in §20.5 of the TEL Specification; the two specifications are pinned to this single vector.

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

A BinTEL document is a self-contained byte sequence whose total length is determined by parsing
its fields; there is no top-level length prefix. A BinTEL document represents **exactly one** TEL
semantic model and is **schema-bound**: every BinTEL document carries a non-empty schema
signature that identifies the schema used to interpret its keyword indices. Untyped TEL
documents (the absent-schema row of §8.2 of the TEL Specification) cannot be encoded as BinTEL.
A byte sequence consisting of two or more concatenated BinTEL documents is not a conforming
BinTEL document; a producer MUST NOT emit such a sequence and a decoder MUST treat any bytes
following a complete document root as a framing error (§10) rather than as a second document.
Any embedding that needs to multiplex BinTEL documents over a shared channel is responsible for
framing at its own layer.

A BinTEL document MAY appear in one of two **modes**, distinguished by its leading magic number:

- **External-schema mode** (§6.1, magic `B2 C4 B5 BB`) — the document carries only a schema
  signature; the schema body itself MUST be obtained out-of-band via the resolution protocol of
  §8.2 of the TEL Specification.
- **Self-contained mode** (§6.2, magic `B2 C4 B5 BC`) — the document carries both the schema
  signature and the schema body inline. The embedded schema body is interpreted under the
  hardwired `tel-schema` axiom (§20.5 of the TEL Specification); a receiver carrying only that
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
2. **Schema signature**: the byte length of the signature (integer), followed by the signature
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
3. **Document root**: encoded using the node encoding described in the Node Encoding section
   below (root form). The encoding terminates exactly when the recursive procedure of §7.8 has
   consumed the last byte of the document root; there is no trailing tag or length.

A conforming decoder MUST verify that all bytes of the input are consumed by this procedure. Any
bytes following the document root are a framing error (§10).

### 6.2 Self-Contained Mode

A BinTEL document in self-contained mode consists of the following fields in order:

1. **Magic number**: the 4 bytes `B2 C4 B5 BC`. In BASE-256 textual form (§9), these bytes
   appear as the characters at positions `0xB2`, `0xC4`, `0xB5`, and `0xBC` of the BASE-256
   alphabet — `β` (`U+03B2`), `τ` (`U+03C4`), `ε` (`U+03B5`), and `μ` (`U+03BC` Greek small
   mu). A self-contained BinTEL document therefore begins with the literal string `βτεμ` —
   the trailing `μ` (for *monolithic*) distinguishes self-contained mode from external mode's
   `βτελ`. As with §6.1 every byte is `≥ 0x80`, so the document cannot be mistaken for ASCII
   or UTF-8 text.
2. **Schema signature**: identical in structure and constraints to §6.1 field 2 — a length
   varint followed by a palimpsest at the BinTEL-pinned parameters `(H, k_i, k_r) = (32, 4, 2)`.
   The signature carried here MUST be the composed signature obtained from the embedded schema
   body (field 3 below) under §8. A decoder MUST recompute the signature from the embedded body
   and verify equality byte-for-byte; mismatch is fatal (B11).
3. **Embedded schema body**: the byte length of the schema body (integer), followed by that many
   bytes. The bytes are the bare document-root encoding (§7.1) of the schema document, with the
   root struct's member list taken to be `tel-schema.document.members` (an axiomatic property
   of any conforming implementation, per §20.5 of the TEL Specification). No nested magic
   number and no nested signature appear: framing is provided by the outer schema_bytes_len
   varint, and the implicit governing schema is `tel-schema`.

   The embedded schema body MAY contain `layer` compounds. A decoder reconstructs the composed
   schema by stripping the `layer` compounds to obtain the base schema, treating each `layer`
   compound as a tel-schema `Layer` Definition (§8.1), and applying the layers in source order
   per §20.3 of the TEL Specification.
4. **Document root**: encoded using the node encoding (§7.1), under the composed schema
   obtained from field 3. The bytes are identical to those of §6.1 field 3 for the same
   semantic content and the same composed schema; the embedded-schema preamble is the only
   wire-form difference between the two modes.

A conforming decoder MUST verify that all bytes of the input are consumed by this procedure;
any bytes following the document root are a framing error (B08). A decoder MUST NOT begin
decoding the document root until the embedded schema's signature has been recomputed and
verified equal to the carried signature (B11 on mismatch); it MUST NOT emit a partial result
when verification fails.

A receiver that already has the embedded schema cached or known (e.g., the signature equals
the built-in tel-schema signature, or matches an entry of an in-memory library) MAY skip
decoding the embedded body — advancing the cursor by `schema_bytes_len` bytes — and use the
known schema, provided it has previously verified that schema's signature.

## 7. Node Encoding

BinTEL encodes the **semantic model** of a TEL document (§18 of the TEL Specification). The
semantic model is a tree of `Element` values: `Node`s (Struct- or Flag-typed) and `Value`s
(Scalar-typed). Every presentation-layer atom and every presentation-layer compound contributes
exactly one element (§18.2); the semantic model does not distinguish between an atom and a
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

### 7.7 Framing

There are no pad bytes, alignment constraints, or inter-node delimiters between the encoded
elements of a Struct's child list. The schema provides all type information needed to decode the
stream unambiguously: at each child position the decoder consults the parent's keyword order to
determine the child's type (Struct, Scalar, or Flag) from the next-read keyword index.

### 7.8 Decoding

A BinTEL decoder consumes the byte sequence defined in §6 and produces the semantic model defined
in §18 of the TEL Specification. The decoder dispatches on the leading magic number (§6.1 field 1
or §6.2 field 1); in external-schema mode it MUST have access to the resolved composed schema
before it begins reading the document root (the composed schema is obtained per §8.2 of the TEL
Specification), while in self-contained mode it obtains the composed schema from the embedded
schema body inline, using the hardwired `tel-schema` axiom (§20.5 of the TEL Specification) as
its bootstrap.

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

  read signature-length = decode-varint(bytes)
  read signature-bytes = next signature-length bytes
  verify signature length and cadence XOR per §8.2 (B03 on failure)

  if mode == SelfContained:
    read schema-bytes-length = decode-varint(bytes)
    read schema-bytes = next schema-bytes-length bytes
    // The embedded schema body is a bare document-root encoding under
    // tel-schema; tel-schema's keyword indices and member layout are
    // axiomatic to any conforming implementation (§20.5 of the TEL spec).
    schema-doc = decode-struct-body(schema-bytes, tel_schema.document.members)
    if schema-doc is malformed: report error (B12)
    composed-schema = construct_schema_and_compose(schema-doc)  // §20.3
    recomputed-sig = composed-signature(schema-doc) per §8
    if recomputed-sig != signature-bytes: report error (B11)
    schema = composed-schema
  else:
    // Resolution to a composed schema is handled at the §8.2 (TEL spec) layer;
    // this algorithm assumes the schema is already composed.
    schema = schema_or_resolver  // supplied by the caller

  root = decode-struct-body(bytes, schema.document.members)
  if bytes-remaining(): report error (B08)
  return Document { signature: signature-bytes, root, mode }

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
  resolved-type = resolve(type, schema)   // Reference resolution per §20.2
  switch resolved-type:
    Struct(child-members):
      sub-children = decode-struct-body(bytes, child-members)
      return Struct-element { kidx, keyword, children: sub-children }
    Scalar:
      value-length = decode-varint(bytes)
      value-bytes = next value-length bytes        // B06 if cursor advances past EOI
      value-text = UTF-8-decode(value-bytes)       // B07 if not valid UTF-8
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
hash (§3): the component is encoded as a BinTEL document root (§7) and the 256-bit BLAKE3 digest
is taken over that root encoding alone, without the magic number or schema signature.

### 8.1 Per-Component Encoding

The base schema and each layer are encoded as standalone BinTEL document roots using §7. Both
cases reuse the entire composed `tel-schema` namespace (every Definition reachable from
`Schema.records ∪ Schema.scalars ∪ Schema.selects`); only the root Struct differs:

- **Base-schema component** uses `Schema.document = Document` (the full schema-document root,
  per the tel-schema `Document` RecordDefinition). The base schema's BinTEL encoding is
  produced by encoding the schema document **with all `layer` compounds removed**. That is:
  the encoded element list at the root contains the `name`, `sigil`, `record`, `scalar`,
  `select`, and `document` children, but no `layer` children, even when the original schema
  document declared layers. The base schema is the schema-without-layers.
- **Layer component** uses `Schema.document = Layer` (the tel-schema `Layer` RecordDefinition).
  The layer's BinTEL encoding treats the `layer` compound's children as the document root of a
  virtual schema whose `document` Struct is the `Layer` Definition from tel-schema and whose
  Definition namespace is inherited unchanged from the surrounding schema. Concretely: the
  encoded element list at the root contains the layer's `name`, each of its `record` /
  `scalar` / `select` children, and its `overlay` child (if present), in canonical order per
  §7.2. Keyword indices are computed against `Layer`'s keyword order.

A conforming implementation of `schema-signature(schema-document)` therefore:

1. Constructs the base-schema document (the schema document minus its `layer` compounds) and
   computes h₀ = BLAKE3-256 of its document-root BinTEL encoding.
2. For each `layer` compound L_i in source order, computes h_{i+1} = BLAKE3-256 of the
   document-root BinTEL encoding of L_i's children under the `Layer` Definition.
3. Combines the sequence (h₀, h₁, …, h_n) into the palimpsest signature per §8.2 below.

### 8.2 Signature Construction

A schema signature is a **palimpsest** as defined in the
[Palimpsest Specification](palimpsest.md), constructed at the BinTEL-pinned parameters
`(H, k_i, k_r) = (32, 4, 2)` — equivalently, hash-size index `s = 7`, regular cadence 2 bytes,
initial cadence 4 bytes. The palimpsest framework permits any combination of these parameters;
this specification pins them so that producers and consumers can statically reason about
signature sizes. The pinned values are sufficient for schema libraries of up to `2^32 ≈ 4 × 10^9`
distinct base components without backtracking during decode of the base hash, while keeping
signature size growth to two bytes per additional layer.

**Encoding.** Given an ordered sequence of `n` component hashes `h₀, h₁, …, h_{n−1}` (each
32 bytes, BLAKE3-256), the signature is computed as the palimpsest of those hashes per §3 of
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

**Textual form.** When a schema signature appears in textual contexts — most notably the schema
identifier of a TEL pragma (see §8.1 of the TEL Specification) — it is encoded with
[BASE-256](base256.md), producing one Unicode character per signature byte. BASE-256 is chosen
over BASE64-URL or hex because (a) it is the most compact character-per-byte encoding
available — half the length of hex; (b) every character is a Unicode letter or digit, so the
encoded signature is a single word for double-click selection (per Unicode Annex #29); and (c)
the alphabet contains no whitespace or punctuation, so the signature always occupies a single
phrase on the pragma line. Encoders and decoders use the alphabet defined in §4 of the BASE-256
Specification.

**Correctness property.** Decodability rests on the structural property of §3.3 of the
Palimpsest Specification: the first `k_i = 4` bytes of the body equal `h₀[0..3]` uncontested,
and after `h₀` is XORed out, the bytes at offset `o₁ = 4` for the next `k_r = 2` positions equal
`h₁[0..1]` uncontested, and so on. Decoding therefore proceeds deterministically as long as no
two hashes in the candidate library share the same first 4 bytes (for the base lookup) or the
same first 2 bytes (for layer lookups within a single base's reachable layers); see §5 of the
Palimpsest Specification for the probabilistic analysis.

**Decoding.** Given a signature of known byte length `L` and a set of candidate hashes:

1. XOR every byte of the signature; the result MUST equal `0x79` (the BinTEL-pinned cadence
   byte). If it does not, treat this as a framing error (B03 if the byte length is otherwise
   plausible, otherwise B04 by extension).
2. From `L`, compute `n`: `n = 1` if `L = 33`; otherwise require `L ≥ 37` and
   `(L − 37) mod 2 = 0`, and set `n = 2 + (L − 37) / 2`. Any other `L` is a framing error
   (B03).
3. Treating bytes `[0..L − 2]` as the palimpsest body, run the recursive search of §4.3 of the
   Palimpsest Specification with `(H, k_i, k_r) = (32, 4, 2)`. Candidates at step 0 are looked
   up by 4-byte prefix; at every subsequent step by 2-byte prefix.
4. If the search returns a valid sequence, decoding succeeds. If no valid sequence is found
   against the candidate library, the signature is malformed (B04). The "more than one valid
   sequence" case requires a BLAKE3 collision among components — a second-preimage attack on
   BLAKE3-256 — and is computationally infeasible under the security assumptions of §7 of the
   Palimpsest Specification; a decoder that nevertheless encounters multiple satisfying
   sequences MUST also report B04 (treating it as a corruption or integrity failure rather than
   as a regular decoding outcome).

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
| B01  | Magic number absent or does not match either of the recognised values `B2 C4 B5 BB` (external-schema mode, BASE-256: `βτελ`, §6.1 field 1) or `B2 C4 B5 BC` (self-contained mode, BASE-256: `βτεμ`, §6.2 field 1). |
| B02  | A variable-length integer (§4) extends beyond the end of input, or its accumulator overflows the decoder's chosen integer width. |
| B03  | Schema signature length is not `33` (for `n = 1`) and not `37 + 2·(n − 2)` for any `n ≥ 2` (§6 field 2), **or** the XOR of every signature byte does not equal `0x79` — the BinTEL-pinned cadence byte (§8.2). |
| B04  | Schema signature does not decode against the available hash library (§8.2 decoding); zero or more than one valid hash sequence. |
| B05  | A keyword index read from the stream is outside `[0, keyword-count(parent-members))` (§7.8). |
| B06  | A Scalar value's byte length extends beyond the end of input.                                 |
| B07  | A Scalar value's UTF-8 bytes are not a valid UTF-8 sequence.                                  |
| B08  | The document-root decoding procedure of §7.8 terminates with input bytes remaining (framing error per §6). |
| B09  | The document-root decoding procedure of §7.8 requests bytes beyond end of input.              |
| B10  | A `Reference` type appears in the schema but resolves to no `Definition` (E210 condition at parse time; surfaced by the decoder as a configuration error if the composed schema is malformed). |
| B11  | In self-contained mode (§6.2), the composed signature recomputed from the embedded schema body does not equal the carried signature byte-for-byte. |
| B12  | In self-contained mode (§6.2), the embedded schema body does not decode as a valid TEL document under `tel-schema` (structural error during bootstrap; the bytes do not yield a well-formed schema document). |

All BinTEL error codes (B01–B12) are **fatal**: on any such error a conforming decoder MUST
abort decoding and MUST NOT emit any partial result. BinTEL is the authoritative serialisation
of the semantic model — once any byte is found inconsistent with §6 / §7, no remaining bytes
can be trusted to convey their nominal types and lengths. No recovery is specified for BinTEL.

A decoder MAY perform additional consistency checks beyond those above (for example, checking
that a Struct-typed child's claimed type is actually a Struct after Reference resolution); these
are implementation-specific and are not error conditions defined by this specification.
