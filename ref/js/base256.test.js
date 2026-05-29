import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ALPHABET, encode, decode, decodeStrict, decodeStrictAll, DecodeError,
} from "./base256.js";

test("alphabet has 256 distinct characters", () => {
  const chars = Array.from(ALPHABET);
  assert.equal(chars.length, 256);
  assert.equal(new Set(chars).size, 256, "alphabet contains duplicates");
});

test("alphabet defining property: codepoint(A[b]) ≡ b (mod 256) for every b", () => {
  const chars = Array.from(ALPHABET);
  for (let b = 0; b < 256; b++) {
    const cp = chars[b].codePointAt(0);
    assert.equal(cp % 256, b,
      `alphabet[${b}] = U+${cp.toString(16).padStart(4, "0")} ('${chars[b]}'), residue ${cp % 256}`);
  }
});

test("ASCII positions are self-encoded", () => {
  const chars = Array.from(ALPHABET);
  for (let b = 0x30; b <= 0x39; b++) assert.equal(chars[b], String.fromCharCode(b));
  for (let b = 0x41; b <= 0x5A; b++) assert.equal(chars[b], String.fromCharCode(b));
  for (let b = 0x61; b <= 0x7A; b++) assert.equal(chars[b], String.fromCharCode(b));
});

test("round-trip every byte value 0..255", () => {
  const data = new Uint8Array(256);
  for (let i = 0; i < 256; i++) data[i] = i;
  const encoded = encode(data);
  assert.equal(Array.from(encoded).length, 256, "encoded length in chars == input length in bytes");
  const decoded = decode(encoded);
  assert.deepEqual(decoded, data);
});

test("empty round-trip", () => {
  assert.equal(encode(new Uint8Array(0)), "");
  assert.deepEqual(decode(""), new Uint8Array(0));
});

test("permissive decode yields residue for non-alphabet chars", () => {
  // 'A' = 0x41 = 65; ' ' (space) = 0x20 = 32 — both reduce by mod 256 to themselves.
  assert.deepEqual(decode("A "), Uint8Array.from([65, 32]));
});

test("strict decode accepts the entire alphabet", () => {
  const data = new Uint8Array(256);
  for (let i = 0; i < 256; i++) data[i] = i;
  const encoded = encode(data);
  assert.deepEqual(decodeStrict(encoded), data);
});

test("strict decode rejects non-alphabet characters and reports every offence", () => {
  // Build by character to avoid byte/char-boundary pitfalls of multi-byte UTF-8.
  const encoded = encode(Uint8Array.from([1, 2, 3]));
  const chars = Array.from(encoded);
  chars.splice(1, 0, " ");      // U+0020 — not in the alphabet
  chars.push("Π");              // U+03A0 — letter, but not in this alphabet
  const s = chars.join("");
  const result = decodeStrictAll(s);
  assert.equal(result.ok, false);
  assert.ok(result.errors.some(e => e.character === " "));
  assert.ok(result.errors.some(e => e.character === "Π"));
  assert.throws(() => decodeStrict(s), DecodeError);
});

test("single-byte encoding maps ASCII to itself", () => {
  assert.equal(encode(Uint8Array.from([0x41])), "A");
  assert.equal(encode(Uint8Array.from([0x30])), "0");
  assert.deepEqual(decode("A"), Uint8Array.from([0x41]));
  assert.deepEqual(decode("0"), Uint8Array.from([0x30]));
});

test("ALPHABET round-trips to byte indices 0..255 verbatim", () => {
  // The literal ALPHABET string, decoded, must equal the identity byte sequence.
  const decoded = decode(ALPHABET);
  assert.equal(decoded.length, 256);
  for (let i = 0; i < 256; i++) assert.equal(decoded[i], i);
});
