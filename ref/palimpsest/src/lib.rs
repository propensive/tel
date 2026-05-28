//! Palimpsest: compact, self-describing binary encoding of an ordered sequence
//! of cryptographic hashes drawn from a shared library.
//!
//! See `../../../spec/palimpsest.md`. The implementation supports the
//! dual-cadence design of the v1 specification: an initial cadence `k_i`
//! between the first and second hashes, a regular cadence `k_r` between
//! subsequent hashes, and a trailing cadence byte that encodes
//! `(k_r, k_i − k_r, hash-size index)` and is selected so the XOR of every
//! palimpsest byte equals the cadence byte itself. The hash size is one of
//! the ten values listed in §2.1; BLAKE3-256 (32-byte) is recommended.

use std::collections::HashMap;

/// Lookup table for the hash-size index `s` (4 bits, §2.1). `s ∈ {0..9}` map
/// to hash byte lengths; `{10..15}` are reserved.
const HASH_LENS: [Option<usize>; 16] = [
    Some(8),   // s=0: 64 bits
    Some(10),  // s=1: 80 bits
    Some(12),  // s=2: 96 bits
    Some(16),  // s=3: 128 bits
    Some(20),  // s=4: 160 bits
    Some(24),  // s=5: 192 bits
    Some(28),  // s=6: 224 bits
    Some(32),  // s=7: 256 bits  ← BLAKE3-256, the BinTEL default
    Some(48),  // s=8: 384 bits
    Some(64),  // s=9: 512 bits
    None, None, None, None, None, None, // s=10..15: reserved
];

/// The byte length corresponding to hash-size index `s`, or `None` if `s` is
/// reserved.
pub fn hash_len_from_index(s: u8) -> Option<usize> {
    HASH_LENS.get(s as usize).copied().flatten()
}

/// The hash-size index for a given byte length. Panics if the length is not
/// one of the values in §2.1.
pub fn hash_size_index(hash_len: usize) -> u8 {
    HASH_LENS.iter().enumerate()
        .find_map(|(i, l)| if *l == Some(hash_len) { Some(i as u8) } else { None })
        .unwrap_or_else(|| panic!(
            "hash length {} is not one of the §2.1 sizes {:?}",
            hash_len,
            HASH_LENS.iter().filter_map(|x| *x).collect::<Vec<_>>(),
        ))
}

/// Pack the cadence byte `c` from its three fields (§2.1):
/// bits 0-1 = `k_r − 1`, bits 2-3 = `k_i − k_r`, bits 4-7 = hash-size index.
pub fn pack_cadence(k_r: u8, k_i: u8, hash_len: usize) -> u8 {
    assert!((1..=4).contains(&k_r), "k_r must be in 1..=4 (got {})", k_r);
    assert!(k_i >= k_r && k_i <= k_r + 3,
            "k_i must be in {}..={} (got {})", k_r, k_r + 3, k_i);
    let s = hash_size_index(hash_len);
    (s << 4) | ((k_i - k_r) << 2) | (k_r - 1)
}

/// Unpack the cadence byte `c` into `(k_r, k_i, hash_len)`. Returns `None`
/// if the encoded hash-size index is reserved.
pub fn unpack_cadence(c: u8) -> Option<(u8, u8, usize)> {
    let k_r = (c & 0b11) + 1;
    let diff = (c >> 2) & 0b11;
    let k_i = k_r + diff;
    let s = c >> 4;
    let hash_len = hash_len_from_index(s)?;
    Some((k_r, k_i, hash_len))
}

/// Offset of `hᵢ` within the palimpsest body. `o₀ = 0`; for `i ≥ 1`,
/// `oᵢ = k_i + (i − 1)·k_r`.
fn hash_offset(i: usize, k_i: u8, k_r: u8) -> usize {
    if i == 0 { 0 } else { k_i as usize + (i - 1) * k_r as usize }
}

/// Body length excluding the trailing cadence byte. `H` for `n = 1`,
/// otherwise `H + k_i + k_r·(n − 2)`.
fn body_len(n: usize, hash_len: usize, k_i: u8, k_r: u8) -> usize {
    if n == 1 { hash_len }
    else { hash_len + k_i as usize + k_r as usize * (n - 2) }
}

/// A cryptographic hash of one of the §2.1 lengths. Variable-length because
/// the palimpsest format supports hashes from 8 to 64 bytes; the BinTEL
/// embedding fixes the length at 32 (BLAKE3-256).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hash(Vec<u8>);

impl Hash {
    pub fn new(bytes: Vec<u8>) -> Self {
        assert!(hash_size_index_safe(bytes.len()).is_some(),
                "hash length {} is not one of the §2.1 sizes", bytes.len());
        Self(bytes)
    }
    pub fn bytes(&self) -> &[u8] { &self.0 }
    pub fn len(&self) -> usize { self.0.len() }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
}

fn hash_size_index_safe(hash_len: usize) -> Option<u8> {
    HASH_LENS.iter().enumerate()
        .find_map(|(i, l)| if *l == Some(hash_len) { Some(i as u8) } else { None })
}

impl From<[u8; 32]> for Hash {
    fn from(arr: [u8; 32]) -> Self { Self(arr.to_vec()) }
}

/// A palimpsest: the XOR-stacked encoding of a sequence of hashes plus a
/// trailing cadence byte. Self-describing — the byte stream alone is enough
/// to recover `(k_r, k_i, H)` and the hash count `n`.
#[derive(Debug, Clone, PartialEq)]
pub struct Palimpsest {
    bytes: Vec<u8>,
}

impl Palimpsest {
    pub fn bytes(&self) -> &[u8] { &self.bytes }

    /// Construct from raw bytes without validation. The bytes are assumed to
    /// already be a valid palimpsest; `decode` will reject malformed inputs.
    pub fn from_bytes(bytes: Vec<u8>) -> Self { Self { bytes } }

    /// Recover the cadence byte by XOR-folding every byte of the palimpsest.
    pub fn cadence_byte(&self) -> u8 {
        self.bytes.iter().fold(0u8, |acc, &b| acc ^ b)
    }

    /// Recover `(k_r, k_i, hash_len)` from the cadence byte. Returns `None`
    /// if the encoded hash-size index is reserved.
    pub fn parameters(&self) -> Option<(u8, u8, usize)> {
        unpack_cadence(self.cadence_byte())
    }

    /// Number of hashes encoded, or `None` if the byte length is inconsistent
    /// with the cadence byte.
    pub fn hash_count(&self) -> Option<usize> {
        let (k_r, k_i, h_len) = self.parameters()?;
        if self.bytes.len() < 2 { return None; }
        let body = self.bytes.len() - 1;
        if body == h_len { Some(1) }
        else if body >= h_len + k_i as usize
             && (body - h_len - k_i as usize) % k_r as usize == 0 {
            Some(2 + (body - h_len - k_i as usize) / k_r as usize)
        } else { None }
    }

    pub fn len(&self) -> usize { self.bytes.len() }
    pub fn is_empty(&self) -> bool { self.bytes.is_empty() }
}

/// Hash library indexed by one or more prefix lengths. Construct with the
/// set of cadence values you intend to look up; `add` and `lookup` use those
/// lengths to bucket hashes by their leading bytes.
///
/// For a palimpsest with initial cadence `k_i` and regular cadence `k_r`, the
/// bibliography MUST include both `k_i` and `k_r` in its supported cadences
/// (a single value if `k_i == k_r`).
#[derive(Debug, Clone)]
pub struct Bibliography {
    /// Map from prefix length → prefix bytes → matching hashes.
    indexes: HashMap<u8, HashMap<Vec<u8>, Vec<Hash>>>,
    /// Every hash added (for iteration and counting).
    all: Vec<Hash>,
}

impl Bibliography {
    /// Construct a bibliography that supports lookups at every prefix length
    /// in `cadences`. Duplicates are tolerated. Every cadence must be in
    /// `1..=4`.
    pub fn new(cadences: &[u8]) -> Self {
        let mut indexes = HashMap::new();
        for &k in cadences {
            assert!((1..=4).contains(&k),
                    "bibliography cadence must be in 1..=4 (got {})", k);
            indexes.entry(k).or_insert_with(HashMap::new);
        }
        Self { indexes, all: Vec::new() }
    }

    /// Convenience constructor for a bibliography that supports both the
    /// initial and regular cadences used by a single palimpsest.
    pub fn for_cadences(k_i: u8, k_r: u8) -> Self {
        Self::new(&[k_i, k_r])
    }

    /// Add a hash to the library. Indexed under every supported cadence.
    pub fn add(&mut self, hash: Hash) {
        let cadences: Vec<u8> = self.indexes.keys().copied().collect();
        for k in cadences {
            if hash.0.len() < k as usize {
                // Hash shorter than the cadence prefix — cannot be looked up
                // at this length; skip indexing under this k. (Still added
                // to `all` so callers see it in `len()`.)
                continue;
            }
            let prefix = hash.0[..k as usize].to_vec();
            self.indexes.get_mut(&k).unwrap()
                .entry(prefix).or_default().push(hash.clone());
        }
        self.all.push(hash);
    }

    /// Look up every hash whose first `prefix.len()` bytes match `prefix`.
    /// The prefix length MUST equal one of the cadences this bibliography
    /// was constructed with.
    pub fn lookup(&self, prefix: &[u8]) -> Vec<Hash> {
        let k = prefix.len() as u8;
        let bucket = self.indexes.get(&k).unwrap_or_else(|| panic!(
            "lookup prefix length {} is not one of the supported cadences {:?}",
            k, self.indexes.keys().collect::<Vec<_>>(),
        ));
        bucket.get(prefix).cloned().unwrap_or_default()
    }

    /// Total number of hashes in the library.
    pub fn len(&self) -> usize { self.all.len() }
    pub fn is_empty(&self) -> bool { self.all.is_empty() }
}

/// Encode an ordered sequence of hashes as a palimpsest at the given
/// initial and regular cadences. All hashes must share the same byte length,
/// and that length must be one of the §2.1 sizes.
///
/// Panics if `hashes` is empty, the cadences are out of range, or the hashes
/// are not all the same supported length.
pub fn encode(hashes: &[Hash], k_i: u8, k_r: u8) -> Palimpsest {
    assert!(!hashes.is_empty(), "cannot encode an empty hash sequence");
    assert!((1..=4).contains(&k_r), "k_r must be in 1..=4 (got {})", k_r);
    assert!(k_i >= k_r && k_i <= k_r + 3,
            "k_i must be in {}..={} (got {})", k_r, k_r + 3, k_i);
    let h_len = hashes[0].len();
    assert!(hashes.iter().all(|h| h.len() == h_len),
            "all hashes must share the same byte length");

    let n = hashes.len();
    let body = body_len(n, h_len, k_i, k_r);
    let mut bytes = vec![0u8; body];
    for (i, hash) in hashes.iter().enumerate() {
        let off = hash_offset(i, k_i, k_r);
        for j in 0..h_len {
            bytes[off + j] ^= hash.0[j];
        }
    }
    let c = pack_cadence(k_r, k_i, h_len);
    let body_xor = bytes.iter().fold(0u8, |acc, &b| acc ^ b);
    bytes.push(body_xor ^ c);
    Palimpsest { bytes }
}

/// Decode a palimpsest against a hash library, returning the original sequence
/// of hashes if exactly one valid reconstruction exists.
///
/// Returns `None` on any framing error (truncation, reserved hash-size index,
/// inconsistent byte length, missing hashes, or no valid reconstruction).
pub fn decode(palimpsest: &Palimpsest, bibliography: &Bibliography) -> Option<Vec<Hash>> {
    let (k_r, k_i, h_len) = palimpsest.parameters()?;
    let n = palimpsest.hash_count()?;
    let body = palimpsest.bytes.len() - 1;

    let mut data = palimpsest.bytes[..body].to_vec();
    let mut result: Vec<Hash> = Vec::with_capacity(n);

    fn search(
        data: &mut Vec<u8>,
        bib: &Bibliography,
        k_i: u8,
        k_r: u8,
        h_len: usize,
        i: usize,
        n: usize,
        result: &mut Vec<Hash>,
    ) -> bool {
        if i == n {
            return data.iter().all(|&b| b == 0);
        }
        let off = hash_offset(i, k_i, k_r);
        let p_len = if i == 0 { k_i } else { k_r } as usize;
        let prefix = data[off..off + p_len].to_vec();
        for h in bib.lookup(&prefix) {
            if h.len() != h_len { continue; }
            for j in 0..h_len { data[off + j] ^= h.0[j]; }
            result.push(h.clone());
            if search(data, bib, k_i, k_r, h_len, i + 1, n, result) {
                return true;
            }
            result.pop();
            for j in 0..h_len { data[off + j] ^= h.0[j]; }
        }
        false
    }

    if search(&mut data, bibliography, k_i, k_r, h_len, 0, n, &mut result) {
        Some(result)
    } else {
        None
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic 32-byte test hashes derived from a single seed byte.
    /// Pattern designed so every byte differs across hashes (avoids accidental
    /// prefix collisions at low cadence).
    fn h(seed: u8) -> Hash {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (seed as u16 * 257 + i as u16 * 31) as u8;
        }
        Hash::from(bytes)
    }

    #[test]
    fn cadence_byte_pack_unpack_round_trip() {
        for &h_len in &[8, 10, 12, 16, 20, 24, 28, 32, 48, 64] {
            for k_r in 1u8..=4 {
                for diff in 0u8..=3 {
                    let k_i = k_r + diff;
                    let c = pack_cadence(k_r, k_i, h_len);
                    let (uk_r, uk_i, uh) = unpack_cadence(c).expect("must unpack");
                    assert_eq!((k_r, k_i, h_len), (uk_r, uk_i, uh),
                               "round-trip failed for k_r={} k_i={} h_len={}",
                               k_r, k_i, h_len);
                }
            }
        }
    }

    #[test]
    fn cadence_byte_reserved_indices_are_rejected() {
        for s in 10u8..=15 {
            let c = (s << 4) | 0b00_01;  // k_r=2, k_i-k_r=0
            assert!(unpack_cadence(c).is_none(),
                    "reserved s={} should not unpack", s);
        }
    }

    #[test]
    fn bintel_cadence_byte_is_0x79() {
        // BinTEL fixes (s, k_i-k_r, k_r-1) = (7, 2, 1).
        // bits: 0111_10_01 = 0x79.
        assert_eq!(pack_cadence(2, 4, 32), 0x79);
    }

    #[test]
    fn single_hash_palimpsest_length() {
        let p = encode(&[h(7)], 4, 2);
        // L = H + 1 = 33
        assert_eq!(p.len(), 33);
        assert_eq!(p.parameters(), Some((2, 4, 32)));
        assert_eq!(p.hash_count(), Some(1));
    }

    #[test]
    fn n_two_palimpsest_length_bintel_params() {
        let p = encode(&[h(0), h(1)], 4, 2);
        // L = H + k_i + 1 = 32 + 4 + 1 = 37
        assert_eq!(p.len(), 37);
        assert_eq!(p.hash_count(), Some(2));
    }

    #[test]
    fn length_formula_bintel_params() {
        for n in 1..=8 {
            let hashes: Vec<Hash> = (0..n as u8).map(h).collect();
            let p = encode(&hashes, 4, 2);
            let expected = if n == 1 { 33 } else { 32 + 4 + 2 * (n - 2) + 1 };
            assert_eq!(p.len(), expected, "n={} expected {} got {}", n, expected, p.len());
        }
    }

    #[test]
    fn length_formula_user_example_ki4_kr3() {
        // User's worked example: k_i=4, k_r=3, offsets 0, 4, 7, 10, 13.
        let hashes: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&hashes, 4, 3);
        // Last hash at offset 4 + 3*3 = 13, ends at 13+32 = 45.
        // Body length = 45, total = 46.
        assert_eq!(p.len(), 46);
    }

    #[test]
    fn trailing_byte_xor_property() {
        // §3.1 step 5: XOR of every byte must equal the cadence byte.
        for (k_i, k_r) in [(1, 1), (2, 2), (3, 3), (4, 4), (4, 2), (3, 1), (4, 1)] {
            let hashes: Vec<Hash> = (0..6).map(h).collect();
            let p = encode(&hashes, k_i, k_r);
            let xor: u8 = p.bytes().iter().fold(0u8, |a, &b| a ^ b);
            assert_eq!(xor, pack_cadence(k_r, k_i, 32),
                       "XOR property failed for k_i={} k_r={}", k_i, k_r);
        }
    }

    #[test]
    fn round_trip_bintel_params() {
        let hashes: Vec<Hash> = (0..10).map(h).collect();
        let p = encode(&hashes, 4, 2);
        let mut bib = Bibliography::for_cadences(4, 2);
        for hash in &hashes { bib.add(hash.clone()); }
        assert_eq!(decode(&p, &bib).as_deref(), Some(hashes.as_slice()));
    }

    #[test]
    fn round_trip_cadence_1_1() {
        let hashes: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&hashes, 1, 1);
        let mut bib = Bibliography::new(&[1]);
        for hash in &hashes { bib.add(hash.clone()); }
        assert_eq!(decode(&p, &bib).as_deref(), Some(hashes.as_slice()));
    }

    #[test]
    fn round_trip_cadence_3_3() {
        let hashes: Vec<Hash> = (0..20).map(h).collect();
        let p = encode(&hashes, 3, 3);
        let mut bib = Bibliography::new(&[3]);
        for hash in &hashes { bib.add(hash.clone()); }
        assert_eq!(decode(&p, &bib).as_deref(), Some(hashes.as_slice()));
    }

    #[test]
    fn round_trip_single_hash() {
        // n=1: palimpsest is the hash bytes followed by the cadence byte.
        let hash = h(42);
        let p = encode(&[hash.clone()], 4, 2);
        let mut bib = Bibliography::for_cadences(4, 2);
        bib.add(hash.clone());
        assert_eq!(decode(&p, &bib).as_deref(), Some(&[hash][..]));
    }

    #[test]
    fn round_trip_with_extra_library_hashes() {
        let used: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&used, 4, 2);
        let mut bib = Bibliography::for_cadences(4, 2);
        for hash in (0..50).map(h) { bib.add(hash); }
        assert_eq!(decode(&p, &bib).as_deref(), Some(used.as_slice()));
    }

    #[test]
    fn decode_fails_when_hash_missing_from_library() {
        let hashes: Vec<Hash> = (0..3).map(h).collect();
        let p = encode(&hashes, 4, 2);
        let mut bib = Bibliography::for_cadences(4, 2);
        bib.add(hashes[0].clone());
        bib.add(hashes[1].clone());
        // Third hash missing
        assert_eq!(decode(&p, &bib), None);
    }

    #[test]
    fn decode_fails_on_truncated_palimpsest() {
        let hashes: Vec<Hash> = (0..3).map(h).collect();
        let mut p = encode(&hashes, 4, 2);
        p.bytes.truncate(p.bytes.len() - 1);  // drop trailing byte
        let mut bib = Bibliography::for_cadences(4, 2);
        for hash in &hashes { bib.add(hash.clone()); }
        assert_eq!(decode(&p, &bib), None);
    }

    #[test]
    fn empty_palimpsest_does_not_round_trip() {
        let bib = Bibliography::new(&[1]);
        let empty = Palimpsest { bytes: vec![] };
        assert_eq!(decode(&empty, &bib), None);
    }

    #[test]
    #[should_panic(expected = "cannot encode an empty hash sequence")]
    fn encode_panics_on_empty() {
        let _ = encode(&[], 2, 2);
    }

    #[test]
    #[should_panic(expected = "k_r must be in 1..=4")]
    fn encode_panics_on_invalid_k_r() {
        let _ = encode(&[h(0)], 5, 5);
    }

    #[test]
    #[should_panic(expected = "k_i must be in")]
    fn encode_panics_on_k_i_below_k_r() {
        let _ = encode(&[h(0)], 1, 2);  // k_i=1 < k_r=2
    }

    #[test]
    #[should_panic(expected = "k_i must be in")]
    fn encode_panics_on_k_i_too_large() {
        let _ = encode(&[h(0)], 8, 2);  // k_i-k_r = 6, max is 3
    }

    #[test]
    fn bibliography_lookup_returns_all_matching() {
        // Two hashes sharing the same first byte.
        let mut bib = Bibliography::new(&[1]);
        let h1 = Hash::new(vec![0xAB, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                                16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]);
        let h2 = Hash::new(vec![0xAB, 99, 88, 77, 66, 55, 44, 33, 22, 11, 0, 0, 0, 0, 0, 0,
                                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bib.add(h1.clone());
        bib.add(h2.clone());
        let found = bib.lookup(&[0xAB]);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&h1));
        assert!(found.contains(&h2));
    }

    #[test]
    fn bibliography_lookup_returns_empty_for_missing_prefix() {
        let mut bib = Bibliography::new(&[2]);
        bib.add(h(0));
        assert!(bib.lookup(&[0xFF, 0xFF]).is_empty());
    }

    #[test]
    fn parameters_round_trip_via_byte_xor() {
        let hashes: Vec<Hash> = (0..4).map(h).collect();
        let p = encode(&hashes, 4, 2);
        // The bytes round-trip through Palimpsest::from_bytes — the
        // parameters are recoverable from the bytes alone (no side channel).
        let q = Palimpsest::from_bytes(p.bytes().to_vec());
        assert_eq!(q.parameters(), Some((2, 4, 32)));
        assert_eq!(q.hash_count(), Some(4));
    }

    #[test]
    fn smaller_hash_size_round_trip() {
        // 16-byte (s=3) hashes.
        let mk = |seed: u8| -> Hash {
            let mut b = vec![0u8; 16];
            for (i, x) in b.iter_mut().enumerate() {
                *x = (seed as u16 * 257 + i as u16 * 31) as u8;
            }
            Hash::new(b)
        };
        let hashes: Vec<Hash> = (0..6).map(mk).collect();
        let p = encode(&hashes, 3, 2);
        assert_eq!(p.parameters(), Some((2, 3, 16)));
        let mut bib = Bibliography::for_cadences(3, 2);
        for hash in &hashes { bib.add(hash.clone()); }
        assert_eq!(decode(&p, &bib).as_deref(), Some(hashes.as_slice()));
    }
}
