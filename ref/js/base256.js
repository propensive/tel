// BASE-256 binary-to-text encoding (see spec/base256.md).
//
// Each byte of input is mapped to a single Unicode character drawn from a
// fixed 256-character alphabet whose defining property is
//
//     codepoint(A[b]) ≡ b (mod 256)
//
// Decoding requires no lookup table: every input character's original byte
// is its code point modulo 256 (§6 of the BASE-256 Specification).

export const ALPHABET =
  "ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠḡḢḣḤĥȦȧШḩЪЫЬЭĮį0123456789ĺĻļĽľĿŀ" +
  "ABCDEFGHIJKLMNOPQRSTUVWXYZṛќѝŞşŠabcdefghijklmnopqrstuvwxyzŻżṽžſ" +
  "ẀẁẂẃẄẅẆẇẈẉΊẋẌẍΎƏҐґƒẓΔƕƖẗẘẙҚқƜƝΞƟƠơҢңƤƥΦƧƨΩΪΫάέήίưᾱβγδεζҷᾸικλμẽξοπ" +
  "ӁӂÃτÅÆÇψωϊϋỌύώϏÐǑǒǓÔϕӖϗῘÙῚӛӜӝÞӟàῡǢǣӤåæçǨῩӪӫìíӮӯðñỲỳôỵǶỷӸùῺûǼǽþǿ";

const ALPHABET_ARR = Array.from(ALPHABET);
if (ALPHABET_ARR.length !== 256) {
  throw new Error(`BASE-256 alphabet has ${ALPHABET_ARR.length} characters, expected 256`);
}

const ALPHABET_SET = new Set(ALPHABET_ARR);

// Encode bytes (Uint8Array or array of integers in [0, 255]) to BASE-256 text.
// Returns a string of input.length Unicode characters.
export function encode(data) {
  const bytes = data instanceof Uint8Array ? data : Uint8Array.from(data);
  let out = "";
  for (let i = 0; i < bytes.length; i++) {
    out += ALPHABET_ARR[bytes[i]];
  }
  return out;
}

// Decode BASE-256 text to a Uint8Array, permissively. Every character is
// decoded as `codepoint(c) % 256`; characters outside the alphabet are
// accepted and their residue is taken without error (§9 permissive mode).
export function decode(text) {
  const chars = Array.from(text); // splits into Unicode scalar values, not UTF-16 code units
  const out = new Uint8Array(chars.length);
  for (let i = 0; i < chars.length; i++) {
    out[i] = chars[i].codePointAt(0) % 256;
  }
  return out;
}

// Strict decode: every character MUST be a member of the alphabet of §4.
// Returns the decoded Uint8Array on success; throws a DecodeError listing
// the position and offending character on the first non-alphabet input.
//
// To collect every offending character (matching the Rust API), call
// `decodeStrictAll` instead, which returns either `{ ok: true, bytes }` or
// `{ ok: false, errors }`.
export function decodeStrict(text) {
  const result = decodeStrictAll(text);
  if (result.ok) return result.bytes;
  throw new DecodeError(result.errors);
}

// As decodeStrict, but returns a tagged result rather than throwing,
// collecting *every* offending character with its zero-based code-point
// position in the input.
export function decodeStrictAll(text) {
  const chars = Array.from(text);
  const errors = [];
  const bytes = new Uint8Array(chars.length);
  let writeIdx = 0;
  for (let i = 0; i < chars.length; i++) {
    if (!ALPHABET_SET.has(chars[i])) {
      errors.push({ position: i, character: chars[i] });
    } else {
      bytes[writeIdx++] = chars[i].codePointAt(0) % 256;
    }
  }
  if (errors.length === 0) return { ok: true, bytes };
  return { ok: false, errors };
}

export class DecodeError extends Error {
  constructor(errors) {
    const first = errors[0];
    const cp = first.character.codePointAt(0);
    super(
      `BASE-256 strict decode: character '${first.character}' (U+${cp.toString(16).toUpperCase().padStart(4, "0")}) ` +
      `at position ${first.position} is not in the alphabet (${errors.length} total error(s))`,
    );
    this.name = "Base256DecodeError";
    this.errors = errors;
  }
}
