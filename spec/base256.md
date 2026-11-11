# BASE-256 Specification Draft

## Abstract

BASE-256 is a binary-to-text encoding that maps each byte of input to a single Unicode character
drawn from a fixed 256-character alphabet. The alphabet is constructed such that decoding requires
no lookup table: the original byte value of a character is its Unicode code point taken modulo 256.
Every character in the alphabet is a letter or a decimal digit, drawn from blocks used by European
alphabets: 115 have the Unicode General Category `Lu` (uppercase letter), 131 have `Ll` (lowercase
letter), and 10 have `Nd` (decimal digit). The ten `Nd` characters are those at positions
`0x30`–`0x39`, which appear as the ASCII digits themselves; the remaining 246 positions are
letters. Consequently, a contiguous run of BASE-256
characters forms a single word under the Unicode default word-segmentation algorithm (letters via
rule WB5, digits via WB8, and the two joined by WB9/WB10), so it may be selected as a unit by a
double-click in any conforming text-handling environment.

BASE-256 expands input when measured in UTF-8 bytes: 62 of its characters encode to one UTF-8 byte,
150 to two, and 44 to three (§5), so uniformly distributed input expands by a mean factor of
494/256 ≈ **1.93×**, and the factor ranges from 1× to 3× as the byte distribution varies. The length
in **characters**, by contrast, is always identical to the length in **bytes** of the input. The
encoding is therefore most useful where the carrier medium is character-oriented rather than
byte-oriented, where copy-and-paste handling is required, or where the content must be embeddable in
a textual format without escaping.

## 1. Status

This document is a draft specification of BASE-256.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Definitions

The following definitions apply throughout this document:

- A **byte** is an integer in the range [0, 255].
- A **code point** is a non-negative integer denoting a Unicode scalar value, in the range [0,
  0x10FFFF].
- The **alphabet** is the sequence of 256 Unicode code points specified in §4. The alphabet
  character at position `b` (0 ≤ b ≤ 255) is denoted `A[b]`.

## 4. Alphabet

The alphabet is the following sequence of 256 Unicode characters, indexed from 0. The first
form below presents the alphabet linearly (positions 0–255 in order). The second form is a
16×16 grid: row labels are the byte's high nibble, column labels are the low nibble, so the
character at byte `b` is found at row `b >> 4`, column `b & 0xF`. The line breaks within the
linear form are presentational only: the alphabet is the 256-character sequence obtained by
concatenating those lines, and contains no `LF`.

```
ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠḡḢḣḤĥȦȧШḩЪЫЬЭĮį0123456789ĺĻļĽľĿŀABCDEFGHIJKLMNOPQRSTUVWXYZṛќѝŞşŠabc
defghijklmnopqrstuvwxyzŻżṽžſẀẁẂẃẄẅẆẇẈẉΊẋẌẍΎƏҐґƒẓΔƕƖẗẘẙҚқƜƝΞƟƠơҢңƤƥΦƧƨΩΪΫάέήίưᾱβγδεζҷᾸικλμẽξοπӁӂÃτÅÆÇ
ψωϊϋỌύώϏÐǑǒǓÔϕӖϗῘÙῚӛӜӝÞӟàῡǢǣӤåæçǨῩӪӫìíӮӯðñỲỳôỵǶỷӸùῺûǼǽþǿ
```

```
     0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
 0_  Ḁ  ḁ  Ђ  Ѓ  Ą  ą  Ć  ć  Ȉ  ȉ  Њ  ḋ  Ќ  ḍ  Ď  ď
 1_  Ȑ  ȑ  Ē  Г  Д  ȕ  Ж  З  Ę  ę  Ț  ț  Ĝ  ĝ  Ḟ  ḟ
 2_  Ḡ  ḡ  Ḣ  ḣ  Ḥ  ĥ  Ȧ  ȧ  Ш  ḩ  Ъ  Ы  Ь  Э  Į  į
 3_  0  1  2  3  4  5  6  7  8  9  ĺ  Ļ  ļ  Ľ  ľ  Ŀ
 4_  ŀ  A  B  C  D  E  F  G  H  I  J  K  L  M  N  O
 5_  P  Q  R  S  T  U  V  W  X  Y  Z  ṛ  ќ  ѝ  Ş  ş
 6_  Š  a  b  c  d  e  f  g  h  i  j  k  l  m  n  o
 7_  p  q  r  s  t  u  v  w  x  y  z  Ż  ż  ṽ  ž  ſ
 8_  Ẁ  ẁ  Ẃ  ẃ  Ẅ  ẅ  Ẇ  ẇ  Ẉ  ẉ  Ί  ẋ  Ẍ  ẍ  Ύ  Ə
 9_  Ґ  ґ  ƒ  ẓ  Δ  ƕ  Ɩ  ẗ  ẘ  ẙ  Қ  қ  Ɯ  Ɲ  Ξ  Ɵ
 A_  Ơ  ơ  Ң  ң  Ƥ  ƥ  Φ  Ƨ  ƨ  Ω  Ϊ  Ϋ  ά  έ  ή  ί
 B_  ư  ᾱ  β  γ  δ  ε  ζ  ҷ  Ᾰ  ι  κ  λ  μ  ẽ  ξ  ο
 C_  π  Ӂ  ӂ  Ã  τ  Å  Æ  Ç  ψ  ω  ϊ  ϋ  Ọ  ύ  ώ  Ϗ
 D_  Ð  Ǒ  ǒ  Ǔ  Ô  ϕ  Ӗ  ϗ  Ῐ  Ù  Ὶ  ӛ  Ӝ  ӝ  Þ  ӟ
 E_  à  ῡ  Ǣ  ǣ  Ӥ  å  æ  ç  Ǩ  Ῡ  Ӫ  ӫ  ì  í  Ӯ  ӯ
 F_  ð  ñ  Ỳ  ỳ  ô  ỵ  Ƕ  ỷ  Ӹ  ù  Ὼ  û  Ǽ  ǽ  þ  ǿ
```

The two forms are equivalent; any divergence between them is an error in this specification. A
conforming implementation MAY use either form when transcribing the alphabet into source code,
but MUST verify the defining property of §4 over the resulting table to detect transcription
errors.

The alphabet has the following defining property:

> For every position `b` in [0, 255], the Unicode code point of `A[b]` is congruent to `b` modulo
> 256:
>
>     codepoint(A[b]) ≡ b   (mod 256)

This property is what enables the constant-time, lookup-table-free decoding defined in §6.

All 256 characters of the alphabet are pairwise distinct. The alphabet is drawn from the following
Unicode blocks:

- Basic Latin (`U+0030`–`U+0039`, `U+0041`–`U+005A`, `U+0061`–`U+007A`) — the ten ASCII digits and
  the fifty-two ASCII letters, which appear at the positions equal to their own code points.
- Latin-1 Supplement (`U+00C0`–`U+00FF`)
- Latin Extended-A (`U+0100`–`U+017F`)
- Latin Extended-B (`U+0180`–`U+024F`)
- Greek and Coptic (`U+0370`–`U+03FF`)
- Cyrillic (`U+0400`–`U+04FF`)
- Latin Extended Additional (`U+1E00`–`U+1EFF`)
- Greek Extended (`U+1F00`–`U+1FFF`)

Every code point in the alphabet has the Unicode General Category `Lu` (Uppercase Letter), `Ll`
(Lowercase Letter), or `Nd` (Decimal Digit). The exact composition is 115 `Lu`, 131 `Ll`, and 10
`Nd`; the `Nd` code points are exactly those at positions `0x30`–`0x39`, which encode to the ASCII
digits themselves. No character of the alphabet is `Lt` (Titlecase Letter). This is the property
exploited by §7.

## 5. Encoding

To encode a byte sequence `B = b₀, b₁, …, b_{n-1}` (each `bᵢ` ∈ [0, 255]):

1. For each `i` in `0..n`, the i-th output character is `A[bᵢ]`, where `A` is the alphabet of §4.
2. The encoded form is the resulting sequence of n Unicode characters, concatenated.

An encoder MUST emit the encoded form as Unicode text. When that text is to be serialised to bytes
(for example, written to a file or transmitted over a byte-oriented channel), it MUST be encoded as
UTF-8 unless a different Unicode transformation format is agreed by the parties. §10 states the
constraints on normalisation and case mapping that any carrier of the encoded text must respect.

The encoded length, measured in characters, equals the input length in bytes. The encoded length
measured in UTF-8 bytes is greater than the input length and depends on the specific bytes encoded:

- The 62 input bytes in the ranges [0x30, 0x39], [0x41, 0x5A], and [0x61, 0x7A] (the ASCII digits
  and letters) are encoded as exactly one UTF-8 byte each.
- 150 input bytes are encoded as two UTF-8 bytes each — those whose alphabet character lies in
  `U+0080`–`U+07FF` (Latin-1 Supplement, Latin Extended-A, Latin Extended-B, Greek and Coptic, and
  Cyrillic).
- The remaining 44 input bytes are encoded as three UTF-8 bytes each — those whose alphabet
  character lies in Latin Extended Additional or Greek Extended.

The total UTF-8 length of the whole alphabet is therefore `62·1 + 150·2 + 44·3 = 494` bytes, which
is the basis of the ≈1.93× mean expansion quoted in the Abstract.

## 6. Decoding

To decode a string `S = c₀, c₁, …, c_{n-1}` of Unicode characters previously produced by §5:

1. For each `i` in `0..n`, the i-th output byte is the Unicode code point of `cᵢ` taken modulo 256.
2. The decoded form is the resulting sequence of n bytes.

Equivalently, in pseudocode:

```
decode(S) := [ codepoint(c) mod 256  for c in S ]
```

The decoder requires no lookup table, no state, and no scan of the alphabet: each input character is
decoded in constant time by a single modulo operation on its code point. This is a direct
consequence of the alphabet's defining property in §4.

A decoder MUST treat the input as a sequence of Unicode characters (scalar values), not as a
sequence of UTF-8 bytes. When the input is supplied as UTF-8 (or any other Unicode transformation
format), the decoder MUST first decode it into a sequence of code points and then apply the
per-character rule above.

## 7. Selection Behavior

The Unicode default word-segmentation algorithm (Unicode Standard Annex #29) divides a stream of
Unicode characters into words by applying a set of word-boundary rules. Rule WB5 specifies that no
boundary is introduced between two characters that both have the derived `Word_Break` property
`ALetter` (assigned, among other things, to every character whose General Category is `Lu`, `Ll`, or
`Lt`). Rule WB8 does the same for two characters with property `Numeric` (decimal digits). Rules WB9
and WB10 bridge the two classes so that a letter is not separated from an adjacent digit.

Every character in the BASE-256 alphabet is either an `ALetter` (the 246 non-digit positions) or a
`Numeric` (the ten ASCII digits at positions `0x30`–`0x39`). Adjacent BASE-256 characters therefore
always fall within one of WB5, WB8, WB9, or WB10 and are joined into a single word by the algorithm.

Most operating systems, terminals, web browsers, and editor components implement word selection on
double-click using either the Unicode default word segmentation directly, or a closely equivalent
classifier that treats Unicode letters as part of the same word as their neighbours. In any
environment that does so, a double-click anywhere within a BASE-256 string selects the entire
contiguous run of BASE-256 characters, and does not extend the selection into surrounding
whitespace, punctuation, or symbol characters, which fall under different `Word_Break` classes and
introduce word boundaries.

This behavior is a property of the alphabet, not of any particular implementation of BASE-256, and
this section imposes no requirement on encoders or decoders: no conformance obligation is stated
here, because the behaviour is exhibited by the text-handling environment rather than by any
implementation of this specification. An encoder or decoder that conforms to this specification
need take no special action to obtain it.

## 8. Round-Trip Property

For any byte sequence `B`, `decode(encode(B)) = B`. This follows directly from §4: encoding sends
byte `b` to a character whose code point is congruent to `b` modulo 256, and decoding recovers that
residue.

The converse — that `encode(decode(S)) = S` for an arbitrary Unicode string `S` — does **not** hold,
because many distinct Unicode code points share the same residue modulo 256. The encoding is
injective from bytes to characters within the chosen alphabet; it is not surjective from characters
back to the alphabet. Composing decode then encode is therefore a **normalisation**: any Unicode
string `S` is mapped to the canonical BASE-256 string carrying the same byte residues. See §9.

## 9. Error Handling on Decode

Section 6 defines `decode` as a total function over Unicode strings: every code point has a
well-defined residue modulo 256, so no character is inherently "invalid" in the arithmetic sense.
However, an input string MAY contain characters that were not produced by a conforming BASE-256
encoder.

A decoder MAY operate in one of two modes:

- In **permissive mode**, the decoder applies §6 to every input character without further checking.
  A character not present in the alphabet of §4 is decoded to the byte value equal to its code point
  modulo 256; the byte value recovered is well-defined but the original byte was not produced by a
  conforming encoder.
- In **strict mode**, the decoder additionally verifies that each input character is a member of the
  alphabet of §4 (for example, by membership in a precomputed set of 256 code points). If any input
  character is not a member of the alphabet, the decoder MUST report an error identifying the
  offending character and its position within the input, expressed as a zero-based **code-point**
  index (not a byte offset). A decoder SHOULD report every such character rather than only the
  first.

A decoder operating in strict mode MAY pre-compute the alphabet membership set once at
initialization and consult it in constant time per character. The alphabet membership check is the
only lookup required; the decoding itself remains a modulo operation.

Strict mode is RECOMMENDED for any application where the encoded input is received from an untrusted
source, or where the integrity of the encoded representation is significant. Permissive mode is
RECOMMENDED only where the input is known to have been produced by a conforming encoder and
additional verification is undesired.

## 10. Security Considerations

BASE-256 is an encoding, not an encryption or integrity mechanism. It provides no confidentiality,
no authentication, and no error detection. Applications that require these properties MUST apply
them at a separate layer.

In permissive mode (§9), distinct input strings MAY decode to the same byte sequence. An application
that relies on the identity of the encoded representation — for example, by hashing the encoded
form, or by comparing encoded forms for equality — MUST operate in strict mode, or normalise to the
canonical alphabet before such comparisons.

**Unicode normalization.** BASE-256 text MUST NOT be normalised to NFD or NFKC, and MUST NOT be
transformed by any process that applies such a normalisation. The alphabet is NFC-stable — applying
NFC to a BASE-256 string is a no-op, so passage through an NFC-normalising channel is safe — but:

- **NFD** decomposes 133 of the 256 alphabet characters into a base character followed by one or
  more combining marks, destroying the one-character-per-byte correspondence entirely.
- **NFKC** alters four characters — those at positions `0x3F` (`Ŀ`), `0x40` (`ŀ`), `0x7F` (`ſ`), and
  `0xD5` (`ϕ`) — and changes the length of the string, since `Ŀ` and `ŀ` expand to two characters
  each.

A protocol that carries BASE-256 through a normalising layer therefore MUST pin that layer to NFC or
to no normalisation at all.

**Case mapping.** BASE-256 text MUST NOT be case-folded, uppercased, or lowercased. 83 of the
alphabet's characters have a lowercase form that is *also* in the alphabet, so case mapping silently
rewrites the data to a *different but well-formed* BASE-256 string, decoding to different bytes. This
corruption is undetectable by the strict-mode check of §9, because the result is still drawn entirely
from the alphabet. Applications that normalise identifier case (for example, when using a BASE-256
string as a filename, a hostname label, or a database key) MUST exempt BASE-256 values from that
normalisation.

Some of the characters in the alphabet appear visually similar to characters not in the alphabet, or
to one another at small font sizes. Applications that display BASE-256 strings to humans for visual
transcription, rather than for copy-paste, SHOULD consider the suitability of the rendering font and
SHOULD NOT rely on visual disambiguation between the alphabet and homoglyphic code points.

## 11. Normative Test Vectors

A conforming implementation MUST reproduce every vector in this section exactly. Byte sequences are
written in hexadecimal; encoded forms are written as the literal Unicode characters.

| Input bytes | Encoded form | Note |
| ----------- | ------------ | ---- |
| *(empty)* | *(empty)* | The encoding of the empty sequence is the empty string. |
| `00` | `Ḁ` | Position 0 of the alphabet, `U+1E00`. |
| `01` | `ḁ` | Position 1, `U+1E01`. |
| `30` | `0` | An ASCII digit; encodes to itself, one UTF-8 byte. |
| `41` | `A` | An ASCII letter; encodes to itself. |
| `7F` | `ſ` | `U+017F`; note that `0x7F` is *not* self-encoding. |
| `FF` | `ǿ` | Position 255, `U+01FF`. |
| `00 01 FF` | `Ḁḁǿ` | Concatenation is character-by-character. |
| `41 42 43` | `ABC` | An all-ASCII-letter input is unchanged by the encoding. |

**Identity vector.** Encoding the 256-byte sequence `00 01 02 … FD FE FF` yields exactly the
alphabet of §4, in order. This vector doubles as the transcription self-check that §4 requires: an
implementation that encodes `0..255` and compares the result against its own alphabet table has
verified the table end to end.

**Round-trip vector.** Decoding the alphabet of §4 yields the 256-byte sequence `00 01 02 … FF`. In
strict mode (§9) this decode MUST succeed with no error reported.

**Shared vector.** The value hash pinned by §3 of the [BinTEL Specification](bintel.md) — the
BLAKE3-256 digest of the canonical `tels.tel` — provides a realistic 32-byte vector that all three
specifications share:

```
bytes:    5fe9b009b90872af76f39b273773a85f2f21999d56a91785257deb8a7f6c9ae6
BASE-256: şῩưȉιȈrίvỳқȧ7sƨşįḡẙƝVΩЗẅĥṽӫΊſlҚæ
```

The encoded form is 32 characters long, matching the 32-byte input, and occupies 64 bytes when
serialised as UTF-8 — a 2× expansion for this particular input, against the 1.93× mean.

## 12. References

- The Unicode Standard, Version 15.0 (or later). The Unicode Consortium.
- Unicode Standard Annex #29: Unicode Text Segmentation.
- RFC 2119: Key words for use in RFCs to Indicate Requirement Levels.
- RFC 8174: Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words.
- RFC 3629: UTF-8, a transformation format of ISO 10646.
