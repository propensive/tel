// BinTEL encoder and decoder (see spec/bintel.md).
//
// This module is the JavaScript counterpart to the Rust reference
// implementation in ref/tel/src/bintel.rs. It implements:
//
//   - §4 variable-length integer encoding
//   - §6.1 external-schema file layout (magic B2 C4 B5 BB)
//   - §6.2 self-contained file layout (magic B2 C4 B5 BC)
//   - §7 node encoding (struct / scalar / flag)
//   - §8 schema signature as a palimpsest at the BinTEL-pinned parameters
//     (H, k_i, k_r) = (32, 4, 2)
//   - §10 decoder error taxonomy (B01–B12)
//
// BLAKE3 is pluggable: any function that takes a Uint8Array and returns a
// 32-byte Uint8Array is accepted. The library carries no hashing dependency
// of its own, so the caller supplies the BLAKE3 implementation (or any
// other 256-bit hash) that the wire format demands.
//
// The Schema model is a plain-object shape mirroring the Rust crate's
// `Schema` type; see the JSDoc typedefs at the bottom of this file.

// ── Constants ────────────────────────────────────────────────────────────────

// External-schema-mode magic (BASE-256: βτελ).
export const MAGIC = Uint8Array.from([0xB2, 0xC4, 0xB5, 0xBB]);

// Self-contained-mode magic (BASE-256: βτεμ).
export const MAGIC_SELF_CONTAINED = Uint8Array.from([0xB2, 0xC4, 0xB5, 0xBC]);

export const HASH_LEN = 32;

// BinTEL pins its schema-signature palimpsest at (H, k_i, k_r) = (32, 4, 2).
export const SIGNATURE_INITIAL_CADENCE = 4;
export const SIGNATURE_REGULAR_CADENCE = 2;
// Cadence byte value for (s=7, k_i-k_r=2, k_r-1=1): bits 0111_10_01 = 0x79.
export const SIGNATURE_CADENCE_BYTE = 0x79;

// ── Errors ───────────────────────────────────────────────────────────────────

// Decoder error codes correspond to §10 of the BinTEL Specification.
export const BCode = Object.freeze({
  B01: "B01", // magic absent or unrecognised
  B02: "B02", // malformed varint
  B03: "B03", // invalid signature length / cadence XOR
  B04: "B04", // signature does not decode against the library (multi-component)
  B05: "B05", // keyword index out of range
  B06: "B06", // scalar value length overruns input
  B07: "B07", // scalar bytes are not valid UTF-8
  B08: "B08", // trailing bytes after document root
  B09: "B09", // end of input mid-decode
  B10: "B10", // Reference does not resolve to a Definition
  B11: "B11", // embedded-schema signature mismatch (self-contained)
  B12: "B12", // embedded schema does not decode under tel-schema (self-contained)
});

export class BintelDecodeError extends Error {
  constructor(code, context) {
    super(`BinTEL ${code}: ${context}`);
    this.name = "BintelDecodeError";
    this.code = code;
    this.context = context;
  }
}

// ── Varint (§4) ──────────────────────────────────────────────────────────────

// Encode a non-negative integer (Number, must be a safe integer) as a
// variable-length byte sequence.
export function encodeVarint(n) {
  if (!Number.isInteger(n) || n < 0 || !Number.isSafeInteger(n)) {
    throw new TypeError(`encodeVarint: n must be a non-negative safe integer, got ${n}`);
  }
  const out = [];
  while (true) {
    let b = n & 0x7F;
    n = Math.floor(n / 128);
    if (n > 0) {
      out.push(b | 0x80);
    } else {
      out.push(b);
      return Uint8Array.from(out);
    }
  }
}

// Decode a varint starting at `bytes[offset]`. Returns { value, consumed }
// on success; throws B02 / B09 on malformed input or truncation.
export function decodeVarint(bytes, offset = 0) {
  let value = 0;
  let shift = 1;
  let i = offset;
  while (i < bytes.length) {
    const b = bytes[i++];
    value += (b & 0x7F) * shift;
    if (!Number.isSafeInteger(value)) {
      throw new BintelDecodeError(BCode.B02, "varint accumulator overflows safe integer range");
    }
    if ((b & 0x80) === 0) {
      return { value, consumed: i - offset };
    }
    shift *= 128;
    if (shift > Number.MAX_SAFE_INTEGER) {
      throw new BintelDecodeError(BCode.B02, "varint accumulator overflows");
    }
  }
  throw new BintelDecodeError(BCode.B09, "varint extends past end of input");
}

// ── Schema-driven type resolution ────────────────────────────────────────────

// Built-in type names recognised by `tel-schema` resolution. Maps the
// reference name to a resolved type descriptor (the same shape used for
// Field types after resolution).
const BUILT_INS = Object.freeze({
  Flag:       { kind: "flag" },
  String:     { kind: "scalar", validators: ["string"] },
  Identifier: { kind: "scalar", validators: ["identifier"] },
  Sigil:      { kind: "scalar", validators: ["sigil"] },
  TypeName:   { kind: "scalar", validators: ["type-name"] },
});

// Resolve a Type to one of { kind: "struct"|"scalar"|"flag" } or null if
// the Reference is dangling or kind-mismatched.
export function resolveType(type, schema) {
  if (!type) return null;
  switch (type.kind) {
    case "struct": return { kind: "struct", members: type.members, validators: type.validators ?? [] };
    case "scalar": return { kind: "scalar", validators: type.validators ?? [] };
    case "flag":   return { kind: "flag" };
    case "reference": return resolveByName(type.name, schema);
    default: return null;
  }
}

function resolveByName(name, schema) {
  if (Object.prototype.hasOwnProperty.call(BUILT_INS, name)) {
    return BUILT_INS[name];
  }
  for (const def of schema.records ?? []) {
    if (def.name === name) return { kind: "struct", members: def.members, validators: def.validators ?? [] };
  }
  for (const def of schema.scalars ?? []) {
    if (def.name === name) return { kind: "scalar", validators: def.validators ?? [] };
  }
  for (const def of schema.selects ?? []) {
    if (def.name === name) return { kind: "kindMismatch" }; // use SelectRef, not Reference
  }
  return null; // unresolved
}

// Look up the variants of a SelectDefinition by name (for SelectRef
// resolution at keyword-order time).
export function resolveSelectRef(name, schema) {
  for (const def of schema.selects ?? []) {
    if (def.name === name) return def.variants;
  }
  return null;
}

// Walk a parent's member list and return the (keyword, type) at the given
// keyword index, or null if out of range. Mirrors `lookup_by_index` in
// the Rust impl: Field contributes 1, SelectRef contributes one entry per
// resolved variant, Exclude contributes 0.
export function lookupByIndex(members, k, schema) {
  let idx = 0;
  for (const m of members) {
    if (m.kind === "field") {
      if (idx === k) return { keyword: m.keyword, type: m.type };
      idx += 1;
    } else if (m.kind === "selectRef") {
      const variants = resolveSelectRef(m.reference, schema) ?? [];
      for (const v of variants) {
        if (idx === k) return { keyword: v.keyword, type: v.type };
        idx += 1;
      }
    } // exclude contributes 0
  }
  return null;
}

// Count of keywords contributed by a member list (used for B05 range checks).
export function keywordCount(members, schema) {
  let n = 0;
  for (const m of members) {
    if (m.kind === "field") n += 1;
    else if (m.kind === "selectRef") {
      const variants = resolveSelectRef(m.reference, schema) ?? [];
      n += variants.length;
    }
  }
  return n;
}

// Position of `keyword` in the parent's keyword order, or -1 if absent.
export function keywordIndex(members, keyword, schema) {
  let idx = 0;
  for (const m of members) {
    if (m.kind === "field") {
      if (m.keyword === keyword) return idx;
      idx += 1;
    } else if (m.kind === "selectRef") {
      const variants = resolveSelectRef(m.reference, schema) ?? [];
      for (const v of variants) {
        if (v.keyword === keyword) return idx;
        idx += 1;
      }
    }
  }
  return -1;
}

// ── Encoding (§§6–7) ─────────────────────────────────────────────────────────

// A SemanticElement is the input the encoder consumes. Three shapes:
//   { keyword: string, kind: "struct", children: SemanticElement[] }
//   { keyword: string, kind: "scalar", text: string }
//   { keyword: string, kind: "flag" }
//
// The encoder relies on the schema to determine the keyword index and the
// resolved type of each child position. The caller is responsible for
// producing children in the canonical order defined by §7.2.

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder("utf-8", { fatal: true });

function concatBytes(parts) {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const p of parts) { out.set(p, offset); offset += p.length; }
  return out;
}

// Encode a list of root children as a bare document-root byte sequence
// (the bytes hashed for the value hash, §3). Excludes magic and signature.
export function encodeRoot(children, schema) {
  const parts = [];
  parts.push(encodeVarint(children.length));
  for (const child of children) {
    encodeElement(child, schema.document.members, schema, parts);
  }
  return concatBytes(parts);
}

function encodeElement(elem, parentMembers, schema, parts) {
  const k = keywordIndex(parentMembers, elem.keyword, schema);
  if (k < 0) {
    throw new Error(`encodeElement: keyword "${elem.keyword}" is not a member of the parent struct`);
  }
  parts.push(encodeVarint(k));
  const childType = lookupByIndex(parentMembers, k, schema)?.type;
  const resolved = resolveType(childType, schema);
  if (!resolved || resolved.kind === "kindMismatch") {
    throw new Error(`encodeElement: child type for "${elem.keyword}" does not resolve`);
  }
  switch (resolved.kind) {
    case "struct": {
      const children = elem.children ?? [];
      parts.push(encodeVarint(children.length));
      for (const c of children) encodeElement(c, resolved.members, schema, parts);
      return;
    }
    case "scalar": {
      const text = elem.text ?? "";
      const bytes = TEXT_ENCODER.encode(text);
      parts.push(encodeVarint(bytes.length));
      parts.push(bytes);
      return;
    }
    case "flag":
      // No body.
      return;
    default:
      throw new Error(`encodeElement: unexpected resolved kind "${resolved.kind}"`);
  }
}

// ── Decoding (§§7.8) ─────────────────────────────────────────────────────────

class Cursor {
  constructor(bytes) { this.bytes = bytes; this.pos = 0; }
  remaining() { return this.bytes.length - this.pos; }
  expect(n, codeIfShort, ctx) {
    if (this.remaining() < n) throw new BintelDecodeError(codeIfShort, ctx);
  }
  next(n) {
    const slice = this.bytes.subarray(this.pos, this.pos + n);
    this.pos += n;
    return slice;
  }
  readVarint(ctxIfMalformed) {
    try {
      const { value, consumed } = decodeVarint(this.bytes, this.pos);
      this.pos += consumed;
      return value;
    } catch (e) {
      if (e instanceof BintelDecodeError) {
        // Replace B02 context with our caller's context for clearer errors.
        if (e.code === BCode.B02) throw new BintelDecodeError(BCode.B02, ctxIfMalformed);
        throw e;
      }
      throw e;
    }
  }
}

// Decode a bare document-root bytes into a flat list of SemanticElement
// children. Used both for the outer document root (in either mode) and for
// the embedded schema body in self-contained mode.
export function decodeRoot(bytes, schema) {
  const cur = new Cursor(bytes);
  const out = decodeRootFromCursor(cur, schema);
  if (cur.remaining() !== 0) {
    throw new BintelDecodeError(BCode.B08,
      `${cur.remaining()} byte(s) remained after document root`);
  }
  return out;
}

function decodeRootFromCursor(cur, schema) {
  const childCount = cur.readVarint("malformed root child-count varint");
  const children = [];
  for (let i = 0; i < childCount; i++) {
    children.push(decodeChild(cur, schema.document.members, schema));
  }
  return children;
}

function decodeChild(cur, parentMembers, schema) {
  const k = cur.readVarint("malformed keyword-index varint");
  const lookup = lookupByIndex(parentMembers, k, schema);
  if (!lookup) {
    throw new BintelDecodeError(BCode.B05,
      `keyword index ${k} is out of range [0, ${keywordCount(parentMembers, schema)})`);
  }
  const { keyword, type } = lookup;
  const resolved = resolveType(type, schema);
  if (!resolved || resolved.kind === "kindMismatch") {
    throw new BintelDecodeError(BCode.B10,
      `Reference for "${keyword}" does not resolve cleanly to a Definition of the expected kind`);
  }
  switch (resolved.kind) {
    case "struct": {
      const childCount = cur.readVarint(`malformed child-count varint for "${keyword}"`);
      const children = [];
      for (let i = 0; i < childCount; i++) {
        children.push(decodeChild(cur, resolved.members, schema));
      }
      return { keyword, kind: "struct", children };
    }
    case "scalar": {
      const vlen = cur.readVarint(`malformed value-length varint for "${keyword}"`);
      if (cur.remaining() < vlen) {
        throw new BintelDecodeError(BCode.B06,
          `scalar "${keyword}" length ${vlen} exceeds remaining ${cur.remaining()} bytes`);
      }
      const bytes = cur.next(vlen);
      let text;
      try { text = TEXT_DECODER.decode(bytes); }
      catch (_) {
        throw new BintelDecodeError(BCode.B07, `scalar "${keyword}" is not valid UTF-8`);
      }
      return { keyword, kind: "scalar", text };
    }
    case "flag":
      return { keyword, kind: "flag" };
  }
}

// ── Schema signature (§8) ────────────────────────────────────────────────────

// Construct the §8.2 palimpsest signature from an ordered list of 32-byte
// component hashes (base first, then each layer in source order). Returns
// a Uint8Array. For n=1 the result is 33 bytes; for n≥2 it is 37 + 2·(n−2).
export function schemaSignatureFromHashes(componentHashes) {
  if (componentHashes.length === 0) {
    throw new Error("schemaSignatureFromHashes: at least one component required");
  }
  const n = componentHashes.length;
  const dataLen = n === 1 ? HASH_LEN : HASH_LEN + 4 + 2 * (n - 2);
  const body = new Uint8Array(dataLen);
  for (let i = 0; i < n; i++) {
    const h = componentHashes[i];
    if (h.length !== HASH_LEN) {
      throw new Error(`component hash ${i} must be ${HASH_LEN} bytes, got ${h.length}`);
    }
    const offset = i === 0 ? 0 : 4 + 2 * (i - 1);
    for (let j = 0; j < HASH_LEN; j++) {
      body[offset + j] ^= h[j];
    }
  }
  let dataXor = 0;
  for (let i = 0; i < dataLen; i++) dataXor ^= body[i];
  const cadenceTrailer = dataXor ^ SIGNATURE_CADENCE_BYTE;
  const out = new Uint8Array(dataLen + 1);
  out.set(body, 0);
  out[dataLen] = cadenceTrailer;
  return out;
}

// ── External-mode file layout (§6.1) ─────────────────────────────────────────

export function encodeDocument(rootChildren, schema, componentHashes) {
  const signature = schemaSignatureFromHashes(componentHashes);
  const sigLenVarint = encodeVarint(signature.length);
  const root = encodeRoot(rootChildren, schema);
  return concatBytes([MAGIC, sigLenVarint, signature, root]);
}

// Decode an external-mode BinTEL document. Returns { signature, children }.
// `schema` is the composed schema the caller has already obtained via the
// §8.2 resolution protocol.
export function decodeDocument(bytes, schema) {
  const cur = new Cursor(bytes);
  expectMagic(cur, MAGIC, "external");
  const signature = readSignature(cur);
  const children = decodeRootFromCursor(cur, schema);
  if (cur.remaining() !== 0) {
    throw new BintelDecodeError(BCode.B08,
      `${cur.remaining()} byte(s) remained after document root`);
  }
  return { signature, children };
}

function expectMagic(cur, magic, modeName) {
  cur.expect(magic.length, BCode.B09, "magic number truncated");
  const got = cur.next(magic.length);
  if (!bytesEqual(got, magic)) {
    const other = modeName === "external" ? MAGIC_SELF_CONTAINED : MAGIC;
    const otherName = modeName === "external" ? "self-contained" : "external";
    const hint = bytesEqual(got, other)
      ? `; document is in ${otherName} mode — use the matching decoder`
      : "";
    throw new BintelDecodeError(BCode.B01,
      `magic bytes were ${[...got].map(b => b.toString(16).padStart(2, "0")).join(" ")}; ` +
      `expected ${[...magic].map(b => b.toString(16).padStart(2, "0")).join(" ")}${hint}`);
  }
}

function readSignature(cur) {
  const sigLen = cur.readVarint("malformed schema-signature length varint");
  const validLength = sigLen === 33 || (sigLen >= 37 && (sigLen - 37) % 2 === 0);
  if (!validLength) {
    throw new BintelDecodeError(BCode.B03,
      `signature length ${sigLen} is not 33 (n=1) or 37 + 2·(n-2) for n ≥ 2`);
  }
  if (cur.remaining() < sigLen) {
    throw new BintelDecodeError(BCode.B09, "schema-signature bytes truncated");
  }
  const sig = cur.next(sigLen);
  let xor = 0;
  for (let i = 0; i < sig.length; i++) xor ^= sig[i];
  if (xor !== SIGNATURE_CADENCE_BYTE) {
    throw new BintelDecodeError(BCode.B03,
      `signature byte XOR 0x${xor.toString(16).padStart(2, "0")} does not equal pinned cadence byte 0x79`);
  }
  // Copy out so callers can outlive the source buffer if they wish.
  return Uint8Array.from(sig);
}

function bytesEqual(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}

// ── Self-contained-mode file layout (§6.2) ───────────────────────────────────

// Encode a document in self-contained mode. The caller supplies:
//   - `rootChildren`: the data document's root children
//   - `composedSchema`: the composed schema to encode `rootChildren` against
//   - `schemaChildren`: the schema document's own root children (encoded
//     under tel-schema)
//   - `telSchema`: the tel-schema axiom (Schema), used to encode the
//     embedded schema body
//   - `componentHashes`: the ordered component hashes of `composedSchema`
//     — base hash followed by each layer hash. The carried signature MUST
//     match the composed signature of `schemaChildren`; the caller is
//     responsible for ensuring consistency.
export function encodeDocumentSelfContained({
  rootChildren, composedSchema, schemaChildren, telSchema, componentHashes,
}) {
  const signature = schemaSignatureFromHashes(componentHashes);
  const sigLenVarint = encodeVarint(signature.length);
  const schemaBytes = encodeRoot(schemaChildren, telSchema);
  const schemaLenVarint = encodeVarint(schemaBytes.length);
  const root = encodeRoot(rootChildren, composedSchema);
  return concatBytes([
    MAGIC_SELF_CONTAINED, sigLenVarint, signature,
    schemaLenVarint, schemaBytes, root,
  ]);
}

// Decode a self-contained-mode BinTEL document.
//
// `telSchema` is the hardwired schema-for-schemas. `buildSchema` is a
// pluggable callable that receives the decoded embedded-schema children
// (under tel-schema) and returns:
//
//   { composedSchema, componentHashes }
//
// where `componentHashes` is the ordered component hashes (used for the
// B11 verification, recomputed against the carried signature) and
// `composedSchema` is the composed Schema to decode the data root under.
//
// Returns { signature, embeddedSchemaChildren, composedSchema, children }.
//
// Throws B11 if the recomputed signature doesn't match the carried one,
// B12 if the embedded schema body fails to decode or fails to construct.
export function decodeDocumentSelfContained(bytes, { telSchema, buildSchema }) {
  const cur = new Cursor(bytes);
  expectMagic(cur, MAGIC_SELF_CONTAINED, "selfContained");
  const signature = readSignature(cur);

  const schemaLen = cur.readVarint("malformed embedded-schema length varint");
  if (cur.remaining() < schemaLen) {
    throw new BintelDecodeError(BCode.B09, "embedded schema body truncated");
  }
  const schemaBytes = cur.next(schemaLen);

  let embeddedSchemaChildren;
  try {
    embeddedSchemaChildren = decodeRoot(schemaBytes, telSchema);
  } catch (e) {
    throw new BintelDecodeError(BCode.B12,
      `embedded schema body does not decode under tel-schema: ${e.message ?? e}`);
  }

  let built;
  try {
    built = buildSchema(embeddedSchemaChildren);
  } catch (e) {
    throw new BintelDecodeError(BCode.B12,
      `buildSchema failed: ${e.message ?? e}`);
  }
  const { composedSchema, componentHashes } = built;
  const recomputed = schemaSignatureFromHashes(componentHashes);
  if (!bytesEqual(recomputed, signature)) {
    throw new BintelDecodeError(BCode.B11,
      `embedded schema body's recomputed signature (${recomputed.length} bytes) ` +
      `does not equal the carried signature (${signature.length} bytes)`);
  }

  const children = decodeRootFromCursor(cur, composedSchema);
  if (cur.remaining() !== 0) {
    throw new BintelDecodeError(BCode.B08,
      `${cur.remaining()} byte(s) remained after document root`);
  }
  return { signature, embeddedSchemaChildren, composedSchema, children };
}

// ── Schema-to-BinTEL (helper for self-contained-mode producers) ──────────────

// Encode a schema document as a complete external-mode BinTEL document
// under the tel-schema axiom. The carried signature is tel-schema's
// signature; the body is the schema's root children encoded under
// tel-schema.
//
// `schemaChildren` is the schema document's root children.
// `telSchema` is the tel-schema axiom.
// `telSchemaValueHash` is tel-schema's BLAKE3-256 value hash (Uint8Array
// of HASH_LEN bytes); the caller supplies it because BLAKE3 is pluggable.
export function schemaToBintel(schemaChildren, telSchema, telSchemaValueHash) {
  return encodeDocument(schemaChildren, telSchema, [telSchemaValueHash]);
}

// ── Hash convenience ─────────────────────────────────────────────────────────

// Compute the BinTEL value hash (§3) of a list of root children under the
// given schema, using a pluggable BLAKE3 implementation. `blake3` is a
// callable that takes a Uint8Array and returns a 32-byte Uint8Array.
export function valueHash(children, schema, blake3) {
  return blake3(encodeRoot(children, schema));
}

// JSDoc typedefs — informational, no runtime effect.
//
// @typedef {Object} Schema
// @property {string} name
// @property {Struct} document
// @property {Layer[]} layers
// @property {string|null} sigil
// @property {RecordDefinition[]} records
// @property {ScalarDefinition[]} scalars
// @property {SelectDefinition[]} selects
//
// @typedef {Object} Struct
// @property {Member[]} members
// @property {string[]} [validators]
//
// @typedef {{kind:"field", keyword:string, type:Type, required?:string, repeatable?:string, default?:string}
//   | {kind:"selectRef", reference:string, required?:string, repeatable?:string}
//   | {kind:"exclude", keyword:string}} Member
//
// @typedef {{kind:"struct", members:Member[], validators?:string[]}
//   | {kind:"scalar", validators?:string[]}
//   | {kind:"flag"}
//   | {kind:"reference", name:string}} Type
