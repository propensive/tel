# Standard Codecs Specification Draft

## Abstract

A TEL scalar's semantic value is its text (§18.2 of the [TEL Specification](tel.md)). A
**codec** (TEL §21.7) gives a scalar a binary form: `encode` maps each accepted text to a byte
sequence and `decode` maps it back. The TEL Specification defines only the codec interface and
its laws; every codec is otherwise application-defined, and the only codecs pinned so far are the
variable-width `base-256` and `schema-signature` of the [BinTEL Specification](bintel.md) §8.4.

This document pins a small set of **standard codecs** — integers, IEEE 754 binary64, UUIDs and
BLAKE3-256 digests — so that two implementations binding the same name agree byte-for-byte (law
C4). Every codec defined here is **fixed-width** and **order-preserving**: its byte form has one
length, and the bytewise (unsigned, lexicographic) order of byte forms is the natural order of
the values they denote. These two properties let the random-access encoding
[RaTEL](ratel.md) store values inline in fixed slots and compare index keys type-blind with a
single `memcmp`, while [BinTEL](bintel.md) §7.1 carries the same bytes length-prefixed.

## 1. Status

This document is a draft specification of the TEL standard codecs.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Registry Rules

The names defined here are kebab-case identifiers (TEL §20.7; `blake3-256` is well-formed, since
a hyphen may be followed by a digit) in the shared helper namespace of TEL §21.1. A schema names
one with `encoding ‹name›` in a `scalar` body (TEL §20).

**Fixity.** A name defined here denotes exact extensional behaviour — its accepted texts and its
text↔byte mapping — forever (TEL §21.7, C4). Changed behaviour requires a new name; a later
revision of this specification MAY add names but MUST NOT alter or remove the behaviour of a name
already defined.

**Binding.** An implementation that binds any of these names MUST bind it to exactly the
behaviour defined here, and SHOULD bind all of them. A RaTEL implementation MUST bind all of
them. Each is independent of the others: `tels` declares no encodings, so the BinTEL bootstrap
(BinTEL §6.2) is unaffected by any of them.

**Reserved names.** The following names are reserved for future revisions of this specification
and MUST NOT be bound to any other behaviour: `f32`, `f16`, `u128`, `i128`, `bool`, `timestamp`,
`duration`. A schema naming a reserved-but-undefined encoding is E313 (TEL §21.7) until the name
is defined.

**Validator role.** Because the namespace is shared (TEL §21.1), a standard codec name MAY also be
bound as a scalar validator. If it is, the validator SHOULD accept exactly the codec's accepted
texts, so that `validate u64` and `encoding u64` never disagree.

**Common properties.** Every codec in this document satisfies, in addition to C1–C4:

- `encode` is total on the accepted texts and rejects every other text. The accepted texts are
  **canonical**: a text that denotes an in-range value but is not in canonical form — leading
  zeros, a `+` sign, uppercase hexadecimal, `-0` for an integer type, surrounding or embedded
  whitespace, a trailing `.` or `.0` on a float — is rejected, and the value is **E312**.
- `decode` succeeds on exactly the image of `encode` (C3). For every codec but `f64` the image
  is the whole byte space of the codec's width; `f64` excludes the NaN patterns (§6).
- The byte form has a fixed **width**, stated per codec and summarised in §9. `decode` MUST
  reject input of any other length.
- Bytewise order of byte forms equals the natural order of values (**order preservation**).

## 4. Unsigned Integers: `u8`, `u16`, `u32`, `u64`

For `uN` with `N ∈ {8, 16, 32, 64}`:

- **Accepted texts.** `"0" | [1-9][0-9]*`, denoting a value `v` with `0 ≤ v < 2^N`.
- **Byte form.** `v` as an unsigned big-endian integer in exactly `N/8` bytes.
- **Order.** Bytewise order is numeric order, by construction of big-endian notation.

| Codec | Width | Text          | Bytes                  |
| ----- | ----- | ------------- | ---------------------- |
| `u8`  | 1     | `0`           | `00`                   |
| `u8`  | 1     | `255`         | `FF`                   |
| `u16` | 2     | `256`         | `0100`                 |
| `u32` | 4     | `4294967295`  | `FFFFFFFF`             |
| `u64` | 8     | `1`           | `0000000000000001`     |
| `u64` | 8     | `18446744073709551615` | `FFFFFFFFFFFFFFFF` |

`256` under `u8`, `007`, `+1`, `-0` and `1 ` are each E312.

## 5. Signed Integers: `i8`, `i16`, `i32`, `i64`

For `iN` with `N ∈ {8, 16, 32, 64}`:

- **Accepted texts.** `"0" | "-"? [1-9][0-9]*`, denoting a value `v` with
  `−2^(N−1) ≤ v ≤ 2^(N−1) − 1`. `-0` is not accepted: zero has the single text `0`.
- **Byte form.** `v + 2^(N−1)` as an unsigned big-endian integer in exactly `N/8` bytes. This is
  the two's-complement representation of `v` with its sign bit inverted — an **offset binary**
  (excess-`2^(N−1)`) form.
- **Order.** Adding the constant `2^(N−1)` is a monotone bijection from the signed range onto
  `[0, 2^N)`, so bytewise order is numeric order: every negative value precedes every
  non-negative one.

| Codec | Width | Text                   | Bytes              |
| ----- | ----- | ---------------------- | ------------------ |
| `i8`  | 1     | `-128`                 | `00`               |
| `i8`  | 1     | `-1`                   | `7F`               |
| `i8`  | 1     | `0`                    | `80`               |
| `i8`  | 1     | `127`                  | `FF`               |
| `i16` | 2     | `-32768`               | `0000`             |
| `i16` | 2     | `32767`                | `FFFF`             |
| `i64` | 8     | `-9223372036854775808` | `0000000000000000` |
| `i64` | 8     | `-1`                   | `7FFFFFFFFFFFFFFF` |
| `i64` | 8     | `0`                    | `8000000000000000` |
| `i64` | 8     | `9223372036854775807`  | `FFFFFFFFFFFFFFFF` |

## 6. Double-Precision Floating Point: `f64`

The `f64` codec represents IEEE 754 binary64 values. Its byte form is the familiar
order-preserving transformation of the bit pattern; its accepted texts are the shortest
round-trip decimal renderings, defined here in full so that no host library's formatting is
normative.

### 6.1 Byte Form

Let `p` be the 64-bit pattern of the value, read as a `u64`. Then:

- if the sign bit (bit 63) is clear, `q = p XOR 0x8000000000000000`;
- if the sign bit is set, `q = p XOR 0xFFFFFFFFFFFFFFFF`;

and the byte form is `q` in big-endian order in 8 bytes. `decode` inverts this: read `q`
big-endian; if bit 63 of `q` is set, `p = q XOR 0x8000000000000000`, otherwise
`p = q XOR 0xFFFFFFFFFFFFFFFF`.

The map `p ↦ q` is a bijection on all `2^64` patterns. Non-negative patterns, whose magnitude
bits already sort numerically, are moved above `0x8000000000000000` with their order intact;
negative patterns are reflected so that larger magnitudes sort lower. Consequently bytewise order
equals numeric order on all non-NaN values, with `-Infinity` first, `-0` immediately below `0`,
and `Infinity` last. The patterns with the sign bit set, exponent all ones and non-zero
significand (negative NaNs) fall below `-Infinity`; those with the sign bit clear (positive
NaNs) fall above `Infinity`. Both groups are rejected by `decode` (§6.3).

### 6.2 Accepted Texts

The accepted texts are exactly the canonical renderings of the non-NaN values. `encode(t)`
succeeds if and only if (a) `t` matches the grammar

```
float    ::= "-"? ( "0" | [1-9][0-9]* ) ( "." [0-9]+ )? ( "e" ( "+" | "-" ) [0-9]+ )?
           | "Infinity" | "-Infinity"
```

(b) parsing `t` as a decimal under round-to-nearest-even yields a binary64 value `x`, and (c)
the canonical rendering of `x`, defined below, is code-point-identical to `t`. Condition (c) is
what makes the mapping a bijection (C2, C3): `1.0`, `01`, `1e5`, `.5`, `1E5`, `+1`, `1e+05`,
`0.10` and `100000.0` are all E312, while `1`, `100000`, `0.5` and `0.1` are accepted.

**Canonical rendering of `x`.**

- Positive zero renders as `0`; negative zero renders as `-0`. (This departs from ECMAScript,
  which prints both as `0`: the two zero patterns are distinct bytes and so, by C3, need
  distinct texts.)
- Positive and negative infinity render as `Infinity` and `-Infinity`.
- A negative finite non-zero `x` renders as `-` followed by the rendering of `−x`.
- For a positive finite `x`, let `D = d₁…d_k` (`d₁ ≠ 0`, `d_k ≠ 0`) and the integer `n` be the
  unique digit string and exponent such that `D × 10^(n−k)` round-trips to `x` under
  round-to-nearest-even, `k` is as small as possible, and among candidates of that length `D`
  is the one whose value is closest to `x` (ties broken toward the even `d_k`). This is the
  **shortest round-trip** significand computed by Ryū, Schubfach, Grisu-exact and the
  ECMAScript `Number::toString` digit choice; all agree on `(D, n)`. Then:
  - if `k ≤ n ≤ 21`: the digits of `D` followed by `n − k` zeros (`100`,
    `123456789012345680000`);
  - else if `0 < n ≤ 21`: the first `n` digits of `D`, a `.`, and the remaining `k − n` digits
    (`3.141592653589793`);
  - else if `−6 < n ≤ 0`: `0.`, then `−n` zeros, then the digits of `D` (`0.5`, `0.1`,
    `0.000001`);
  - otherwise (`n > 21` or `n ≤ −6`): `d₁`, then if `k > 1` a `.` and `d₂…d_k`, then `e`, then
    `+` if `n − 1 ≥ 0` or `-` otherwise, then `|n − 1|` in decimal with no leading zeros
    (`1e+21`, `1e-7`, `5e-324`, `1.7976931348623157e+308`).

These rules restate ECMAScript `Number::toString(10)` exactly, except for negative zero, and
produce the renderings in §6.4 as a conformance check. An implementation MAY render by any
algorithm that provably yields the same `(D, n)` and then formats per the rules above.

### 6.3 NaN

No text denotes NaN. `encode` has no NaN output, and `decode` MUST reject every byte form whose
inverse pattern has exponent bits all ones and a non-zero significand — that is, every `q` in
`[0x0000000000000001, 0x000FFFFFFFFFFFFF)` or `(0xFFF0000000000000, 0xFFFFFFFFFFFFFFFF]` —
with a `CodecFailure`. This is permitted by C3, since these bytes lie outside the image of
`encode`; a document whose scalar held NaN has no TEL text and therefore no BinTEL or RaTEL form.

### 6.4 Normative Test Vectors

A conforming `f64` codec MUST map each text below to the stated 8 bytes and back.

| Text                      | Bytes              |
| ------------------------- | ------------------ |
| `0`                       | `8000000000000000` |
| `-0`                      | `7FFFFFFFFFFFFFFF` |
| `1`                       | `BFF0000000000000` |
| `-1`                      | `400FFFFFFFFFFFFF` |
| `0.1`                     | `BFB999999999999A` |
| `0.5`                     | `BFE0000000000000` |
| `100`                     | `C059000000000000` |
| `0.000001`                | `BEB0C6F7A0B5ED8D` |
| `1e-7`                    | `BE7AD7F29ABCAF48` |
| `1e+21`                   | `C44B1AE4D6E2EF50` |
| `123456789012345680000`   | `C41AC53A7E04BCDA` |
| `3.141592653589793`       | `C00921FB54442D18` |
| `5e-324`                  | `8000000000000001` |
| `1.7976931348623157e+308` | `FFEFFFFFFFFFFFFF` |
| `Infinity`                | `FFF0000000000000` |
| `-Infinity`               | `000FFFFFFFFFFFFF` |

The vectors are chosen to exercise each formatting branch and both threshold edges: `1e+21`
(`n = 22`) is exponential while `123456789012345680000` (`n = 21`) is plain digits; `0.000001`
(`n = −5`) is positional while `1e-7` (`n = −6`) is exponential. Ordering the byte column
unsigned-lexicographically reproduces the numeric order `-Infinity < -1 < -0 < 0 < 5e-324 <
… < Infinity`.

**C4 risk (informative).** The chief interoperability hazard is a host whose float formatting
is not shortest-round-trip or formats thresholds differently: `Double.toString` in JDK releases
before 19 (which may print `2.0E-3`-style or non-shortest digits), C's `%g`/`%.17g`, Python's
`repr` (shortest, but `1e-07` and `1e+21`, and `1e-06` for `0.000001`), and any locale-aware
printer. Such hosts MUST NOT be used unadapted: an implementation MUST produce `(D, n)` by a
shortest-round-trip algorithm and format per §6.2 itself. The vectors above exist to detect a
mis-adapted host; an implementation SHOULD run them at start-up or in its test suite.

## 7. UUID: `uuid`

- **Accepted texts.** `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}` — the
  RFC 9562 hyphenated form in lowercase only. Uppercase or mixed-case hexadecimal, braces, the
  `urn:uuid:` prefix and the unhyphenated 32-digit form are E312.
- **Byte form.** The 32 hexadecimal digits, hyphens removed, as 16 bytes in textual order.
- **Width.** 16. Every one of the `2^128` byte patterns is accepted by `decode`; no version or
  variant check is made, so the nil and max UUIDs and non-RFC layouts round-trip.
- **Order.** Bytewise order is the order of the text, and the order of the UUID as a 128-bit
  unsigned integer; for UUIDv7 this is creation-time order.

| Text                                   | Bytes                              |
| -------------------------------------- | ---------------------------------- |
| `00000000-0000-0000-0000-000000000000` | `00000000000000000000000000000000` |
| `0192b1c2-3d4e-7f5a-8b6c-9d0e1f2a3b4c` | `0192B1C23D4E7F5A8B6C9D0E1F2A3B4C` |
| `ffffffff-ffff-ffff-ffff-ffffffffffff` | `FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF` |

## 8. BLAKE3-256 Digest: `blake3-256`

- **Accepted texts.** `[0-9a-f]{64}` — 64 lowercase hexadecimal digits. Uppercase digits are
  E312.
- **Byte form.** The 32 bytes the digits denote, in textual order.
- **Width.** 32. Every byte pattern is accepted by `decode`. The codec does not verify that the
  value is a digest of anything; it is the natural encoding for any 256-bit value written as
  hexadecimal, and in particular for a BinTEL value hash (BinTEL §3) or a schema component hash
  (BinTEL §8.1).
- **Order.** Bytewise order is the order of the text.

The value hash of `acceptance.tel` (BinTEL §8.4) is the pinned example:

```
Text:  ed8e981cd770b7d75067e05dfd87d8b5c37e97763872febf513c125ab1a8b245
Bytes: ED 8E 98 1C D7 70 B7 D7 50 67 E0 5D FD 87 D8 B5
       C3 7E 97 76 38 72 FE BF 51 3C 12 5A B1 A8 B2 45
```

## 9. Summary

| Name         | Width (bytes) | Order-preserving | Accepted texts                                                    |
| ------------ | ------------- | ---------------- | ----------------------------------------------------------------- |
| `u8`         | 1             | yes              | `"0" \| [1-9][0-9]*`, `< 2^8`                                      |
| `u16`        | 2             | yes              | `"0" \| [1-9][0-9]*`, `< 2^16`                                     |
| `u32`        | 4             | yes              | `"0" \| [1-9][0-9]*`, `< 2^32`                                     |
| `u64`        | 8             | yes              | `"0" \| [1-9][0-9]*`, `< 2^64`                                     |
| `i8`         | 1             | yes              | `"0" \| "-"? [1-9][0-9]*`, `−2^7 … 2^7 − 1`                        |
| `i16`        | 2             | yes              | `"0" \| "-"? [1-9][0-9]*`, `−2^15 … 2^15 − 1`                      |
| `i32`        | 4             | yes              | `"0" \| "-"? [1-9][0-9]*`, `−2^31 … 2^31 − 1`                      |
| `i64`        | 8             | yes              | `"0" \| "-"? [1-9][0-9]*`, `−2^63 … 2^63 − 1`                      |
| `f64`        | 8             | yes (non-NaN)    | canonical shortest round-trip rendering (§6.2), `±Infinity`, `-0` |
| `uuid`       | 16            | yes              | `[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`    |
| `blake3-256` | 32            | yes              | `[0-9a-f]{64}`                                                    |

Reserved, not yet defined: `f32`, `f16`, `u128`, `i128`, `bool`, `timestamp`, `duration`.

## 10. Relationship to Other Specifications

*This section is informative.*

- **TEL §21.7** defines the `Codec` interface, laws C1–C4, the `CodecBinding` mechanism, and the
  validation outcome of a rejected text (E312) or an unresolved name (E313). This document adds
  bindings; it changes nothing about the contract.
- **BinTEL §7.1** carries an encoded scalar as the codec's bytes preceded by their length. For a
  standard codec the length is constant and is nonetheless written: BinTEL's framing is
  schema-blind to width. BinTEL's own codecs, `base-256` and `schema-signature` (BinTEL §8.4),
  are variable-width and are not repeated or altered here.
- **RaTEL** (`ratel.md`) relies on the two properties this document guarantees: a fixed width,
  so that a value occupies a slot of known size without a length prefix, and bytewise order
  equal to value order, so that an index over a scalar of any standard codec is compared with
  `memcmp` and no type dispatch. A RaTEL implementation therefore MUST bind every name in §9.
- **Width declarations.** Where a schema declares a `width` for a scalar (TEL §20), a value that
  restates a standard codec's width (§9) is a no-op; a value that differs from it is a schema
  error, **E226**. A width for a codec not defined here is the schema author's assertion about
  an application codec and is outside this document's scope.
- **Validators.** The built-in validators of TEL §21.5 are unrelated to these codecs; a standard
  codec name bound additionally as a validator follows §3 above.

## 11. Security Considerations

Decoders run on attacker-controlled bytes (BinTEL §11). The codecs here are designed so that
decoding is a bounded, branch-light operation:

- Every decoder MUST first check the input length against the codec's width and reject any
  other length with a `CodecFailure`; no decoder reads beyond its width or allocates in
  proportion to its input.
- Every decoder is total on its fixed-width byte space except `f64`, which rejects the NaN
  patterns (§6.3). A decoder MUST NOT substitute a default or a canonical NaN for a rejected
  pattern; the value is unrepresentable, and the containing document is malformed.
- Decoded texts are canonical by construction (C2), so `encode(decode(b)) = b`; an
  implementation that hardens by re-encoding (BinTEL §7.8, B15) pays one fixed-size
  comparison per value.
- The `f64` text parser accepts at most 17 significant digits in a canonical rendering, but the
  grammar of §6.2 permits longer digit strings and large exponents before condition (c) rejects
  them. Implementations MUST parse without unbounded precision or recursion — a correctly
  rounded parser is required for condition (b), but it MUST bound its work in the length of the
  input — and MUST NOT evaluate exponents eagerly as big integers.
- Order preservation is a property of the byte forms alone and is independent of any
  interpretation of the text, so an index built on these bytes cannot be confused by
  non-canonical texts: none has a byte form.
