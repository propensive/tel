# BASE-256 Specification Draft

## Abstract

BASE-256 is a binary-to-text encoding that maps each byte of input to a single
Unicode character drawn from a fixed 256-character alphabet. The alphabet is
constructed such that decoding requires no lookup table: the original byte value
of a character is its Unicode code point taken modulo 256. Every character in
the alphabet has the Unicode General Category `Lu`, `Ll`, or `Lt` — uppercase,
lowercase, or titlecase letter — drawn from blocks used by European alphabets.
Consequently, a contiguous run of BASE-256 characters forms a single word under
the Unicode default word-segmentation algorithm, so it may be selected as a unit
by a double-click in any conforming text-handling environment.

BASE-256 expands input by a factor of approximately 1.5× when measured in UTF-8
bytes (since most of its characters are encoded in 2 or 3 UTF-8 bytes), but the
length in **characters** is identical to the length in **bytes** of the input.
The encoding is therefore most useful where the carrier medium is character-
oriented rather than byte-oriented, where copy-and-paste handling is required,
or where the content must be embeddable in a textual format without escaping.

## 1. Status

This document is a draft specification of BASE-256.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this
document are to be interpreted as described in RFC 2119 and RFC 8174 when, and
only when, they appear in all capitals.

## 3. Definitions

The following definitions apply throughout this document:

- A **byte** is an integer in the range [0, 255].
- A **code point** is a non-negative integer denoting a Unicode scalar value, in
  the range [0, 0x10FFFF].
- The **alphabet** is the sequence of 256 Unicode code points specified in §4.
  The alphabet character at position `b` (0 ≤ b ≤ 255) is denoted `A[b]`.

## 4. Alphabet

The alphabet is the following sequence of 256 Unicode characters, indexed from
0:

```
ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠḡḢḣḤĥȦȧШḩЪЫЬЭĮį0123456789ĺĻļĽľĿŀABCDEFGHIJKLMNOPQRSTUVWXYZṛќѝŞşŠabcdefghijklmnopqrstuvwxyzŻżṽžſẀẁẂẃẄẅẆẇẈẉΊẋẌẍΎƏҐґƒẓΔƕƖẗẘẙҚқƜƝΞƟƠơҢңƤƥΦƧƨΩΪΫάέήίưᾱβγδεζҷᾸικλμẽξοπӁӂÃτÅÆÇψωϊϋỌύώϏÐǑǒǓÔϕӖϗῘÙῚӛӜӝÞӟàῡǢǣӤåæçǨῩӪӫìíӮӯðñỲỳôỵǶỷӸùῺΏǼǽþǿ
```

The alphabet has the following defining property:

> For every position `b` in [0, 255], the Unicode code point of `A[b]` is
> congruent to `b` modulo 256:
>
>     codepoint(A[b]) ≡ b   (mod 256)

This property is what enables the constant-time, lookup-table-free decoding
defined in §6.

All 256 characters of the alphabet are pairwise distinct. The alphabet is drawn
from the following Unicode blocks:

- Basic Latin (`U+0030`–`U+0039`, `U+0041`–`U+005A`, `U+0061`–`U+007A`) — the
  ten ASCII digits and the fifty-two ASCII letters, which appear at the
  positions equal to their own code points.
- Latin-1 Supplement (`U+00C0`–`U+00FF`)
- Latin Extended-A (`U+0100`–`U+017F`)
- Latin Extended-B (`U+0180`–`U+024F`)
- Greek and Coptic (`U+0370`–`U+03FF`)
- Cyrillic (`U+0400`–`U+04FF`)
- Latin Extended Additional (`U+1E00`–`U+1EFF`)
- Greek Extended (`U+1F00`–`U+1FFF`)

Every code point in the alphabet has the Unicode General Category `Lu`
(Uppercase Letter), `Ll` (Lowercase Letter), or `Lt` (Titlecase Letter). This is
the property exploited by §7.

## 5. Encoding

To encode a byte sequence `B = b₀, b₁, …, b_{n-1}` (each `bᵢ` ∈ [0, 255]):

1. For each `i` in `0..n`, the i-th output character is `A[bᵢ]`, where `A` is
   the alphabet of §4.
2. The encoded form is the resulting sequence of n Unicode characters,
   concatenated.

An encoder MUST emit the encoded form as Unicode text. When that text is to be
serialised to bytes (for example, written to a file or transmitted over a
byte-oriented channel), it MUST be encoded as UTF-8 unless a different Unicode
transformation format is agreed by the parties.

The encoded length, measured in characters, equals the input length in bytes.
The encoded length measured in UTF-8 bytes is greater than the input length and
depends on the specific bytes encoded:

- Input bytes in the ranges [0x30, 0x39], [0x41, 0x5A], and [0x61, 0x7A] (the
  ASCII digits and letters) are encoded as exactly one UTF-8 byte each.
- All other input bytes are encoded as either two or three UTF-8 bytes, drawn
  from the blocks listed in §4.

## 6. Decoding

To decode a string `S = c₀, c₁, …, c_{n-1}` of Unicode characters previously
produced by §5:

1. For each `i` in `0..n`, the i-th output byte is the Unicode code point of
   `cᵢ` taken modulo 256.
2. The decoded form is the resulting sequence of n bytes.

Equivalently, in pseudocode:

```
decode(S) := [ codepoint(c) mod 256  for c in S ]
```

The decoder requires no lookup table, no state, and no scan of the alphabet:
each input character is decoded in constant time by a single modulo operation
on its code point. This is a direct consequence of the alphabet's defining
property in §4.

A decoder MUST treat the input as a sequence of Unicode characters (scalar
values), not as a sequence of UTF-8 bytes. When the input is supplied as UTF-8
(or any other Unicode transformation format), the decoder MUST first decode it
into a sequence of code points and then apply the per-character rule above.

## 7. Selection Behavior

The Unicode default word-segmentation algorithm (Unicode Standard Annex #29)
divides a stream of Unicode characters into words by applying a set of
word-boundary rules. Rule WB5 in particular specifies that no boundary is
introduced between two characters that both have the derived `Word_Break`
property `ALetter`. The `ALetter` property is assigned, among other things, to
every character whose General Category is `Lu`, `Ll`, or `Lt`, with a small
number of explicit exclusions that do not intersect the alphabet of §4.

Because every character in the BASE-256 alphabet has General Category `Lu`,
`Ll`, or `Lt`, every adjacent pair of BASE-256 characters falls within rule
WB5 and is therefore joined into a single word by the algorithm.

Most operating systems, terminals, web browsers, and editor components
implement word selection on double-click using either the Unicode default word
segmentation directly, or a closely equivalent classifier that treats Unicode
letters as part of the same word as their neighbours. As a result, a
double-click anywhere within a BASE-256 string SHALL select the entire
contiguous run of BASE-256 characters — and SHALL NOT extend the selection
into surrounding whitespace, punctuation, or symbol characters, which fall
under different `Word_Break` classes and introduce word boundaries.

This behavior is a property of the alphabet, not of any particular
implementation of BASE-256. An encoder or decoder that conforms to this
specification need take no special action to obtain it.

## 8. Round-Trip Property

For any byte sequence `B`, `decode(encode(B)) = B`. This follows directly from
§4: encoding sends byte `b` to a character whose code point is congruent to `b`
modulo 256, and decoding recovers that residue.

The converse — that `encode(decode(S)) = S` for an arbitrary Unicode string
`S` — does **not** hold, because many distinct Unicode code points share the
same residue modulo 256. The encoding is injective from bytes to characters
within the chosen alphabet; it is not surjective from characters back to the
alphabet. See §9.

## 9. Error Handling on Decode

Section 6 defines `decode` as a total function over Unicode strings: every code
point has a well-defined residue modulo 256, so no character is inherently
"invalid" in the arithmetic sense. However, an input string MAY contain
characters that were not produced by a conforming BASE-256 encoder.

A decoder MAY operate in one of two modes:

- In **permissive mode**, the decoder applies §6 to every input character
  without further checking. A character not present in the alphabet of §4 is
  decoded to the byte value equal to its code point modulo 256; the byte value
  recovered is well-defined but the original byte was not produced by a
  conforming encoder.
- In **strict mode**, the decoder additionally verifies that each input
  character is a member of the alphabet of §4 (for example, by membership in a
  precomputed set of 256 code points). If any input character is not a member
  of the alphabet, the decoder MUST report an error identifying the offending
  character and its position within the input.

A decoder operating in strict mode MAY pre-compute the alphabet membership set
once at initialization and consult it in constant time per character. The
alphabet membership check is the only lookup required; the decoding itself
remains a modulo operation.

Strict mode is RECOMMENDED for any application where the encoded input is
received from an untrusted source, or where the integrity of the encoded
representation is significant. Permissive mode is RECOMMENDED only where the
input is known to have been produced by a conforming encoder and additional
verification is undesired.

## 10. Security Considerations

BASE-256 is an encoding, not an encryption or integrity mechanism. It provides
no confidentiality, no authentication, and no error detection. Applications
that require these properties MUST apply them at a separate layer.

In permissive mode (§9), distinct input strings MAY decode to the same byte
sequence. An application that relies on the identity of the encoded
representation — for example, by hashing the encoded form, or by comparing
encoded forms for equality — MUST operate in strict mode, or normalise to the
canonical alphabet before such comparisons.

Some of the characters in the alphabet appear visually similar to characters
not in the alphabet, or to one another at small font sizes. Applications that
display BASE-256 strings to humans for visual transcription, rather than for
copy-paste, SHOULD consider the suitability of the rendering font and SHOULD
NOT rely on visual disambiguation between the alphabet and homoglyphic code
points.

## 11. References

- The Unicode Standard, Version 15.0 (or later). The Unicode Consortium.
- Unicode Standard Annex #29: Unicode Text Segmentation.
- RFC 2119: Key words for use in RFCs to Indicate Requirement Levels.
- RFC 8174: Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words.
- RFC 3629: UTF-8, a transformation format of ISO 10646.
