# BinTEL Specification Draft

## Abstract

BinTEL is the binary encoding of the semantic model of a TEL document, as defined by the
[TEL Specification](spec.md). Every well-typed TEL document has exactly one BinTEL encoding; the
mapping is fully deterministic. A schema is itself a TEL document and therefore has a BinTEL
encoding.

BinTEL provides an unambiguous, compact serialization of the semantic model, suitable for hashing,
transmission, and schema identification.

A BinTEL document is defined here as a byte sequence. Where a text-oriented carrier is required —
embedding in a TEL document, transmission over a textual channel, display, or copy-and-paste — a
BinTEL byte sequence MAY be encoded as Unicode text using BASE-256 (see
[BASE-256 Specification](base256-spec.md)). The textual form is character-for-byte with the byte
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
a **schema signature** per §8 below. The signature is hex-encoded in lowercase for textual
representation. A schema with no layers has a single-component signature whose bytes are exactly
the 32-byte value hash, encoded as 64 hex characters.

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

The keyword index used in BinTEL node encoding is the position of a keyword in keyword order, as
defined in §20 of the TEL Specification. Because the schema determines the type of every node from
its keyword index and its parent's type, BinTEL encodes no type tags.

## 6. File Layout

A BinTEL file consists of the following fields in order:

1. **Magic number**: the 2 bytes `C0 D1`. When the document is carried in BASE-256 textual form
   (§9), these bytes appear as the two characters at positions `0xC0` and `0xD1` of the BASE-256
   alphabet defined in the [BASE-256 Specification](base256-spec.md).
2. **Schema signature**: the byte length of the signature (integer), followed by the signature
   bytes. The schema signature (whose construction is defined in the Schema Signature section below)
   identifies the composed schema (base plus layers) used to type the document.
3. **Document root**: encoded using the node encoding described in the Node Encoding section below
   (root form).

## 7. Node Encoding

**Document root.** The root is a virtual struct with no parent keyword. It is encoded as:

1. The number of top-level child nodes (integer).
2. Each top-level child node in order, using the struct, primitive, or flag encoding below.

**Struct node** (schema type is `Struct`, as defined in the TEL Specification):

1. The keyword index of this node (integer).
2. The number of child nodes (integer).
3. Each child node in order, recursively.

**Scalar node** (schema type is `Scalar`):

1. The keyword index of this node (integer).
2. The byte length of the UTF-8 encoding of the value string (integer).
3. The UTF-8-encoded bytes of the value string.

**Flag node** (schema type is `Flag`):

1. The keyword index of this node (integer).

**Default values.** BinTEL encodes the semantic model, in which a required `Scalar` member with a
non-null default is semantically present even when it was absent from the source document.
Therefore, when encoding a document to BinTEL, a missing required primitive whose default is used
MUST be encoded as a primitive node with the default value string. This ensures that the BinTEL
encoding is identical regardless of whether the member was explicitly written or filled by its
default.

There are no pad bytes, alignment constraints, or inter-node delimiters. The schema provides all
type information needed to decode the stream unambiguously.

## 8. Schema Signature

A schema signature identifies a composed schema as an ordered sequence of components: a base schema
followed by zero or more layers. Each component is identified by its value hash (§3).

A schema document (a TEL document conforming to the `tel-schema` schema; see §20 of the TEL
Specification) defines a base schema and zero or more layers. Each component's hash is its value
hash (§3): the component is encoded as a BinTEL document root (§7) and the SHA-256 digest is taken
over that root encoding alone, without the magic number or schema signature.

The construction below is a **palimpsest** with byte cadence `k = 2`, as defined in the
[Palimpsest Specification](palimpsest/spec.md). The palimpsest framework permits any byte cadence;
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
identifier of a TEL pragma (see §8.1 of the TEL Specification) — it is **hex-encoded** in lowercase
ASCII, producing `60 + 4n` characters. Hex (rather than BASE64-URL) is chosen because it preserves
byte-aligned structure: each component's contribution to the signature spans the same character
boundaries, making the encoded form interpretable by inspection. Decoders MUST accept both
lowercase and uppercase hex digits; encoders MUST emit lowercase.

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
in the [BASE-256 Specification](base256-spec.md). The textual form has one Unicode character per
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
