# Palimpsest Specification

## 1. Abstract

A _palimpsest_ is a compact binary encoding of an ordered sequence of fixed-length cryptographic
hashes, drawn from a larger known set. By exploiting the fact that both sender and receiver share
access to the same hash library, a palimpsest encodes a sequence of `n` hashes in `32 + k*(n-1)`
bytes rather than the naive `32*n` bytes, where `k` is the _cadence_, a small integer (typically
1–3) chosen relative to the library size. The space saving comes at the cost of additional
computation during decoding: the receiver must search its hash library to reconstruct the original
sequence.

The name _palimpsest_ is used because the encoding layers the hashes over one another (each offset
by `k` bytes — the cadence — from the previous), so each hash is partially overwritten by its
neighbours in the XOR superposition — echoing the palimpsests of manuscript tradition where earlier
writing shows through later layers.

---

## 2. Definitions

| Symbol | Meaning                                                                                              |
| ------ | ---------------------------------------------------------------------------------------------------- |
| `H`    | Hash length in bytes. For SHA-256, `H = 32`.                                                         |
| `n`    | Number of hashes in the sequence to encode. `n ≥ 1` (the empty sequence is not encodable).           |
| `k`    | _Cadence_: the byte offset between successive hashes in the palimpsest. `1 ≤ k < H`. See note below. |
| `hᵢ`   | The i-th hash in the sequence (0-indexed), a byte array of length `H`.                               |
| `N`    | The total number of hashes in the receiver's library.                                                |
| `L`    | Length of the palimpsest in bytes: `L = H + k*(n-1)`.                                                |

**Normative constraints.** Encoders and decoders MUST enforce `n ≥ 1` and `1 ≤ k < H`. An empty
input sequence (`n = 0`) is not representable as a palimpsest; protocols that need to convey
"no hashes" MUST signal that out-of-band rather than by emitting a zero-length palimpsest.
A cadence `k ≥ H` would yield a palimpsest no smaller than the naive concatenation of hashes
(`L = H + k·(n−1) ≥ H·n` for `n ≥ 2`) and provides no compression benefit; the constraint `k < H`
ensures the encoding is strictly more compact than the naive form.

**Note on bit vs. byte cadence**: In principle, the cadence `k` could be measured in bits rather
than bytes, allowing finer-grained control over the compression/cost trade-off. However, a bit-level
cadence would generally produce a palimpsest whose length is not a whole number of bytes (since
`H + k*(n-1)` bits may not be divisible by 8), making the encoding awkward to handle in practice. A
byte cadence is therefore used, keeping the palimpsest length always an exact number of bytes.

**Hash library**: A collection of `N` known hashes, held by the receiver (server). The library is
indexed by prefix: for a given sequence of `k` bytes `b`, the receiver can efficiently enumerate all
hashes `h` in the library for which `h[0..k-1] == b`.

**Precondition — monotonic growth**: Hashes may be added to the library at any time. Hashes must
_never_ be removed. A palimpsest encodes references to hashes that must be present in the receiver's
library at decode time; removing a referenced hash makes the palimpsest undecodable.

---

## 3. Encoding

### 3.1 Construction

Given a sequence of hashes `h₀, h₁, ..., hₙ₋₁` and cadence `k`:

1. Allocate a zero-filled byte array `P` of length `L = H + k*(n-1)`.
2. For each `i` in `0..n`, XOR `hᵢ` into `P` starting at byte offset `k*i`:

   ```
   P[k*i + j] ^= hᵢ[j]   for j in 0..H
   ```

3. The result `P` is the palimpsest.

### 3.2 Equivalent view via padded hashes

The same construction can be stated as: form `n` padded arrays, each of length `L`, where padded
hash `i` has `k*i` leading zero bytes, then the `H` bytes of `hᵢ`, then `k*(n-1-i)` trailing zero
bytes. The palimpsest is the XOR of all `n` padded arrays. The XOR is commutative and associative,
so order does not matter.

### 3.3 Structural property

The first `k` bytes of `P` equal `h₀[0..k-1]`, because only `h₀` contributes to those positions (all
other padded hashes start with at least `k` zero bytes). More generally, position `k*i` receives
contributions from `hᵢ` and the following `⌊(H-1)/k⌋` hashes, but `hᵢ[0]` is the _sole_ contribution
from any hash whose first byte of significant data falls at that position. Consequently, the leading
`k` bytes at position `k*i` in any partially-reduced palimpsest uniquely identify the first `k`
bytes of the hash that belongs at that position.

---

## 4. Decoding

### 4.1 Overview

The receiver holds the palimpsest `P` of length `L` and knows the cadence `k`. It can derive
`n = (L - H) / k + 1` (which must be a positive integer; `L - H` must be divisible by `k`). The
receiver reconstructs the sequence `h₀, h₁, ..., hₙ₋₁` by iterating through positions
`i = 0, 1, ..., n-1`, at each step identifying `hᵢ` using the library index, XORing `hᵢ` out of `P`,
and continuing.

### 4.2 Algorithm

```
decode(P, k, library) -> sequence or failure:
  n = (len(P) - H) / k + 1
  result = []
  return search(P, k, library, 0, n, result)

search(P, k, library, i, n, result) -> sequence or failure:
  if i == n:
    if all bytes of P are zero: return result
    else: return failure

  candidates = library.lookup(P[k*i .. k*i + k - 1])  // first k bytes at position i
  for each hash h in candidates:
    XOR h into P at offset k*i   // P[k*i + j] ^= h[j] for j in 0..H
    outcome = search(P, k, library, i+1, n, result + [h])
    if outcome != failure: return outcome
    XOR h into P at offset k*i   // undo (XOR is self-inverse)

  return failure
```

### 4.3 Correctness of the key step

When `search` is called with index `i`, the bytes of `P` at positions `k*i` through `k*i + k - 1`
equal `hᵢ[0..k-1]`. This is because:

- Hashes `h₀, ..., hᵢ₋₁` have already been XORed out of `P`.
- Hash `hᵢ` contributes its bytes starting at offset `k*i`; its first `k` bytes appear uncontested
  at `P[k*i..k*i+k-1]`.
- Hashes `hᵢ₊₁, ..., hₙ₋₁` contribute to `P[k*(i+1)..]` and higher, not to `P[k*i..k*i+k-1]`.

Thus the `k`-byte prefix at position `i` uniquely identifies the first `k` bytes of `hᵢ`, allowing
the library lookup to narrow candidates.

### 4.4 Termination and correctness check

After all `n` hashes have been XORed out, `P` should be all zeros (since `P` was constructed as the
XOR of exactly those hashes). The all-zeros check at the base case (`i == n`) therefore serves as an
integrity check: it confirms that the selected set of hashes is consistent with the palimpsest. A
corrupted palimpsest or a missing hash will cause the check to fail, causing the algorithm to
backtrack or ultimately return failure.

**Ambiguity.** If two distinct hash sequences both leave `P` all-zero at the base case, the
palimpsest is structurally ambiguous. With a collision-resistant hash (§7) this requires a hash
collision and is computationally infeasible; a conforming decoder MAY treat the ambiguous case as
a failure (returning no result) or MAY return the lexicographically first valid sequence
(comparing hashes as byte arrays, position by position). The choice is implementation-defined,
but a decoder MUST be deterministic across repeated invocations on the same inputs.

### 4.5 Backtracking

If a candidate hash `h` leads to eventual failure deeper in the recursion, it is XORed back out
(undone) and the next candidate is tried. This backtracking is correct because XOR is its own
inverse.

---

## 5. Parameter Selection

### 5.1 Choosing the cadence `k`

The cadence `k` controls both the compression ratio and the decoding cost:

- **Compression ratio**: A palimpsest is `H + k*(n-1)` bytes versus `H*n` bytes naive. The saving is
  `(n-1)*(H - k)` bytes. This is positive for all `k < H`.

- **Decoding cost**: At each step `i`, the library lookup returns all hashes whose first `k` bytes
  match. The expected number of candidates is `N / 256^k` (assuming uniform distribution of hashes).
  Each false candidate may cause a recursive search that eventually backtracks.

**The cadence SHOULD be chosen so that `N < 256^k`**, i.e. the number of hashes in the library is
less than the number of distinct `k`-byte prefixes. This ensures that the expected number of
candidates per lookup is less than one, so decoding proceeds without backtracking in the typical
case.

If the cadence is too small (i.e. `N ≥ 256^k`), multiple hashes will share each prefix on average.
Every such collision requires the decoder to explore a branch that may ultimately dead-end, and the
resulting backtracking can degrade decoding performance significantly — in the worst case
exponentially in the number of candidates per step.

The minimum cadence satisfying `N < 256^k` is:

```
k = ⌈log₂₅₆(N)⌉ = ⌈log₂(N) / 8⌉
```

### 5.2 Examples

| Library size `N` | `log₂₅₆(N)` | Recommended `k` |
| ---------------- | ----------- | --------------- |
| 1,000            | 1.25        | 2               |
| 10,000,000       | 2.91        | 3               |
| 1,000,000,000    | 3.75        | 4               |

### 5.3 Size examples

With `H = 32` (SHA-256):

| `N`  | `k` | `n` | Palimpsest size | Naive size | Ratio |
| ---- | --- | --- | --------------- | ---------- | ----- |
| 10M  | 3   | 100 | 329 bytes       | 3200 bytes | 9.7×  |
| 1000 | 1   | 100 | 131 bytes       | 3200 bytes | 24.4× |

**Note**: The intro document uses `k = 1` for `N = 1000`, which gives 256 buckets for 1000 hashes
(~4 hashes per bucket). Using `k = 2` (65,536 buckets) would give fewer than one hash per bucket on
average and substantially lower decoding cost. `k = 1` is workable but not optimal for this library
size.

---

## 6. Complexity Analysis

### 6.1 Encoding

Encoding is `O(n * H)`: for each of `n` hashes, XOR `H` bytes into the palimpsest.

### 6.2 Decoding (average case)

At each of the `n` steps, the library lookup returns an expected `N / 256^k` candidates. If `k` is
chosen so that `N / 256^k ≈ 1`, the algorithm is expected to proceed without backtracking, giving
`O(n * H)` work for decoding (plus the cost of library lookups).

### 6.3 Decoding (worst case)

In the worst case, many candidates match at each step and all but the correct one lead to dead ends,
giving exponential backtracking. However, because the hash function is assumed to distribute values
uniformly, the probability of deep backtracking is low when `k` is chosen appropriately.

> **UNRESOLVED**: The original document flags this analysis as incomplete ("FIXME: work on this
> analysis some more"). A rigorous probabilistic bound on the expected number of recursive calls has
> not been established. In particular, the analysis of non-uniform hash distributions — where some
> buckets are denser than average — requires further work.

### 6.4 Bucket count

The number of distinct `k`-byte prefixes is `256^k` (since each byte takes 256 values). The intro
document states "2^k distinct buckets" which is incorrect; the correct value is `256^k = 2^(8k)`.

---

## 7. Requirements on the Hash Function

- **Fixed output length**: All hashes must be exactly `H` bytes.
- **Collision resistance**: No two distinct data chunks should share the same hash. A collision
  would make the palimpsest ambiguous to decode.
- **Uniform distribution**: The hash output should be uniformly distributed across all possible
  `H`-byte values. This ensures that the library index buckets are populated evenly, making the
  average-case complexity analysis valid.

SHA-256 (`H = 32`) satisfies all three properties and is used as the motivating example throughout
this document.

---

## 8. Protocol Context

A palimpsest is intended for use in a protocol where:

- A _sender_ (client) holds a subset of the hashes in the receiver's library.
- The sender wishes to communicate an ordered sequence of hashes to the _receiver_ (server) as
  compactly as possible.
- The receiver holds a strictly larger set of hashes and maintains an index by `k`-byte prefix.
- The receiver performs the computationally more expensive decoding; the sender only needs to XOR.

**Cadence is not embedded.** The palimpsest byte sequence does not carry the cadence `k`; sender
and receiver MUST agree `k` out-of-band. Typical mechanisms include a framing header, a protocol
constant, or a per-deployment configuration value. A receiver that assumes the wrong cadence will
fail decoding (the integrity check of §4.4 will reject every candidate). Specifications that
embed a palimpsest within another format are RESPONSIBLE for fixing or communicating `k`; for
example, the BinTEL specification fixes `k = 2` for all schema signatures.

---

## 9. Open Items

The following remain as deferred analytical work, not as gaps in the encoding contract:

- **Tight worst-case bound on backtracking depth.** The average-case analysis of §6.2 establishes
  that backtracking is rare when `k ≥ ⌈log₂₅₆(N)⌉`, but a closed-form probabilistic bound on the
  expected number of recursive calls under non-uniform hash distributions, and a formal
  worst-case bound, are not given. Implementations SHOULD impose an explicit recursion-depth
  limit (e.g. `2·n`) to guarantee termination on pathological inputs.
- **Correlated hashes.** The complexity analysis assumes uniformly distributed hashes. When the
  hash function's inputs are correlated (for example, near-duplicate source data), bucket
  occupancy may deviate from uniform; the effect on decoding cost has not been characterised.

All other items previously listed as open (encoding for `n = 0`, communication of the cadence,
the `2^k`-vs-`256^k` arithmetic, the `k < H` constraint, the bandwidth-factor wording, the
recommended cadence for small libraries, the base-case condition in decoding) are resolved by
the normative text of §2 through §8 of this revision.

---

## 10. Reference Implementation

The Rust implementation in `src/lib.rs` conforms to this specification. It exposes:

- `pub struct Hash([u8; 32])` — a SHA-256 hash.
- `pub struct Palimpsest` — carries the encoded bytes plus its cadence so it is self-describing for
  decoding.
- `pub struct Bibliography` — a hash library indexed by `k`-byte prefix; cadence is supplied at
  construction.
- `pub fn encode(hashes: &[Hash], cadence: usize) -> Palimpsest` — implements §3.
- `pub fn decode(palimpsest: &Palimpsest, bibliography: &Bibliography) -> Option<Vec<Hash>>` —
  implements §4. Returns `None` if reconstruction fails (missing hashes, malformed length, or no
  valid path).

The implementation supports any cadence `k` in `1..H`. For `n = 1` (a single hash with no layers)
the palimpsest is exactly the 32-byte hash.

Sixteen unit tests cover the length formula at three cadences, full-cycle round-trips, the
single-hash special case, library-extra-hashes scenarios, decode failures (missing hash,
truncated input, empty sequence), the invariants checked by `encode`'s asserts, and the
`Bibliography`'s prefix-lookup semantics.
