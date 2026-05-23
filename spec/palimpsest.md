# Palimpsest Specification Draft

## Abstract

A **palimpsest** is a compact binary encoding of an ordered sequence of fixed-length cryptographic
hashes, drawn from a larger known set. By exploiting the fact that both sender and receiver share
access to the same hash library, a palimpsest encodes a sequence of `n` hashes in `H + k*(n − 1)`
bytes rather than the naive `H*n` bytes, where `H` is the hash length in bytes and `k` is the
**cadence**, a small integer (typically 1–3) chosen relative to the library size. The space saving
comes at the cost of additional computation during decoding: the receiver must search its hash
library to reconstruct the original sequence.

The name **palimpsest** is used because the encoding layers the hashes over one another, each offset
by `k` bytes from the previous, so each hash is partially overwritten by its neighbours in the XOR
superposition — echoing the palimpsests of manuscript tradition where earlier writing shows through
later layers.

## 1. Status

This document is a draft specification of the palimpsest encoding.

## 2. Conformance Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD
NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as
described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

## 3. Definitions

| Symbol | Meaning                                                                                          |
| ------ | ------------------------------------------------------------------------------------------------ |
| `H`    | Hash length in bytes. For SHA-256, `H = 32`.                                                     |
| `n`    | Number of hashes in the sequence to encode. `n ≥ 1`.                                             |
| `k`    | **Cadence**: the byte offset between successive hashes in the palimpsest. `1 ≤ k < H`.           |
| `hᵢ`   | The i-th hash in the sequence (0-indexed), a byte array of length `H`.                           |
| `N`    | The total number of hashes in the receiver's library.                                            |
| `L`    | Length of the palimpsest in bytes: `L = H + k*(n − 1)`.                                          |

**Hash library.** A collection of `N` known hashes held by the receiver. The library MUST be
indexable by `k`-byte prefix: given a `k`-byte string `b`, the receiver MUST be able to enumerate
all hashes `h` in the library for which `h[0..k] == b`.

**Monotonic growth.** Hashes MAY be added to the library at any time. Hashes MUST NOT be removed: a
palimpsest encodes references to hashes that must be present in the receiver's library at decode
time, and removing a referenced hash makes the palimpsest undecodable.

**Cadence is in bytes.** The cadence `k` is a whole number of bytes. A bit-level cadence would
generally produce a palimpsest whose length is not a whole number of bytes (`H + k*(n − 1)` bits may
not be divisible by 8) and is therefore not used; a byte cadence keeps the palimpsest length always
an exact number of bytes.

## 4. Encoding

### 4.1 Construction

Given a sequence of hashes `h₀, h₁, …, hₙ₋₁` and a cadence `k`:

1. Allocate a zero-filled byte array `P` of length `L = H + k*(n − 1)`.
2. For each `i` in `0..n`, XOR `hᵢ` into `P` starting at byte offset `k*i`:

   ```
   P[k*i + j] ^= hᵢ[j]   for j in 0..H
   ```

3. The result `P` is the palimpsest.

The case `n = 1` produces a palimpsest of length `L = H` whose bytes are exactly the bytes of `h₀`.

### 4.2 Equivalent view via padded hashes

The same construction can be stated as: form `n` padded arrays, each of length `L`, where padded
hash `i` has `k*i` leading zero bytes, then the `H` bytes of `hᵢ`, then `k*(n − 1 − i)` trailing
zero bytes. The palimpsest is the XOR of all `n` padded arrays. XOR is commutative and associative,
so order does not matter.

### 4.3 Structural property

The first `k` bytes of `P` equal the first `k` bytes of `h₀`, because only `h₀` contributes to
those positions (all other padded hashes start with at least `k` zero bytes). More generally, after
hashes `h₀, …, hᵢ₋₁` have been XORed out of `P`, the bytes at positions `k*i` through `k*i + k − 1`
equal the first `k` bytes of `hᵢ`: no earlier hash still contributes (each has been removed) and no
later hash contributes (each begins at offset `k*(i + 1)` or later). This property is the basis of
the decoding algorithm.

## 5. Decoding

### 5.1 Overview

The receiver holds the palimpsest `P` of length `L` and knows the cadence `k`. It derives
`n = (L − H) / k + 1`. `L − H` MUST be divisible by `k`; if it is not, the input is malformed.
The receiver reconstructs the sequence `h₀, h₁, …, hₙ₋₁` by iterating through positions
`i = 0, 1, …, n − 1`, at each step identifying `hᵢ` by the leading `k`-byte prefix at position `k*i`
in the (partially reduced) palimpsest, XORing `hᵢ` out of `P`, and continuing.

### 5.2 Algorithm

```
decode(P, k, library) -> sequence or failure:
  n = (len(P) − H) / k + 1
  result = []
  return search(P, k, library, 0, n, result)

search(P, k, library, i, n, result) -> sequence or failure:
  if i == n:
    if all bytes of P are zero: return result
    else: return failure

  candidates = library.lookup(P[k*i .. k*i + k])   // first k bytes at position i
  for each hash h in candidates:
    XOR h into P at offset k*i                     // P[k*i + j] ^= h[j] for j in 0..H
    outcome = search(P, k, library, i + 1, n, result + [h])
    if outcome != failure: return outcome
    XOR h into P at offset k*i                     // undo (XOR is self-inverse)

  return failure
```

### 5.3 Correctness

By §4.3, when `search` is entered at index `i`, the `k`-byte slice of `P` at offset `k*i` equals
the first `k` bytes of `hᵢ`. The library lookup therefore returns a superset containing `hᵢ`. Each
candidate is tried in turn; correct candidates extend toward a valid solution and incorrect ones
ultimately fail the all-zeros check at the base case (or run out of candidates at some deeper
position). Backtracking via the self-inverse property of XOR restores `P` to the state required to
try the next candidate.

A decoder MAY apply this algorithm in any equivalent form. In particular, an implementation that
proceeds left-to-right and selects the unique candidate at each step (when the library is
sufficiently sparse — see §6) avoids recursion entirely.

### 5.4 Termination

After all `n` hashes have been XORed out, `P` is all zeros: it was constructed by XORing exactly
those hashes into an initially zero buffer. The all-zeros check at the base case (`i == n`) serves
as an integrity check; a corrupted palimpsest or an incompatible candidate set causes the check to
fail.

### 5.5 Cadence transmission

The palimpsest does not embed its cadence; the cadence MUST be communicated to the receiver out of
band, either by a framing protocol, a header field external to the palimpsest, or a convention
agreed by sender and receiver. A receiver that attempts to decode with the wrong cadence will
either compute a non-integer `n` (and reject the input as malformed) or find that no valid hash
sequence reconstructs to all zeros (and report a decode failure).

## 6. Parameter Selection

### 6.1 Choosing the cadence

The cadence `k` controls both the compression ratio and the decoding cost.

- **Compression ratio.** A palimpsest is `H + k*(n − 1)` bytes versus `H*n` bytes for the naive
  encoding. The saving is `(n − 1)*(H − k)` bytes, positive for all `k < H`.
- **Decoding cost.** At each step `i`, the library lookup returns all hashes whose first `k` bytes
  match the leading prefix at position `k*i`. Under the uniform-distribution assumption (§7), the
  expected number of candidates is `N / 256^k`. False candidates require recursive exploration
  that ultimately backtracks.

The cadence SHOULD be chosen so that `N < 256^k`: the library is smaller than the number of
distinct `k`-byte prefixes. Under this condition the expected number of candidates per lookup is
less than one and decoding proceeds without backtracking in the typical case. The minimum cadence
satisfying this is:

```
k = ⌈log₂₅₆(N)⌉ = ⌈log₂(N) / 8⌉
```

The constant of proportionality between `k` and `log₂(N)` is `1/8`, reflecting that the cadence is
measured in bytes.

### 6.2 Examples

| Library size `N` | `log₂₅₆(N)` | Recommended `k` |
| ---------------- | ----------- | --------------- |
| 1 000            | 1.25        | 2               |
| 10 000 000       | 2.91        | 3               |
| 1 000 000 000    | 3.75        | 4               |

With `H = 32` (SHA-256):

| `N`     | `k` | `n` | Palimpsest size | Naive size | Ratio |
| ------- | --- | --- | --------------- | ---------- | ----- |
| 10⁷     | 3   | 100 | 329 bytes       | 3 200 bytes | 9.7× |
| 10³     | 2   | 100 | 230 bytes       | 3 200 bytes | 13.9× |
| 10³     | 1   | 100 | 131 bytes       | 3 200 bytes | 24.4× |

### 6.3 Empty sequences

The formula `L = H + k*(n − 1)` is defined only for `n ≥ 1`. An empty sequence is therefore not
representable as a palimpsest; a protocol that wishes to convey "no hashes" MUST do so by a
separate convention (for example, a length-zero envelope) outside the palimpsest format.

## 7. Requirements on the Hash Function

- **Fixed output length.** All hashes MUST be exactly `H` bytes.
- **Collision resistance.** No two distinct inputs should share the same hash. A collision would
  make the palimpsest ambiguous to decode.
- **Uniform distribution.** The hash output should be uniformly distributed across all possible
  `H`-byte values. This ensures that the library index buckets are populated evenly, making the
  average-case complexity analysis in §6.1 valid.

SHA-256 (`H = 32`) satisfies all three properties and is the motivating example used throughout
this specification.

## 8. Complexity Analysis

### 8.1 Encoding

Encoding is `O(n*H)`: for each of `n` hashes, XOR `H` bytes into the palimpsest.

### 8.2 Decoding (average case)

At each of the `n` steps, the library lookup returns an expected `N / 256^k` candidates. If `k` is
chosen per §6.1 so that `N / 256^k < 1`, the algorithm is expected to proceed without backtracking,
giving `O(n*H)` work plus the cost of library lookups.

### 8.3 Decoding (worst case)

In the worst case, multiple candidates match at each step and all but the correct one lead to dead
ends, giving exponential backtracking in the number of false candidates per step. Under the
uniform-distribution assumption of §7 and the cadence rule of §6.1, the probability of deep
backtracking is low; rigorous probabilistic bounds for non-uniform distributions are out of scope
of this specification.

### 8.4 Bucket count

The number of distinct `k`-byte prefixes is `256^k = 2^(8k)`. Under uniform distribution, the
expected occupancy of any single bucket is `N / 256^k`.

## 9. Protocol Context

A palimpsest is intended for use in a protocol where:

- A **sender** (typically a client) holds a subset of the hashes in the receiver's library.
- The sender wishes to communicate an ordered sequence of hashes to the **receiver** (typically a
  server) as compactly as possible.
- The receiver holds the superset of hashes and maintains an index by `k`-byte prefix.
- The receiver performs the computationally more expensive decoding; the sender need only XOR.

## 10. Reference Implementation

A reference Rust implementation conforming to this specification is provided in the companion
`palimpsest` crate. It exposes:

- `Hash` — a 32-byte SHA-256 hash.
- `Palimpsest` — carries the encoded bytes and the cadence used to produce them, so it is
  self-describing for decoding within a process.
- `Bibliography` — a hash library indexed by `k`-byte prefix; cadence is supplied at construction.
- `encode(hashes: &[Hash], cadence: usize) -> Palimpsest` — implements §4.
- `decode(palimpsest: &Palimpsest, bibliography: &Bibliography) -> Option<Vec<Hash>>` — implements
  §5; returns `None` if reconstruction fails (missing hashes, malformed length, or no valid path).

The implementation supports any cadence `k` in `1..H`. For `n = 1` the palimpsest is exactly the
32-byte hash, in accordance with §4.1.

## 11. References

- RFC 2119: Key words for use in RFCs to Indicate Requirement Levels.
- RFC 8174: Ambiguity of Uppercase vs Lowercase in RFC 2119 Key Words.
- FIPS 180-4: Secure Hash Standard (SHA-256).
