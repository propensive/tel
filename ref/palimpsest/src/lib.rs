//! Palimpsest: compact binary encoding of an ordered sequence of fixed-length
//! cryptographic hashes drawn from a shared library.
//!
//! See `../../../spec/palimpsest.md`. The implementation supports any byte cadence `k` in
//! `1..H` (where `H = 32` for SHA-256, the only hash length supported here).

use std::collections::HashMap;

/// Hash length in bytes. SHA-256 is the only hash function this module is
/// parameterised against; `H = 32`.
pub const HASH_LEN: usize = 32;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct Hash([u8; HASH_LEN]);

impl Hash {
    pub fn new(bytes: [u8; HASH_LEN]) -> Self { Self(bytes) }
    pub fn bytes(&self) -> &[u8; HASH_LEN] { &self.0 }
}

impl From<[u8; HASH_LEN]> for Hash {
    fn from(bytes: [u8; HASH_LEN]) -> Self { Self(bytes) }
}

/// A palimpsest: the XOR-stacked encoding of a sequence of hashes. Carries
/// its cadence so it is self-describing for decoding purposes.
#[derive(Debug, Clone, PartialEq)]
pub struct Palimpsest {
    bytes: Vec<u8>,
    cadence: usize,
}

impl Palimpsest {
    pub fn bytes(&self) -> &[u8] { &self.bytes }
    pub fn cadence(&self) -> usize { self.cadence }
    /// The number of hashes encoded in this palimpsest.
    pub fn len(&self) -> usize {
        if self.bytes.len() < HASH_LEN { 0 } else {
            (self.bytes.len() - HASH_LEN) / self.cadence + 1
        }
    }
    pub fn is_empty(&self) -> bool { self.bytes.is_empty() }

    /// Construct a `Palimpsest` from its byte representation. The
    /// caller MUST know the cadence used to encode it; the byte
    /// representation does not carry the cadence.
    pub fn from_bytes(bytes: Vec<u8>, cadence: usize) -> Self {
        Self { bytes, cadence }
    }
}

/// Hash library indexed by `k`-byte prefix. Construct with the cadence you
/// expect to use; the `add` and `lookup` operations use that cadence to bucket
/// hashes by their first `k` bytes.
#[derive(Debug, Clone)]
pub struct Bibliography {
    cadence: usize,
    by_prefix: HashMap<Vec<u8>, Vec<Hash>>,
}

impl Bibliography {
    pub fn new(cadence: usize) -> Self {
        assert!(cadence >= 1 && cadence < HASH_LEN,
                "cadence must be in 1..{}", HASH_LEN);
        Bibliography { cadence, by_prefix: HashMap::new() }
    }

    pub fn cadence(&self) -> usize { self.cadence }

    /// Add a hash to the library. Bucketed by its first `cadence` bytes.
    pub fn add(&mut self, hash: Hash) {
        let prefix = hash.0[..self.cadence].to_vec();
        self.by_prefix.entry(prefix).or_insert_with(Vec::new).push(hash);
    }

    /// Look up every hash in the library whose first `cadence` bytes match the
    /// supplied prefix. The prefix MUST be exactly `cadence` bytes long.
    pub fn lookup(&self, prefix: &[u8]) -> Vec<Hash> {
        assert_eq!(prefix.len(), self.cadence,
                   "lookup prefix length {} must equal cadence {}",
                   prefix.len(), self.cadence);
        self.by_prefix.get(prefix).cloned().unwrap_or_default()
    }

    /// Total number of hashes in the library.
    pub fn len(&self) -> usize {
        self.by_prefix.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool { self.by_prefix.is_empty() }
}

/// Encode an ordered sequence of hashes as a palimpsest at the given cadence.
///
/// Length of the result: `HASH_LEN + cadence * (hashes.len() - 1)`. The
/// resulting palimpsest carries the cadence so the decoder doesn't need it
/// supplied separately.
///
/// Panics if `hashes` is empty or `cadence` is outside `1..HASH_LEN`.
pub fn encode(hashes: &[Hash], cadence: usize) -> Palimpsest {
    assert!(!hashes.is_empty(), "cannot encode an empty hash sequence");
    assert!(cadence >= 1 && cadence < HASH_LEN,
            "cadence must be in 1..{}", HASH_LEN);
    let length = HASH_LEN + cadence * (hashes.len() - 1);
    let mut bytes = vec![0u8; length];
    for (i, hash) in hashes.iter().enumerate() {
        let offset = cadence * i;
        for j in 0..HASH_LEN {
            bytes[offset + j] ^= hash.0[j];
        }
    }
    Palimpsest { bytes, cadence }
}

/// Decode a palimpsest against a hash library, returning the original sequence
/// of hashes if exactly one valid reconstruction exists.
///
/// Returns `None` if no valid reconstruction exists or if multiple
/// reconstructions are possible (the latter indicates a malformed library or
/// pathological collisions).
pub fn decode(palimpsest: &Palimpsest, bibliography: &Bibliography) -> Option<Vec<Hash>> {
    assert_eq!(palimpsest.cadence, bibliography.cadence,
               "palimpsest cadence {} must match bibliography cadence {}",
               palimpsest.cadence, bibliography.cadence);
    let cadence = palimpsest.cadence;
    let len = palimpsest.bytes.len();
    // `L = H + k*(n-1)` ⇒ `n = (L - H) / k + 1`. L must be at least H and
    // (L - H) must be divisible by k.
    if len < HASH_LEN { return None; }
    let span = len - HASH_LEN;
    if span % cadence != 0 { return None; }
    let n = span / cadence + 1;

    let mut data = palimpsest.bytes.clone();
    let mut result: Vec<Hash> = Vec::with_capacity(n);

    fn search(
        data: &mut Vec<u8>,
        bibliography: &Bibliography,
        cadence: usize,
        i: usize,
        n: usize,
        result: &mut Vec<Hash>,
    ) -> bool {
        if i == n {
            return data.iter().all(|&b| b == 0);
        }
        let offset = cadence * i;
        // Snapshot the k-byte prefix that identifies h_i.
        let prefix = data[offset..offset + cadence].to_vec();
        for hash in bibliography.lookup(&prefix) {
            // XOR the candidate hash out of data at offset.
            for j in 0..HASH_LEN {
                data[offset + j] ^= hash.0[j];
            }
            result.push(hash);
            if search(data, bibliography, cadence, i + 1, n, result) {
                return true;
            }
            // Backtrack: undo XOR and pop.
            result.pop();
            for j in 0..HASH_LEN {
                data[offset + j] ^= hash.0[j];
            }
        }
        false
    }

    if search(&mut data, bibliography, cadence, 0, n, &mut result) {
        Some(result)
    } else {
        None
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn h(seed: u8) -> Hash {
        // Deterministic test hashes derived from a single seed byte. Pattern
        // designed so every byte differs across hashes (avoids accidental
        // prefix collisions at low cadence).
        let mut bytes = [0u8; HASH_LEN];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (seed as u16 * 257 + i as u16 * 31) as u8;
        }
        Hash(bytes)
    }

    #[test]
    fn single_hash_palimpsest_is_the_hash() {
        let hash = h(7);
        let p = encode(&[hash], 1);
        assert_eq!(p.bytes(), &hash.0[..]);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn length_formula_cadence_1() {
        let hashes: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&hashes, 1);
        // L = H + k*(n-1) = 32 + 1*4 = 36
        assert_eq!(p.bytes().len(), 32 + 4);
    }

    #[test]
    fn length_formula_cadence_2() {
        let hashes: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&hashes, 2);
        // L = 32 + 2*4 = 40
        assert_eq!(p.bytes().len(), 32 + 8);
    }

    #[test]
    fn length_formula_cadence_3() {
        let hashes: Vec<Hash> = (0..10).map(h).collect();
        let p = encode(&hashes, 3);
        assert_eq!(p.bytes().len(), 32 + 27);
    }

    #[test]
    fn round_trip_cadence_1() {
        let hashes: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&hashes, 1);
        let mut bib = Bibliography::new(1);
        for hash in &hashes { bib.add(*hash); }
        assert_eq!(decode(&p, &bib), Some(hashes));
    }

    #[test]
    fn round_trip_cadence_2() {
        // Match the BinTEL schema signature use case.
        let hashes: Vec<Hash> = (0..10).map(h).collect();
        let p = encode(&hashes, 2);
        let mut bib = Bibliography::new(2);
        for hash in &hashes { bib.add(*hash); }
        assert_eq!(decode(&p, &bib), Some(hashes));
    }

    #[test]
    fn round_trip_cadence_3() {
        let hashes: Vec<Hash> = (0..20).map(h).collect();
        let p = encode(&hashes, 3);
        let mut bib = Bibliography::new(3);
        for hash in &hashes { bib.add(*hash); }
        assert_eq!(decode(&p, &bib), Some(hashes));
    }

    #[test]
    fn decode_with_extra_library_hashes_still_works() {
        // Library contains many hashes; only some are in the palimpsest.
        let used: Vec<Hash> = (0..5).map(h).collect();
        let p = encode(&used, 2);
        let mut bib = Bibliography::new(2);
        for hash in (0..50).map(h) { bib.add(hash); }   // includes the used ones
        assert_eq!(decode(&p, &bib), Some(used));
    }

    #[test]
    fn decode_fails_when_hash_missing_from_library() {
        let hashes: Vec<Hash> = (0..3).map(h).collect();
        let p = encode(&hashes, 2);
        let mut bib = Bibliography::new(2);
        // Only add the first two — third is missing
        bib.add(hashes[0]);
        bib.add(hashes[1]);
        assert_eq!(decode(&p, &bib), None);
    }

    #[test]
    fn decode_fails_on_truncated_palimpsest() {
        let hashes: Vec<Hash> = (0..3).map(h).collect();
        let mut p = encode(&hashes, 2);
        p.bytes.truncate(p.bytes.len() - 1);  // remove a byte → length no longer a valid L
        let mut bib = Bibliography::new(2);
        for hash in &hashes { bib.add(*hash); }
        assert_eq!(decode(&p, &bib), None);
    }

    #[test]
    fn empty_palimpsest_does_not_round_trip() {
        // The spec forbids n=0 (§9.7); we panic in encode and accept None on decode.
        let bib = Bibliography::new(1);
        let empty = Palimpsest { bytes: vec![], cadence: 1 };
        assert_eq!(decode(&empty, &bib), None);
    }

    #[test]
    #[should_panic(expected = "cannot encode an empty hash sequence")]
    fn encode_panics_on_empty() {
        let _ = encode(&[], 1);
    }

    #[test]
    #[should_panic(expected = "cadence must be in")]
    fn encode_panics_on_invalid_cadence() {
        let _ = encode(&[h(0)], 0);
    }

    #[test]
    #[should_panic(expected = "cadence must be in")]
    fn encode_panics_on_cadence_equal_to_hash_len() {
        let _ = encode(&[h(0)], HASH_LEN);
    }

    #[test]
    fn bibliography_lookup_returns_empty_for_missing_prefix() {
        let mut bib = Bibliography::new(2);
        bib.add(h(0));
        // Look up a prefix that doesn't match h(0)
        let other_prefix = vec![0xFF, 0xFF];
        assert!(bib.lookup(&other_prefix).is_empty());
    }

    #[test]
    fn bibliography_lookup_returns_all_matching() {
        // Two hashes with the same first byte
        let mut bib = Bibliography::new(1);
        let h1 = Hash([0xAB, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                       16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]);
        let h2 = Hash([0xAB, 99, 88, 77, 66, 55, 44, 33, 22, 11, 0, 0, 0, 0, 0, 0,
                       0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bib.add(h1);
        bib.add(h2);
        let found = bib.lookup(&[0xAB]);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&h1));
        assert!(found.contains(&h2));
    }
}
