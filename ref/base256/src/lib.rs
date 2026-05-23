//! BASE-256 binary-to-text encoding (see `spec/base256.md`).
//!
//! Encodes each byte as one Unicode character drawn from a 256-character
//! alphabet whose defining property is `codepoint(A[b]) ≡ b (mod 256)`.
//! Decoding therefore requires no lookup table: each input character's
//! original byte value is its code point modulo 256.

use std::sync::LazyLock;

/// The 256-character BASE-256 alphabet, in byte-value order. The character
/// at index `b` is the encoding of the byte value `b`.
pub const ALPHABET: &str = "ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠḡḢḣḤĥȦȧШḩЪЫЬЭĮį0123456789ĺĻļĽľĿŀABCDEFGHIJKLMNOPQRSTUVWXYZṛќѝŞşŠabcdefghijklmnopqrstuvwxyzŻżṽžſẀẁẂẃẄẅẆẇẈẉΊẋẌẍΎƏҐґƒẓΔƕƖẗẘẙҚқƜƝΞƟƠơҢңƤƥΦƧƨΩΪΫάέήίưᾱβγδεζҷᾸικλμẽξοπӁӂÃτÅÆÇψωϊϋỌύώϏÐǑǒǓÔϕӖϗῘÙῚӛӜӝÞӟàῡǢǣӤåæçǨῩӪӫìíӮӯðñỲỳôỵǶỷӸùῺûǼǽþǿ";

/// Indexed array view of the alphabet for O(1) encoding lookup.
static ALPHABET_ARR: LazyLock<[char; 256]> = LazyLock::new(|| {
    let mut arr = ['\0'; 256];
    for (i, c) in ALPHABET.chars().enumerate() {
        assert!(i < 256, "alphabet has more than 256 characters");
        arr[i] = c;
    }
    arr
});

/// Membership set of alphabet characters, for strict-mode decoding.
static ALPHABET_SET: LazyLock<std::collections::HashSet<char>> = LazyLock::new(|| {
    ALPHABET.chars().collect()
});

/// Encode a byte sequence to its BASE-256 textual form.
///
/// The result has one Unicode character per input byte. When serialised to
/// UTF-8 it is approximately 1.5× the input length, since most alphabet
/// characters take 2–3 UTF-8 bytes.
pub fn encode(data: &[u8]) -> String {
    let arr = &*ALPHABET_ARR;
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(arr[b as usize]);
    }
    s
}

/// Decode a BASE-256 textual form to bytes, permissively. Every input
/// character is decoded as `codepoint(c) mod 256`; characters outside the
/// alphabet are accepted and their residue is taken without error.
pub fn decode(text: &str) -> Vec<u8> {
    text.chars().map(|c| (c as u32 % 256) as u8).collect()
}

/// A character outside the BASE-256 alphabet encountered during strict-mode
/// decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    /// Zero-based code-point index in the input where the offending character
    /// appears.
    pub position: usize,
    /// The character that's not in the alphabet.
    pub character: char,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "character `{}` (U+{:04X}) at position {} is not in the BASE-256 alphabet",
               self.character, self.character as u32, self.position)
    }
}

/// Decode a BASE-256 textual form to bytes, strictly: every input character
/// MUST be in the alphabet. Returns the decoded bytes if all characters are
/// valid, otherwise returns the list of every offending character with its
/// position.
pub fn decode_strict(text: &str) -> Result<Vec<u8>, Vec<DecodeError>> {
    let set = &*ALPHABET_SET;
    let mut bytes = Vec::with_capacity(text.len());
    let mut errors = Vec::new();
    for (pos, c) in text.chars().enumerate() {
        if !set.contains(&c) {
            errors.push(DecodeError { position: pos, character: c });
        } else {
            bytes.push((c as u32 % 256) as u8);
        }
    }
    if errors.is_empty() { Ok(bytes) } else { Err(errors) }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The defining property: `codepoint(A[b]) ≡ b (mod 256)` for every b.
    #[test]
    fn alphabet_round_trip_property() {
        for (i, c) in ALPHABET.chars().enumerate() {
            assert_eq!((c as u32 % 256) as usize, i,
                "alphabet[{}] = U+{:04X} (`{}`), codepoint mod 256 = {} (expected {})",
                i, c as u32, c, c as u32 % 256, i);
        }
    }

    /// The alphabet MUST contain exactly 256 distinct characters.
    #[test]
    fn alphabet_has_256_distinct_characters() {
        let chars: Vec<char> = ALPHABET.chars().collect();
        assert_eq!(chars.len(), 256, "alphabet has {} characters, expected 256", chars.len());
        let unique: std::collections::HashSet<char> = chars.iter().copied().collect();
        assert_eq!(unique.len(), 256, "alphabet contains duplicates");
    }

    /// Every alphabet character MUST be a Unicode Letter or Decimal Digit
    /// (so that double-click word selection per Unicode Annex #29 picks up
    /// the entire encoded string as one word — letters via WB5 and digits
    /// via WB8, joined across boundaries by WB9/WB10).
    #[test]
    fn alphabet_is_letters_or_digits() {
        for (i, c) in ALPHABET.chars().enumerate() {
            assert!(c.is_alphabetic() || c.is_ascii_digit(),
                "alphabet[{}] = U+{:04X} (`{}`) is neither a Unicode letter nor a digit",
                i, c as u32, c);
        }
    }

    /// ASCII positions in the alphabet MUST be the corresponding ASCII
    /// characters themselves (digits, uppercase, lowercase).
    #[test]
    fn ascii_positions_are_self_encoded() {
        let arr = &*ALPHABET_ARR;
        for b in b'0'..=b'9' { assert_eq!(arr[b as usize], b as char); }
        for b in b'A'..=b'Z' { assert_eq!(arr[b as usize], b as char); }
        for b in b'a'..=b'z' { assert_eq!(arr[b as usize], b as char); }
    }

    #[test]
    fn round_trip_all_bytes() {
        let data: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&data);
        assert_eq!(encoded.chars().count(), 256, "encoded length should equal input length");
        let decoded = decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn round_trip_random_bytes() {
        // A deterministic "random" sample (Knuth's multiplicative hash).
        let data: Vec<u8> = (0..1000u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 16) as u8)
            .collect();
        assert_eq!(decode(&encode(&data)), data);
    }

    #[test]
    fn decode_strict_accepts_alphabet() {
        let data: Vec<u8> = (0..=255u8).collect();
        let encoded = encode(&data);
        assert_eq!(decode_strict(&encoded), Ok(data));
    }

    #[test]
    fn decode_strict_rejects_non_alphabet_chars() {
        // Build by-character to avoid byte/char-boundary issues with the
        // multi-byte UTF-8 of encoded chars.
        let encoded = encode(&[1, 2, 3]);
        let mut chars: Vec<char> = encoded.chars().collect();
        chars.insert(1, ' ');                  // U+0020 — not in the alphabet
        chars.push('Π');                       // U+03A0 — letter, but not in this alphabet
        let s: String = chars.into_iter().collect();
        match decode_strict(&s) {
            Err(errs) => {
                assert!(errs.iter().any(|e| e.character == ' '),
                        "expected ' ' in errors, got: {:?}", errs);
                assert!(errs.iter().any(|e| e.character == 'Π'),
                        "expected 'Π' in errors, got: {:?}", errs);
            }
            Ok(b) => panic!("expected errors, got bytes: {:?}", b),
        }
    }

    #[test]
    fn permissive_decode_still_yields_residue_for_unknown_chars() {
        // 'A' has codepoint 0x41 = 65. ' ' (space) has codepoint 0x20 = 32.
        // Both should decode to their codepoint mod 256.
        assert_eq!(decode("A "), vec![65, 32]);
    }

    #[test]
    fn encode_empty_yields_empty() {
        assert_eq!(encode(&[]), "");
        assert_eq!(decode(""), Vec::<u8>::new());
    }

    #[test]
    fn single_byte_encoding() {
        assert_eq!(encode(&[0x41]), "A");           // ASCII passes through
        assert_eq!(encode(&[0x30]), "0");
        assert_eq!(decode("A"), vec![0x41]);
        assert_eq!(decode("0"), vec![0x30]);
    }
}
