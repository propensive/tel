# Palimpsest Specification

## 1. Abstract

A _palimpsest_ is a compact binary encoding of an ordered sequence of fixed-length cryptographic
hashes, drawn from a larger known set. By exploiting the fact that both sender and receiver share
access to the same hash library, a palimpsest encodes a sequence of `n` hashes in
`H + k_i + k_r·(n − 2) + 1` bytes (for `n ≥ 2`) or `H + 1` bytes (for `n = 1`), rather than the
naive `H·n` bytes, where `H` is the hash length in bytes, `k_i` is the _initial cadence_ (the
offset between the first and second hashes), and `k_r` is the _regular cadence_ (the offset
between subsequent hashes). The space saving comes at the cost of additional computation during
decoding: the receiver must search its hash library to reconstruct the original sequence.

The palimpsest is **self-describing**: its trailing byte encodes the hash length and both
cadences, and is selected so that the XOR of every byte of the palimpsest equals that same
byte's encoded value. A receiver therefore needs no out-of-band parameters to decode a
palimpsest beyond access to the shared hash library; the trailing byte also serves as a one-byte
integrity check.

The name _palimpsest_ is used because the encoding layers the hashes over one another (each
offset from the previous by `k_i` or `k_r` bytes — the cadence), so each hash is partially
overwritten by its neighbours in the XOR superposition — echoing the palimpsests of manuscript
tradition where earlier writing shows through later layers.

---

## 2. Definitions

| Symbol | Meaning                                                                                                  |
| ------ | -------------------------------------------------------------------------------------------------------- |
| `H`    | Hash length in bytes. Determined by the hash-size index `s` (see §2.1). `H ∈ {8, 10, 12, 16, 20, 24, 28, 32, 48, 64}`. |
| `n`    | Number of hashes in the sequence to encode. `n ≥ 1` (the empty sequence is not encodable).               |
| `k_r`  | _Regular cadence_: the byte offset between hashes `hᵢ` and `hᵢ₊₁` for `i ≥ 1`. `1 ≤ k_r ≤ 4`.            |
| `k_i`  | _Initial cadence_: the byte offset between `h₀` and `h₁`. `k_r ≤ k_i ≤ k_r + 3` (i.e. `k_i − k_r ∈ {0, 1, 2, 3}`). |
| `hᵢ`   | The i-th hash in the sequence (0-indexed), a byte array of length `H`.                                   |
| `oᵢ`   | The byte offset of `hᵢ` within the palimpsest body. `o₀ = 0`; for `i ≥ 1`, `oᵢ = k_i + (i − 1)·k_r`.     |
| `N`    | The total number of hashes in the receiver's library.                                                    |
| `c`    | The _cadence byte_: the trailing byte of the palimpsest, packing `(k_r, k_i − k_r, s)`. See §2.1.        |
| `s`    | The _hash-size index_, a 4-bit value in `{0..9}`; values `{10..15}` are reserved.                        |
| `L`    | Length of the palimpsest in bytes, including the cadence byte: `L = H + 1` for `n = 1`, otherwise `L = H + k_i + k_r·(n − 2) + 1`. |

**Normative constraints.** Encoders and decoders MUST enforce `n ≥ 1`, `1 ≤ k_r ≤ 4`,
`0 ≤ k_i − k_r ≤ 3`, and `s ∈ {0..9}`. An empty input sequence (`n = 0`) is not representable as
a palimpsest; protocols that need to convey "no hashes" MUST signal that out-of-band rather than
by emitting a zero-length palimpsest.

### 2.1 Cadence byte layout

The cadence byte `c` packs the three parameters as follows (bit 0 = least-significant bit):

| Bits  | Field          | Encoding                                                                |
| ----- | -------------- | ----------------------------------------------------------------------- |
| 0 – 1 | `k_r − 1`      | `00` → `k_r = 1`, `01` → 2, `10` → 3, `11` → 4                          |
| 2 – 3 | `k_i − k_r`    | `00` → 0, `01` → 1, `10` → 2, `11` → 3 (giving `k_i ∈ {k_r..k_r + 3}`)  |
| 4 – 7 | `s`            | Hash-size index; see hash-size table below                              |

Hash-size table:

| `s` | bits | bytes (`H`) |
| --- | ---- | ----------- |
| 0   | 64   | 8           |
| 1   | 80   | 10          |
| 2   | 96   | 12          |
| 3   | 128  | 16          |
| 4   | 160  | 20          |
| 5   | 192  | 24          |
| 6   | 224  | 28          |
| 7   | 256  | 32          |
| 8   | 384  | 48          |
| 9   | 512  | 64          |
| 10 – 15 | — | reserved    |

A decoder that observes `s ∈ {10..15}` MUST reject the palimpsest as malformed. Reserved values
exist so that future revisions of this specification can register additional hash sizes without
a format break.

**Why these sizes?** The values are conventional output sizes of BLAKE3 and of the SHA-2 / SHA-3
families; choosing them keeps the table inter-operable with hash output widths that implementers
will already be familiar with.

### 2.2 Hash library

A **hash library** is a collection of `N` known hashes, held by the receiver (server). The
library is indexed by prefix: for a given sequence of `k` bytes `b`, the receiver can efficiently
enumerate all hashes `h` in the library for which `h[0..k − 1] == b`. A receiver that wishes to
decode palimpsests with several different cadences SHOULD maintain its index keyed on the longest
plausible prefix (the maximum `k_i`), since shorter-prefix lookups can be derived from
longer-prefix ones.

**Precondition — monotonic growth.** Hashes may be added to the library at any time. Hashes must
_never_ be removed. A palimpsest encodes references to hashes that must be present in the
receiver's library at decode time; removing a referenced hash makes the palimpsest undecodable.

---

## 3. Encoding

### 3.1 Construction

Given a sequence of hashes `h₀, h₁, …, hₙ₋₁`, an initial cadence `k_i`, a regular cadence `k_r`,
and a hash-size index `s` consistent with the hashes' actual byte length `H`:

1. Compute `L_data = H` if `n = 1`, otherwise `L_data = H + k_i + k_r·(n − 2)`. Allocate a
   zero-filled byte array `B` of length `L_data` (the palimpsest **body**).
2. For each `i` in `0..n`, XOR `hᵢ` into `B` starting at byte offset `oᵢ`:

   ```
   B[oᵢ + j] ^= hᵢ[j]   for j in 0..H
   ```

3. Form the cadence byte `c` by packing `(k_r − 1, k_i − k_r, s)` per §2.1.
4. Let `D = B[0] ⊕ B[1] ⊕ … ⊕ B[L_data − 1]` (the XOR of every body byte).
5. Append the trailing byte `z = D ⊕ c` to `B`, producing the palimpsest `P` of length
   `L = L_data + 1`. By construction, the XOR of every byte of `P` equals `c`.

### 3.2 Equivalent view via padded hashes

The body can be stated equivalently as: form `n` padded arrays, each of length `L_data`, where
padded hash `i` has `oᵢ` leading zero bytes, then the `H` bytes of `hᵢ`, then `L_data − oᵢ − H`
trailing zero bytes. The body is the XOR of all `n` padded arrays. The XOR is commutative and
associative, so order does not matter.

### 3.3 Structural property

The first `k_i` bytes of `P` equal `h₀[0..k_i − 1]`, because only `h₀` contributes to those
positions (`h₁`'s padded form starts with at least `k_i` zero bytes, and every later hash starts
even further right). More generally, position `oᵢ` receives contributions from `hᵢ` and from
each later hash `hⱼ` (`j > i`) whose offset `oⱼ` satisfies `oⱼ ≤ oᵢ + H − 1`, but `hᵢ[0]` is the
_sole_ contribution from any hash whose first byte of significant data falls at that position.
Consequently, the bytes at position `oᵢ` of any partially-reduced palimpsest body — at depths
`k_i` bytes (for `i = 0`) or `k_r` bytes (for `i ≥ 1`) — uniquely identify the corresponding
prefix of `hᵢ`.

---

## 4. Decoding

### 4.1 Overview

The receiver holds the palimpsest `P` of length `L`. It first recovers the cadence byte `c` by
XOR-folding every byte of `P`, then unpacks `c` into `(k_r, k_i, H)`, computes the number of
hashes `n` from `L`, and finally runs the recursive search procedure to reconstruct
`h₀, h₁, …, hₙ₋₁`.

### 4.2 Parameter recovery

1. Compute `c = P[0] ⊕ P[1] ⊕ … ⊕ P[L − 1]`. This is the cadence byte.
2. Unpack `c` per §2.1 into `(k_r, k_i, s)`. Reject the palimpsest if `s ∈ {10..15}` (reserved
   hash size).
3. Look up `H = bytes(s)` from the table in §2.1.
4. Let `L_data = L − 1`. Solve for `n`:
   - If `L_data = H`, then `n = 1`.
   - If `L_data ≥ H + k_i` and `(L_data − H − k_i) mod k_r = 0`, then
     `n = 2 + (L_data − H − k_i) / k_r`.
   - Otherwise the byte length is inconsistent with the cadence byte; reject as malformed.

### 4.3 Recursive search

Let `B = P[0..L_data − 1]` denote the palimpsest body. The recursive search reconstructs the
hash sequence by iterating through positions `i = 0, 1, …, n − 1`, at each step identifying
`hᵢ` using the library index, XORing `hᵢ` out of `B`, and continuing.

```
decode(P, library) -> sequence or failure:
  c = XOR of every byte of P
  (k_r, k_i, H) = unpack(c)            // reject reserved s
  L_data = len(P) - 1
  n = derive_n(L_data, H, k_i, k_r)    // §4.2
  B = P[0..L_data - 1]                 // mutable copy
  result = []
  return search(B, k_i, k_r, H, library, 0, n, result)

search(B, k_i, k_r, H, library, i, n, result) -> sequence or failure:
  if i == n:
    if all bytes of B are zero: return result
    else: return failure

  oᵢ = 0 if i == 0 else k_i + (i - 1)·k_r
  pᵢ = k_i if i == 0 else k_r          // length of the unambiguous prefix
  candidates = library.lookup(B[oᵢ .. oᵢ + pᵢ - 1])
  for each hash h in candidates:
    XOR h into B at offset oᵢ          // B[oᵢ + j] ^= h[j] for j in 0..H
    outcome = search(B, k_i, k_r, H, library, i+1, n, result + [h])
    if outcome != failure: return outcome
    XOR h into B at offset oᵢ          // undo (XOR is self-inverse)

  return failure
```

### 4.4 Correctness of the key step

When `search` is called with index `i`, the bytes of `B` at positions `oᵢ` through
`oᵢ + pᵢ − 1` equal `hᵢ[0..pᵢ − 1]`. This is because:

- Hashes `h₀, …, hᵢ₋₁` have already been XORed out of `B`.
- Hash `hᵢ` contributes its bytes starting at offset `oᵢ`; its first `pᵢ` bytes appear
  uncontested at `B[oᵢ..oᵢ + pᵢ − 1]` because the next hash `hᵢ₊₁` starts at offset
  `oᵢ + pᵢ` (= `oᵢ + k_i` when `i = 0`, and `oᵢ + k_r` otherwise) and every later hash starts
  later still.
- Hashes `hᵢ₊₁, …, hₙ₋₁` contribute to `B[oᵢ + pᵢ..]` and higher, not to
  `B[oᵢ..oᵢ + pᵢ − 1]`.

Thus the `pᵢ`-byte prefix at position `i` uniquely identifies the first `pᵢ` bytes of `hᵢ`,
allowing the library lookup to narrow candidates.

### 4.5 Termination and correctness check

After all `n` hashes have been XORed out, `B` should be all zeros (since `B` was constructed as
the XOR of exactly those hashes). The all-zeros check at the base case (`i == n`) therefore
serves as an integrity check on the body: it confirms that the selected set of hashes is
consistent with the palimpsest. A corrupted palimpsest or a missing hash will cause the check
to fail, causing the algorithm to backtrack or ultimately return failure.

A second, cheaper integrity check is implicit in §4.2: if any byte of `P` is corrupted in
transit, the trailing-byte XOR property of §3 no longer holds, and the recovered `c` will
deviate from the encoder's intended cadence byte. A decoder MAY perform this check explicitly by
verifying that the recovered `(k_r, k_i, s)` are consistent with the apparent byte length per
§4.2; the spec requires no additional CRC.

**Ambiguity.** If two distinct hash sequences both leave `B` all-zero at the base case, the
palimpsest is structurally ambiguous. With a collision-resistant hash (§7) this requires a hash
collision and is computationally infeasible; a conforming decoder MAY treat the ambiguous case
as a failure (returning no result) or MAY return the lexicographically first valid sequence
(comparing hashes as byte arrays, position by position). The choice is implementation-defined,
but a decoder MUST be deterministic across repeated invocations on the same inputs.

### 4.6 Backtracking

If a candidate hash `h` leads to eventual failure deeper in the recursion, it is XORed back out
(undone) and the next candidate is tried. This backtracking is correct because XOR is its own
inverse.

---

## 5. Parameter Selection

### 5.1 Choosing the cadences

Because the palimpsest has two cadences, the encoder has two distinct levers:

- **Initial cadence `k_i`** controls how many uncontested bytes are available at offset `0` to
  identify `h₀`. With a library of `N` hashes, choosing `k_i` so that `N < 256^{k_i}` means the
  expected number of candidates for `h₀` is less than one and the base-hash lookup proceeds
  without backtracking. Because identifying `h₀` correctly is what makes every subsequent step
  unambiguous, errors at this step are the most expensive to recover from, and `k_i` should be
  chosen generously.

- **Regular cadence `k_r`** controls the size cost per additional hash and the expected number
  of candidates at each subsequent step. After `hᵢ` is identified, the next step has `k_r`
  uncontested prefix bytes; choosing `k_r` so that `N < 256^{k_r}` likewise keeps the expected
  candidate count below one.

Because hashes added to the library after the encoder commits to `(k_r, k_i)` may push `N`
above `256^{k_r}`, encoders should choose `k_r` with some headroom relative to the current
library size.

### 5.2 Size cost

For `n ≥ 2`, the palimpsest is `L = H + k_i + k_r·(n − 2) + 1` bytes versus `H·n` bytes naive.
The saving is `(H − k_r)·(n − 1) + (k_r − k_i) − 1` bytes — positive for any `k_r < H` once
`n` is moderately large.

### 5.3 Examples

| Library size `N` | `log₂₅₆(N)` | Recommended `k_i` | Recommended `k_r` |
| ---------------- | ----------- | ----------------- | ----------------- |
| 1,000            | 1.25        | 2                 | 2                 |
| 10,000,000       | 2.91        | 3                 | 3                 |
| 1,000,000,000    | 3.75        | 4                 | 4                 |

For mixed regimes — where the base-hash library is large but extension hashes draw from a much
smaller per-base library — `k_i > k_r` is often the right choice. The BinTEL specification, for
example, uses `k_i = 4, k_r = 2` because schema base components are drawn from a global library
but each base's layer extensions come from a much smaller per-schema set.

### 5.4 Size examples

With `H = 32` (256-bit hash):

| `N`  | `k_i` | `k_r` | `n` | Palimpsest size | Naive size | Ratio |
| ---- | ----- | ----- | --- | --------------- | ---------- | ----- |
| 10M  | 3     | 3     | 100 | 330 bytes       | 3200 bytes | 9.7×  |
| 1000 | 2     | 2     | 100 | 231 bytes       | 3200 bytes | 13.9× |
| 4G   | 4     | 2     | 100 | 233 bytes       | 3200 bytes | 13.7× |

---

## 6. Complexity Analysis

### 6.1 Encoding

Encoding is `O(n·H)`: for each of `n` hashes, XOR `H` bytes into the body, plus an `O(L)` pass
to compute the trailing-byte XOR.

### 6.2 Decoding (average case)

At the first step the library lookup returns an expected `N / 256^{k_i}` candidates; at every
subsequent step, an expected `N / 256^{k_r}` candidates. If `k_i` and `k_r` are chosen so that
both expected counts are `≲ 1`, the algorithm is expected to proceed without backtracking,
giving `O(n·H)` work for decoding (plus the cost of library lookups).

### 6.3 Decoding (worst case)

In the worst case, many candidates match at each step and all but the correct one lead to dead
ends, giving exponential backtracking. However, because the hash function is assumed to
distribute values uniformly, the probability of deep backtracking is low when the cadences are
chosen appropriately.

> **UNRESOLVED**: A rigorous probabilistic bound on the expected number of recursive calls
> under non-uniform hash distributions, and a formal worst-case bound, are not given. In
> particular, the analysis of non-uniform hash distributions — where some buckets are denser
> than average — requires further work.

### 6.4 Bucket count

The number of distinct prefixes used at the first step is `256^{k_i}` and at each subsequent
step is `256^{k_r}` (since each byte takes 256 values).

---

## 7. Requirements on the Hash Function

- **Selectable output length**: The hash function must support every length `H` listed in §2.1
  that the implementation needs to handle, with the output at each length being a deterministic
  truncation / extension of a single underlying construction.
- **Collision resistance**: No two distinct data chunks should share the same hash at the chosen
  output length. A collision would make the palimpsest ambiguous to decode.
- **Uniform distribution**: The hash output should be uniformly distributed across all possible
  `H`-byte values. This ensures that the library index buckets are populated evenly, making the
  average-case complexity analysis valid.

**BLAKE3** satisfies all three properties and is the recommended hash function for new
deployments. BLAKE3 produces an arbitrary-length output from a single hashing pass — the
truncation to any of the table sizes in §2.1 is the prefix of the same extended output — so a
single implementation can drive every entry in the hash-size table without having to compose
distinct fixed-output algorithms. The BinTEL specification (§3) pins its value hash to 256-bit
BLAKE3 (`s = 7`, `H = 32`).

---

## 8. Protocol Context

A palimpsest is intended for use in a protocol where:

- A _sender_ (client) holds a subset of the hashes in the receiver's library.
- The sender wishes to communicate an ordered sequence of hashes to the _receiver_ (server) as
  compactly as possible.
- The receiver holds a strictly larger set of hashes and maintains an index by prefix.
- The receiver performs the computationally more expensive decoding; the sender only needs to
  XOR.

**Cadence is embedded.** Unlike earlier revisions of this specification, the palimpsest byte
sequence is self-describing: the trailing byte recovers `(k_r, k_i, H)` without any framing
header or out-of-band agreement. A specification that embeds a palimpsest within another format
SHOULD nevertheless pin a single normative `(k_r, k_i, H)` for its own use, so that producers
and consumers agree on the parameter regime and so that a static analysis of message sizes is
possible. The BinTEL specification, for example, pins `(k_r, k_i, H) = (2, 4, 32)` for every
schema signature.

A receiver that decodes a palimpsest whose cadence byte names parameters incompatible with the
embedding protocol's normative values MUST reject it as a framing error of the embedding
protocol, even if the palimpsest is internally consistent.

---

## 9. Open Items

The following remain as deferred analytical work, not as gaps in the encoding contract:

- **Tight worst-case bound on backtracking depth.** The average-case analysis of §6.2
  establishes that backtracking is rare when the cadences are chosen so that
  `N ≲ 256^{k_i}` and `N ≲ 256^{k_r}`, but a closed-form probabilistic bound on the expected
  number of recursive calls under non-uniform hash distributions, and a formal worst-case
  bound, are not given. Implementations SHOULD impose an explicit recursion-depth limit (e.g.
  `2·n`) to guarantee termination on pathological inputs.
- **Correlated hashes.** The complexity analysis assumes uniformly distributed hashes. When the
  hash function's inputs are correlated (for example, near-duplicate source data), bucket
  occupancy may deviate from uniform; the effect on decoding cost has not been characterised.

All other items previously listed as open (encoding for `n = 0`, communication of the cadence,
the `2^k`-vs-`256^k` arithmetic, the cadence/hash-length constraints, the bandwidth-factor
wording, the recommended cadence for small libraries, the base-case condition in decoding) are
resolved by the normative text of §2 through §8 of this revision.

---

## 10. Reference Implementation

The Rust implementation in `src/lib.rs` is being updated to conform to this revision. The
intended interface is:

- `pub struct Hash(Vec<u8>)` — a BLAKE3 hash of one of the §2.1 lengths.
- `pub struct Palimpsest` — carries the encoded bytes; cadence and hash size are recovered from
  the trailing byte and so are not separately required.
- `pub struct Bibliography` — a hash library indexed by prefix; the prefix length used at
  construction time SHOULD be the maximum `k_i` the library is expected to serve.
- `pub fn encode(hashes: &[Hash], k_i: u8, k_r: u8) -> Palimpsest` — implements §3, deriving the
  hash size index from the hashes' shared length.
- `pub fn decode(palimpsest: &Palimpsest, bibliography: &Bibliography) -> Option<Vec<Hash>>` —
  implements §4. Returns `None` if reconstruction fails (missing hashes, malformed length,
  reserved hash-size index, or no valid path).

For `n = 1` (a single hash with no layers) the palimpsest is the `H` bytes of the hash followed
by the cadence byte, giving `H + 1` total bytes.
