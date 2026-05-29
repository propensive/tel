import { test } from "node:test";
import assert from "node:assert/strict";
import {
  MAGIC, MAGIC_SELF_CONTAINED, HASH_LEN, SIGNATURE_CADENCE_BYTE, BCode, BintelDecodeError,
  encodeVarint, decodeVarint,
  encodeRoot, decodeRoot,
  encodeDocument, decodeDocument,
  encodeDocumentSelfContained, decodeDocumentSelfContained,
  schemaToBintel, schemaSignatureFromHashes, valueHash,
  keywordIndex, lookupByIndex, keywordCount,
} from "./bintel.js";

// ── Helpers ──────────────────────────────────────────────────────────────────

// A trivial Schema with one required `name` scalar field.
const nameSchema = {
  name: "demo",
  document: {
    members: [{ kind: "field", keyword: "name", type: { kind: "scalar", validators: ["string"] } }],
    validators: [],
  },
  layers: [], sigil: null, records: [], scalars: [], selects: [],
};

// A Schema with one struct field `person` containing two scalar children.
const personSchema = {
  name: "person-demo",
  document: {
    members: [{
      kind: "field", keyword: "person",
      type: { kind: "struct", members: [
        { kind: "field", keyword: "first", type: { kind: "scalar", validators: ["string"] } },
        { kind: "field", keyword: "last",  type: { kind: "scalar", validators: ["string"] } },
      ] },
    }],
    validators: [],
  },
  layers: [], sigil: null, records: [], scalars: [], selects: [],
};

// A Schema with one optional Flag field.
const flagSchema = {
  name: "flag-demo",
  document: {
    members: [{ kind: "field", keyword: "ok", type: { kind: "flag" } }],
    validators: [],
  },
  layers: [], sigil: null, records: [], scalars: [], selects: [],
};

// A SelectRef-based schema: one Select with two flag variants `a` and `b`.
const selectSchema = {
  name: "select-demo",
  document: {
    members: [{ kind: "selectRef", reference: "Choice" }],
    validators: [],
  },
  layers: [], sigil: null, records: [], scalars: [],
  selects: [{
    name: "Choice",
    variants: [
      { keyword: "a", type: { kind: "flag" } },
      { keyword: "b", type: { kind: "flag" } },
    ],
    validators: [],
    layerExcludes: [],
  }],
};

// A deterministic stand-in for BLAKE3 used by the self-contained-mode and
// signature tests. Real BLAKE3 must be supplied by the caller in
// production; here we use a simple linear-hash function so tests don't
// pull in a hashing dependency. As long as the same input produces the
// same 32-byte output, the wire-format machinery can be exercised.
function stubBlake3(data) {
  const out = new Uint8Array(HASH_LEN);
  for (let i = 0; i < data.length; i++) {
    // FNV-1a–ish: rotate by position, fold into a 32-byte window.
    out[i % HASH_LEN] ^= data[i];
    out[(i * 7 + 13) % HASH_LEN] = (out[(i * 7 + 13) % HASH_LEN] + data[i]) & 0xFF;
  }
  // Salt with a fixed value so the all-zero input still has a deterministic hash.
  for (let i = 0; i < HASH_LEN; i++) out[i] ^= 0x5A;
  return out;
}

// ── §4 Variable-length integer ───────────────────────────────────────────────

test("varint test vectors from spec §4", () => {
  const vectors = [
    [0,     [0x00]],
    [1,     [0x01]],
    [127,   [0x7F]],
    [128,   [0x80, 0x01]],
    [255,   [0xFF, 0x01]],
    [16383, [0xFF, 0x7F]],
    [16384, [0x80, 0x80, 0x01]],
  ];
  for (const [n, expected] of vectors) {
    const enc = encodeVarint(n);
    assert.deepEqual(Array.from(enc), expected, `encode(${n})`);
    const { value, consumed } = decodeVarint(enc);
    assert.equal(value, n);
    assert.equal(consumed, expected.length);
  }
});

test("varint round-trip random small/medium/large values", () => {
  const samples = [0, 1, 7, 63, 64, 127, 128, 200, 500, 1234, 16_000, 16_384, 50_000, 1_000_000];
  for (const n of samples) {
    const enc = encodeVarint(n);
    const { value } = decodeVarint(enc);
    assert.equal(value, n);
  }
});

test("varint malformed input → B02 / B09", () => {
  // Truncated continuation byte.
  assert.throws(() => decodeVarint(Uint8Array.from([0x80])),
    e => e instanceof BintelDecodeError && e.code === BCode.B09);
});

// ── §7 Node encoding ─────────────────────────────────────────────────────────

test("encodeRoot: minimal scalar", () => {
  const children = [{ keyword: "name", kind: "scalar", text: "Alice" }];
  // child_count=1, kidx=0, value_len=5, "Alice"
  assert.deepEqual(Array.from(encodeRoot(children, nameSchema)),
    [0x01, 0x00, 0x05, 0x41, 0x6c, 0x69, 0x63, 0x65]);
});

test("encodeRoot: flag has no body", () => {
  const children = [{ keyword: "ok", kind: "flag" }];
  // child_count=1, kidx=0, no body.
  assert.deepEqual(Array.from(encodeRoot(children, flagSchema)), [0x01, 0x00]);
});

test("encodeRoot: nested struct round-trip", () => {
  const children = [{
    keyword: "person", kind: "struct", children: [
      { keyword: "first", kind: "scalar", text: "Alice" },
      { keyword: "last",  kind: "scalar", text: "Liddell" },
    ],
  }];
  const bytes = encodeRoot(children, personSchema);
  const decoded = decodeRoot(bytes, personSchema);
  assert.deepEqual(decoded, children);
});

test("encodeRoot: empty string scalar encodes as length-0", () => {
  const children = [{ keyword: "name", kind: "scalar", text: "" }];
  // child_count=1, kidx=0, value_len=0, no bytes.
  assert.deepEqual(Array.from(encodeRoot(children, nameSchema)), [0x01, 0x00, 0x00]);
});

test("encodeRoot: SelectRef variant encodes at variant's keyword index", () => {
  // Choice has variants a (index 0) and b (index 1).
  const children = [{ keyword: "b", kind: "flag" }];
  assert.deepEqual(Array.from(encodeRoot(children, selectSchema)), [0x01, 0x01]);
});

test("keywordIndex and lookupByIndex agree on the Select case", () => {
  const members = selectSchema.document.members;
  assert.equal(keywordIndex(members, "a", selectSchema), 0);
  assert.equal(keywordIndex(members, "b", selectSchema), 1);
  assert.equal(keywordCount(members, selectSchema), 2);
  assert.equal(lookupByIndex(members, 0, selectSchema).keyword, "a");
  assert.equal(lookupByIndex(members, 1, selectSchema).keyword, "b");
  assert.equal(lookupByIndex(members, 2, selectSchema), null);
});

// ── §6.1 External-mode round trip ────────────────────────────────────────────

test("encode/decodeDocument: external-mode round trip with stub hash", () => {
  const children = [{ keyword: "name", kind: "scalar", text: "Alice" }];
  const baseHash = valueHash(children, nameSchema, stubBlake3);
  assert.equal(baseHash.length, HASH_LEN);
  const bytes = encodeDocument(children, nameSchema, [baseHash]);
  // Header layout sanity check.
  assert.deepEqual(Array.from(bytes.subarray(0, 4)), Array.from(MAGIC));
  const decoded = decodeDocument(bytes, nameSchema);
  assert.equal(decoded.signature.length, 33);
  assert.deepEqual(Array.from(decoded.signature.subarray(0, HASH_LEN)), Array.from(baseHash));
  assert.deepEqual(decoded.children, children);
});

test("decodeDocument: bad magic → B01", () => {
  const bytes = Uint8Array.from([0xDE, 0xAD, 0xBE, 0xEF, 0x21]);
  assert.throws(() => decodeDocument(bytes, nameSchema),
    e => e instanceof BintelDecodeError && e.code === BCode.B01);
});

test("decodeDocument: self-contained magic on external decoder → B01 with hint", () => {
  // Build a self-contained document and try to decode it as external mode.
  const children = [{ keyword: "name", kind: "scalar", text: "Bob" }];
  const baseHash = valueHash(children, nameSchema, stubBlake3);
  const bytes = encodeDocumentSelfContained({
    rootChildren: children, composedSchema: nameSchema,
    schemaChildren: children, telSchema: nameSchema,
    componentHashes: [baseHash],
  });
  try {
    decodeDocument(bytes, nameSchema);
    assert.fail("expected B01");
  } catch (e) {
    assert.ok(e instanceof BintelDecodeError);
    assert.equal(e.code, BCode.B01);
    assert.match(e.context, /self-contained/);
  }
});

test("decodeDocument: bad signature length → B03", () => {
  // magic + sig_len=35 (invalid: not 33 and not 37+2(n-2)) + 35 zero bytes.
  const bytes = new Uint8Array(4 + 1 + 35);
  bytes.set(MAGIC, 0);
  bytes[4] = 35;
  assert.throws(() => decodeDocument(bytes, nameSchema),
    e => e instanceof BintelDecodeError && e.code === BCode.B03);
});

test("decodeDocument: bad cadence XOR → B03", () => {
  // magic + sig_len=33 + 33 zero bytes (XOR=0, not 0x79).
  const bytes = new Uint8Array(4 + 1 + 33);
  bytes.set(MAGIC, 0);
  bytes[4] = 33;
  assert.throws(() => decodeDocument(bytes, nameSchema),
    e => e instanceof BintelDecodeError && e.code === BCode.B03 && /XOR/.test(e.context));
});

test("decodeDocument: keyword index out of range → B05", () => {
  // magic + valid 33-byte sig + child_count=1 + kidx=99.
  const sig = craftValidSignature();
  const bytes = new Uint8Array(4 + 1 + 33 + 1 + 1);
  bytes.set(MAGIC, 0);
  bytes[4] = 33;
  bytes.set(sig, 5);
  bytes[38] = 0x01;   // child_count
  bytes[39] = 99;     // kidx (well over 1 member)
  assert.throws(() => decodeDocument(bytes, nameSchema),
    e => e instanceof BintelDecodeError && e.code === BCode.B05);
});

test("decodeDocument: trailing bytes → B08", () => {
  const children = [{ keyword: "name", kind: "scalar", text: "Alice" }];
  const baseHash = valueHash(children, nameSchema, stubBlake3);
  const valid = encodeDocument(children, nameSchema, [baseHash]);
  const bytes = new Uint8Array(valid.length + 3);
  bytes.set(valid, 0);
  assert.throws(() => decodeDocument(bytes, nameSchema),
    e => e instanceof BintelDecodeError && e.code === BCode.B08);
});

// Hand-craft a valid 33-byte signature whose first 32 bytes are zero and
// whose cadence trailer makes the byte-XOR equal 0x79.
function craftValidSignature() {
  const sig = new Uint8Array(33);
  sig[32] = SIGNATURE_CADENCE_BYTE; // all-zero body XOR is 0; trailer = 0 ^ 0x79.
  return sig;
}

// ── §8.2 Signature palimpsest ────────────────────────────────────────────────

test("schemaSignatureFromHashes: single component is 33 bytes, XOR == 0x79", () => {
  const h = new Uint8Array(HASH_LEN);
  for (let i = 0; i < HASH_LEN; i++) h[i] = i + 1;
  const sig = schemaSignatureFromHashes([h]);
  assert.equal(sig.length, 33);
  let xor = 0;
  for (let i = 0; i < sig.length; i++) xor ^= sig[i];
  assert.equal(xor, SIGNATURE_CADENCE_BYTE);
  // First 32 bytes are the hash verbatim.
  assert.deepEqual(Array.from(sig.subarray(0, HASH_LEN)), Array.from(h));
});

test("schemaSignatureFromHashes: two components is 37 bytes", () => {
  const a = new Uint8Array(HASH_LEN).fill(0xAA);
  const b = new Uint8Array(HASH_LEN).fill(0x55);
  const sig = schemaSignatureFromHashes([a, b]);
  assert.equal(sig.length, 37);
  let xor = 0;
  for (let i = 0; i < sig.length; i++) xor ^= sig[i];
  assert.equal(xor, SIGNATURE_CADENCE_BYTE);
});

test("schemaSignatureFromHashes: three components is 39 bytes", () => {
  const a = new Uint8Array(HASH_LEN).fill(0x01);
  const b = new Uint8Array(HASH_LEN).fill(0x02);
  const c = new Uint8Array(HASH_LEN).fill(0x03);
  const sig = schemaSignatureFromHashes([a, b, c]);
  assert.equal(sig.length, 39);
});

// ── §6.2 Self-contained mode ─────────────────────────────────────────────────

test("encode/decodeDocumentSelfContained: round trip", () => {
  // Use nameSchema as both "tel-schema" and the embedded "data schema"
  // — the wire-format mechanics are agnostic to that choice. The
  // embedded body is the same children we'd encode at the root if we
  // wanted; for this test we use a separate small embedded payload.
  const dataChildren = [{ keyword: "name", kind: "scalar", text: "Alice" }];
  const schemaChildren = [{ keyword: "name", kind: "scalar", text: "schema-marker" }];
  const baseHash = valueHash(schemaChildren, nameSchema, stubBlake3);

  const bytes = encodeDocumentSelfContained({
    rootChildren: dataChildren, composedSchema: nameSchema,
    schemaChildren, telSchema: nameSchema,
    componentHashes: [baseHash],
  });
  // Header magic is the self-contained variant.
  assert.deepEqual(Array.from(bytes.subarray(0, 4)), Array.from(MAGIC_SELF_CONTAINED));

  const buildSchema = (decodedSchemaChildren) => {
    assert.deepEqual(decodedSchemaChildren, schemaChildren);
    return {
      composedSchema: nameSchema,
      componentHashes: [valueHash(decodedSchemaChildren, nameSchema, stubBlake3)],
    };
  };

  const decoded = decodeDocumentSelfContained(bytes, { telSchema: nameSchema, buildSchema });
  assert.equal(decoded.signature.length, 33);
  assert.deepEqual(decoded.children, dataChildren);
  assert.deepEqual(decoded.embeddedSchemaChildren, schemaChildren);
});

test("decodeDocumentSelfContained: tampered embedded body → B11 or B12", () => {
  const dataChildren = [{ keyword: "name", kind: "scalar", text: "Charlie" }];
  const schemaChildren = [{ keyword: "name", kind: "scalar", text: "schema" }];
  const baseHash = valueHash(schemaChildren, nameSchema, stubBlake3);

  const bytes = encodeDocumentSelfContained({
    rootChildren: dataChildren, composedSchema: nameSchema,
    schemaChildren, telSchema: nameSchema,
    componentHashes: [baseHash],
  });
  // Flip a byte in the embedded body. Layout: 4 magic + 1 sig_len_varint +
  // 33 sig + 1 schema_len_varint (small) + N schema bytes + ...
  // schema_len varint starts at offset 38; for a small embedded body it's 1 byte.
  const schemaStart = 38 + 1;
  bytes[schemaStart] ^= 0xFF;

  const buildSchema = (decodedSchemaChildren) => ({
    composedSchema: nameSchema,
    componentHashes: [valueHash(decodedSchemaChildren, nameSchema, stubBlake3)],
  });

  try {
    decodeDocumentSelfContained(bytes, { telSchema: nameSchema, buildSchema });
    assert.fail("expected B11 or B12 after tampering");
  } catch (e) {
    assert.ok(e instanceof BintelDecodeError, `got ${e}`);
    assert.ok(e.code === BCode.B11 || e.code === BCode.B12,
      `expected B11 or B12, got ${e.code}`);
  }
});

test("decodeDocumentSelfContained: external magic → B01 with hint", () => {
  const children = [{ keyword: "name", kind: "scalar", text: "X" }];
  const baseHash = valueHash(children, nameSchema, stubBlake3);
  const bytes = encodeDocument(children, nameSchema, [baseHash]);
  try {
    decodeDocumentSelfContained(bytes, { telSchema: nameSchema, buildSchema: () => ({}) });
    assert.fail("expected B01");
  } catch (e) {
    assert.ok(e instanceof BintelDecodeError);
    assert.equal(e.code, BCode.B01);
    assert.match(e.context, /external/);
  }
});

// ── schema_to_bintel helper ──────────────────────────────────────────────────

test("schemaToBintel: encodes a schema document under tel-schema with tel-schema's signature", () => {
  const schemaChildren = [{ keyword: "name", kind: "scalar", text: "my-schema" }];
  const telSchemaValueHash = valueHash(
    [{ keyword: "name", kind: "scalar", text: "tel-schema" }],
    nameSchema,
    stubBlake3,
  );
  const bytes = schemaToBintel(schemaChildren, nameSchema, telSchemaValueHash);
  const decoded = decodeDocument(bytes, nameSchema);
  // Carried signature equals the tel-schema-stand-in signature.
  const expectedSig = schemaSignatureFromHashes([telSchemaValueHash]);
  assert.deepEqual(decoded.signature, expectedSig);
  // The decoded body is the schema-document children.
  assert.deepEqual(decoded.children, schemaChildren);
});

// ── Value-hash invariance (§3) ───────────────────────────────────────────────

test("value hash is mode-invariant: external and self-contained produce identical root bytes", () => {
  const dataChildren = [{ keyword: "name", kind: "scalar", text: "Bob" }];
  const schemaChildren = [{ keyword: "name", kind: "scalar", text: "schema" }];
  const baseHash = valueHash(schemaChildren, nameSchema, stubBlake3);

  const external = encodeDocument(dataChildren, nameSchema, [baseHash]);
  const selfContained = encodeDocumentSelfContained({
    rootChildren: dataChildren, composedSchema: nameSchema,
    schemaChildren, telSchema: nameSchema,
    componentHashes: [baseHash],
  });

  // Strip headers and compare the trailing root encoding bytes.
  // External: 4 magic + 1 sig_len + 33 sig + root.
  const externalRoot = external.subarray(4 + 1 + 33);
  // Self-contained: 4 magic + 1 sig_len + 33 sig + 1 schema_len + N schema + root.
  const sc1 = decodeVarint(selfContained, 4 + 1 + 33);
  const selfContainedRoot = selfContained.subarray(4 + 1 + 33 + sc1.consumed + sc1.value);
  assert.deepEqual(Array.from(externalRoot), Array.from(selfContainedRoot),
    "root bytes are byte-identical between modes");

  // Therefore value hash is identical too.
  assert.deepEqual(stubBlake3(externalRoot), stubBlake3(selfContainedRoot));
});
