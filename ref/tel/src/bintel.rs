//! BinTEL encoder and decoder.
//!
//! Implements the encoding defined in `spec/bintel.md`:
//!
//! - §4 variable-length integer encoding.
//! - §6 file layout (magic number `B2 C4 B5 BB` / BASE-256 `βτελ`, schema signature, document root).
//! - §7 node encoding (struct / scalar / flag, with default-value
//!   canonicalization).
//! - §3 value hash (BLAKE3-256 of the document root encoding alone).
//! - §8 schema signature as a palimpsest at the pinned parameters
//!   `(H, k_i, k_r) = (32, 4, 2)`.
//!
//! See also `base256.rs` for the textual form (§9) and the `palimpsest` crate
//! for the underlying construction.

use crate::{
    Atom, Block, CodecBindingFn, CodecResolver, Compound, Diagnostic, Document, LineEndings,
    Member, Schema, Struct, Type,
    builtin_tels, compose_schema, construct_schema,
    resolve, ResolvedType, scalar_value_text,
};
#[cfg(test)]
use crate::Polarity;

/// The BinTEL external-schema-mode magic number: four bytes `B2 C4 B5 BB`.
/// In BASE-256 textual form (§9 of the spec) these render as the four Greek
/// letters `βτελ` — `β` for "binary", `τελ` the Greek root for *tel*-. None
/// of the bytes is below `0x80`, so a BinTEL stream cannot be mistaken for
/// the start of an ASCII or UTF-8 text file.
pub const MAGIC: [u8; 4] = [0xB2, 0xC4, 0xB5, 0xBB];

/// The BinTEL self-contained-mode magic number: four bytes `B2 C4 B5 BC`.
/// In BASE-256 textual form these render as `βτεμ` — `μ` (Greek small mu,
/// `U+03BC`) for *monolithic*, distinguishing this mode from external
/// mode's `βτελ`. See §6.2 of the BinTEL Specification.
pub const MAGIC_SELF_CONTAINED: [u8; 4] = [0xB2, 0xC4, 0xB5, 0xBC];

pub const HASH_LEN: usize = 32;

/// BinTEL pins its schema-signature palimpsest at `(H, k_i, k_r) = (32, 4, 2)`
/// per spec/bintel.md §8.
pub const SIGNATURE_INITIAL_CADENCE: u8 = 4;
pub const SIGNATURE_REGULAR_CADENCE: u8 = 2;
/// Cadence byte value for the BinTEL-pinned palimpsest parameters
/// (s=7, k_i-k_r=2, k_r-1=1 → bits 0111_10_01 = 0x79).
pub const SIGNATURE_CADENCE_BYTE: u8 = 0x79;

// ── Integer encoding (§4) ────────────────────────────────────────────────────

/// Encode a non-negative integer as a variable-length byte sequence (§4).
pub fn encode_varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut b = (n & 0x7F) as u8;
        n >>= 7;
        if n > 0 {
            b |= 0x80;
            out.push(b);
        } else {
            out.push(b);
            return out;
        }
    }
}

/// Decode a variable-length integer starting at `bytes[0]`. Returns
/// `(value, byte_count)` on success.
pub fn decode_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let chunk = (b & 0x7F) as u64;
        // §4 pins the representable range to [0, 2^64 − 1]. Ten bytes carry
        // 70 bits, so a tenth byte may contribute only the low bit; anything
        // wider is ↯/t.4.2 rather than a silently truncated value.
        if shift >= 64 || (shift == 63 && chunk > 1) {
            return None;
        }
        value |= chunk << shift;
        if b & 0x80 == 0 {
            // §4 also requires the *minimal* encoding. A multi-byte encoding
            // whose terminating byte contributes no bits is overlong (`80 00`
            // for zero) and would give one integer many encodings, breaking
            // the byte-determinism of §7 and the value hash of §3.
            if i > 0 && chunk == 0 {
                return None;
            }
            return Some((value, i + 1));
        }
        shift += 7;
    }
    None // ran out of bytes before terminator
}

// ── Keyword order (§5) ───────────────────────────────────────────────────────

/// Returns the keyword index (position in keyword order) of the given keyword
/// among the parent's members, or `None` if absent.
pub fn keyword_index(members: &[Member], keyword: &str, schema: &Schema) -> Option<usize> {
    let mut idx = 0;
    for m in members {
        match m {
            Member::Field(f) => {
                if f.keyword == keyword { return Some(idx); }
                idx += 1;
            }
            Member::SelectRef(s) => {
                if let Some(variants) = crate::resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        if v.keyword == keyword { return Some(idx); }
                        idx += 1;
                    }
                }
            }
            Member::Exclude(_) => {
                // Exclude ops are layer-only; not in a composed schema.
            }
        }
    }
    None
}

/// Return the `Type` declared at the given keyword position, or `None`.
pub fn keyword_type<'a>(members: &'a [Member], keyword: &str, schema: &'a Schema) -> Option<&'a Type> {
    for m in members {
        match m {
            Member::Field(f) => if f.keyword == keyword { return Some(&f.r#type); },
            Member::SelectRef(s) => {
                if let Some(variants) = crate::resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        if v.keyword == keyword { return Some(&v.r#type); }
                    }
                }
            }
            Member::Exclude(_) => {}
        }
    }
    None
}

/// Return the member index of the member that declares the given keyword (a
/// Field's keyword or any Select variant's keyword via SelectRef).
pub(crate) fn member_index(members: &[Member], keyword: &str, schema: &Schema) -> Option<usize> {
    for (i, m) in members.iter().enumerate() {
        match m {
            Member::Field(f) => if f.keyword == keyword { return Some(i); },
            Member::SelectRef(s) => {
                if let Some(variants) = crate::resolve_select_ref(&s.reference, schema) {
                    if variants.iter().any(|v| v.keyword == keyword) {
                        return Some(i);
                    }
                }
            }
            Member::Exclude(_) => {}
        }
    }
    None
}

// ── Encoding (§§6–7) ─────────────────────────────────────────────────────────

use crate::atom_text;

/// A semantic-model element to be encoded as one BinTEL child node (§7.2).
///
/// Atom-derived elements correspond to inline atoms that the type-assignment
/// algorithm (§20.2 of the TEL Specification) assigned to a parent member.
/// Compound-derived elements correspond to compound lines beneath the parent.
/// Default elements correspond to required Scalar Fields with non-null
/// defaults that were not filled by any atom or compound child (§7.5).
pub(crate) enum Element<'a> {
    /// Compound child appearing under the parent compound.
    Compound(&'a Compound),
    /// Atom-filled Scalar element: an inline atom on the parent's line
    /// assigned to a Field of Scalar type. The `keyword` is the Field's
    /// keyword; the `text` is the atom's text.
    AtomScalar { keyword: &'a str, text: String },
    /// Atom-filled Flag element: an inline atom matching either a Flag
    /// Field's keyword or one variant keyword of an all-Flag Select. The
    /// `keyword` is the matched keyword (Field.keyword or Variant.keyword).
    AtomFlag { keyword: &'a str },
    /// Default Scalar substitution for an absent required Field.
    DefaultScalar { keyword: &'a str, value: &'a str },
}

/// Walk a compound (or the document root) and enumerate its semantic
/// children in canonical order (§7.2): in member order, atom-derived
/// elements first, then compound-derived elements, then a default
/// substitution if applicable.
pub(crate) fn enumerate_children<'a>(
    atoms: &'a [Atom],
    blocks: &'a [Block],
    members: &'a [Member],
    schema: &'a Schema,
) -> Vec<Element<'a>> {
    // ── Atom phase: assign each atom to a member, mimicking §20.2 ──
    let mut atom_assignments: Vec<(usize, Element<'a>)> = Vec::new();
    let mut pos: usize = 0;
    for atom in atoms {
        let atext = atom_text(atom);
        while pos < members.len() && should_skip_member(members, pos, &atext, schema) {
            pos += 1;
        }
        if pos >= members.len() { break; }
        let m = &members[pos];
        match m {
            Member::Field(f) => match resolve(&f.r#type, schema) {
                ResolvedType::Scalar(_) => {
                    atom_assignments.push((pos, Element::AtomScalar {
                        keyword: &f.keyword,
                        text: atext,
                    }));
                    if !f.repeatable.effective_repeatable() { pos += 1; }
                }
                ResolvedType::Flag => {
                    // atom_text == f.keyword (enforced by skip rule)
                    atom_assignments.push((pos, Element::AtomFlag { keyword: &f.keyword }));
                    if !f.repeatable.effective_repeatable() { pos += 1; }
                }
                _ => {
                    // Non-atom-assignable; type assignment would flag ↯/t.3.3.
                    break;
                }
            },
            Member::SelectRef(s) => {
                // Only atom-assignable if every variant of the referenced
                // SelectDefinition resolves to Flag.
                let variants = crate::resolve_select_ref(&s.reference, schema)
                    .unwrap_or(&[]);
                if let Some(v) = variants.iter().find(|v| v.keyword == atext) {
                    atom_assignments.push((pos, Element::AtomFlag { keyword: &v.keyword }));
                    if !s.repeatable.effective_repeatable() { pos += 1; }
                } else {
                    // ↯/t.3.4 territory; skip the atom.
                    break;
                }
            }
            Member::Exclude(_) => { pos += 1; }
        }
    }

    // ── Compound phase: bucket children by member index ──
    let mut compound_by_member: Vec<Vec<&Compound>> = vec![Vec::new(); members.len()];
    for block in blocks {
        for c in &block.compounds {
            if let Some(i) = member_index(members, &c.keyword, schema) {
                compound_by_member[i].push(c);
            }
            // Unknown keyword would trigger ↯/t.3.6 at type-assignment time.
        }
    }

    // ── Emit in canonical order ──
    let mut out: Vec<Element<'a>> = Vec::new();
    for (i, m) in members.iter().enumerate() {
        // Atom-derived elements first.
        let mut had_filling = false;
        for (j, ae) in &atom_assignments {
            if *j == i {
                out.push(clone_element(ae));
                had_filling = true;
            }
        }
        // Compound-derived elements next.
        for c in &compound_by_member[i] {
            out.push(Element::Compound(c));
            had_filling = true;
        }
        // Default substitution if still unfilled and applicable.
        if !had_filling {
            if let Member::Field(f) = m {
                if f.required.effective_required() {
                    if let Some(def) = &f.default {
                        out.push(Element::DefaultScalar {
                            keyword: &f.keyword,
                            value: def,
                        });
                    }
                }
            }
        }
    }
    out
}

/// True when the member at `pos` would be skipped by the atom-phase skip
/// rule (§20.2 step 3a): non-required, and either non-atom-assignable or
/// an atom-assignable Flag-shaped member whose keyword doesn't match.
fn should_skip_member(
    members: &[Member],
    pos: usize,
    atom_text: &str,
    schema: &Schema,
) -> bool {
    let m = &members[pos];
    match m {
        Member::Field(f) => {
            if f.required.effective_required() { return false; }
            let resolved = resolve(&f.r#type, schema);
            let atom_assignable = matches!(resolved, ResolvedType::Scalar(_) | ResolvedType::Flag);
            if !atom_assignable { return true; }
            // For Flag, skip if atom doesn't match the keyword.
            if matches!(resolved, ResolvedType::Flag) && f.keyword != atom_text {
                return true;
            }
            false
        }
        Member::SelectRef(s) => {
            if s.required.effective_required() { return false; }
            let variants = crate::resolve_select_ref(&s.reference, schema).unwrap_or(&[]);
            let all_flag = variants.iter().all(|v|
                matches!(resolve(&v.r#type, schema), ResolvedType::Flag));
            if !all_flag { return true; }
            !variants.iter().any(|v| v.keyword == atom_text)
        }
        Member::Exclude(_) => true,
    }
}

fn clone_element<'a>(e: &Element<'a>) -> Element<'a> {
    match e {
        Element::Compound(c) => Element::Compound(*c),
        Element::AtomScalar { keyword, text } => Element::AtomScalar {
            keyword: *keyword, text: text.clone(),
        },
        Element::AtomFlag { keyword } => Element::AtomFlag { keyword: *keyword },
        Element::DefaultScalar { keyword, value } => Element::DefaultScalar {
            keyword: *keyword, value: *value,
        },
    }
}

/// Error from a codec-aware BinTEL encode (§7.1 / TEL §21.7): an
/// unresolved encoding name or an encoder-rejected value. Per §7.1 the
/// encoder MUST fail without emitting a document — BinTEL never encodes
/// an invalid document.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeError {
    pub message: String,
}

fn diagnostic_message(diag: &Diagnostic) -> &str {
    match diag {
        Diagnostic::Scalar { message, .. } => message,
        Diagnostic::Struct { message, .. } => message,
    }
}

/// Encode a scalar value's payload (length varint + value bytes) per §7.1:
/// UTF-8 text when the type declares no encoding, the bound codec's bytes
/// otherwise.
fn encode_scalar_payload(
    sc: &crate::Scalar,
    text: &str,
    keyword: &str,
    codecs: &CodecResolver,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match &sc.encoding {
        None => {
            let bytes = text.as_bytes();
            out.extend(encode_varint(bytes.len() as u64));
            out.extend_from_slice(bytes);
        }
        Some(name) => {
            let codec = codecs.resolve(name).ok_or_else(|| EncodeError {
                message: format!(
                    "scalar `{}` declares encoding `{}`, which the codec binding does not resolve",
                    keyword, name,
                ),
            })?;
            let bytes = codec.encode(text).map_err(|diag| EncodeError {
                message: format!(
                    "scalar `{}` value rejected by codec `{}`: {}",
                    keyword, name, diagnostic_message(&diag),
                ),
            })?;
            out.extend(encode_varint(bytes.len() as u64));
            out.extend_from_slice(&bytes);
        }
    }
    Ok(())
}

/// Encode the document root (§7.1). The result is the bytes hashed for the
/// value hash (§3); it excludes the magic number and the schema signature.
///
/// This entry point has no codec binding and therefore supports only
/// schemas that declare no encodings; encountering an encoded scalar
/// panics. Use `encode_root_with_codecs` for schemas with encodings.
pub fn encode_root(doc: &Document, schema: &Schema) -> Vec<u8> {
    encode_root_with_codecs(doc, schema, None)
        .expect("schema declares encodings; use encode_root_with_codecs with a codec binding")
}

/// `encode_root` with a codec binding (TEL §21.7). Required whenever the
/// composed schema declares any encoding; fails (rather than emitting a
/// partial document) on an unresolved encoding name or an encoder-rejected
/// value.
pub fn encode_root_with_codecs(
    doc: &Document,
    schema: &Schema,
    codec_binding: Option<&CodecBindingFn>,
) -> Result<Vec<u8>, EncodeError> {
    let codecs = CodecResolver::new(codec_binding);
    let mut out = Vec::new();
    // The document root has no atoms (it's a virtual struct).
    let children = enumerate_children(&[], &doc.children, &schema.document.members, schema);
    out.extend(encode_varint(children.len() as u64));
    for child in &children {
        encode_element(child, &schema.document.members, schema, &codecs, &mut out)?;
    }
    Ok(out)
}

/// Encode one semantic-model element (§7.1) into `out`.
fn encode_element<'a>(
    elem: &Element<'a>,
    parent_members: &'a [Member],
    schema: &'a Schema,
    codecs: &CodecResolver,
    out: &mut Vec<u8>,
) -> Result<(), EncodeError> {
    match elem {
        Element::Compound(c) => {
            let kidx = keyword_index(parent_members, &c.keyword, schema)
                .expect("keyword must resolve; type assignment should have caught ↯/t.3.6");
            let t = keyword_type(parent_members, &c.keyword, schema)
                .expect("keyword must resolve");
            out.extend(encode_varint(kidx as u64));
            match resolve(t, schema) {
                ResolvedType::Struct(child_members) => {
                    let grand = enumerate_children(&c.atoms, &c.children, child_members, schema);
                    out.extend(encode_varint(grand.len() as u64));
                    for gc in &grand {
                        encode_element(gc, child_members, schema, codecs, out)?;
                    }
                }
                ResolvedType::Scalar(sc) => {
                    let value = scalar_value_text(c);
                    encode_scalar_payload(&sc, &value, &c.keyword, codecs, out)?;
                }
                ResolvedType::Flag => {
                    // No body.
                }
                ResolvedType::Unresolved | ResolvedType::KindMismatch => {
                    // Schema invalid; emit nothing rather than panic.
                }
            }
        }
        Element::AtomScalar { keyword, text } => {
            let kidx = keyword_index(parent_members, keyword, schema)
                .expect("atom-scalar keyword must resolve");
            out.extend(encode_varint(kidx as u64));
            match keyword_type(parent_members, keyword, schema).map(|t| resolve(t, schema)) {
                Some(ResolvedType::Scalar(sc)) => {
                    encode_scalar_payload(&sc, text, keyword, codecs, out)?;
                }
                _ => {
                    let bytes = text.as_bytes();
                    out.extend(encode_varint(bytes.len() as u64));
                    out.extend_from_slice(bytes);
                }
            }
        }
        Element::AtomFlag { keyword } => {
            let kidx = keyword_index(parent_members, keyword, schema)
                .expect("atom-flag keyword must resolve");
            out.extend(encode_varint(kidx as u64));
        }
        Element::DefaultScalar { keyword, value } => {
            let kidx = keyword_index(parent_members, keyword, schema)
                .expect("default keyword must resolve");
            out.extend(encode_varint(kidx as u64));
            match keyword_type(parent_members, keyword, schema).map(|t| resolve(t, schema)) {
                Some(ResolvedType::Scalar(sc)) => {
                    encode_scalar_payload(&sc, value, keyword, codecs, out)?;
                }
                _ => {
                    let bytes = value.as_bytes();
                    out.extend(encode_varint(bytes.len() as u64));
                    out.extend_from_slice(bytes);
                }
            }
        }
    }
    Ok(())
}

// ── Value hash (§3) ──────────────────────────────────────────────────────────

/// Compute the value hash (§3): 256-bit BLAKE3 of the document root encoding
/// alone, excluding magic number and schema signature.
///
/// Supports only schemas that declare no encodings; use
/// `value_hash_with_codecs` otherwise.
pub fn value_hash(doc: &Document, schema: &Schema) -> [u8; 32] {
    let bytes = encode_root(doc, schema);
    *blake3::hash(&bytes).as_bytes()
}

/// `value_hash` with a codec binding (TEL §21.7), for schemas that declare
/// encodings. Under codec laws C1–C4 the hash remains a function of the
/// semantic model and schema alone.
pub fn value_hash_with_codecs(
    doc: &Document,
    schema: &Schema,
    codec_binding: Option<&CodecBindingFn>,
) -> Result<[u8; 32], EncodeError> {
    let bytes = encode_root_with_codecs(doc, schema, codec_binding)?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

// ── Schema signature (§8) ────────────────────────────────────────────────────

/// Compute the schema signature for a schema with `component_hashes` ordered
/// `[base, layer_0, layer_1, …]`. This is the palimpsest at the BinTEL-pinned
/// parameters `(H, k_i, k_r) = (32, 4, 2)` per BinTEL §8.
pub fn schema_signature_from_hashes(component_hashes: &[[u8; 32]]) -> Vec<u8> {
    assert!(!component_hashes.is_empty(), "schema signature requires at least one component");
    let hashes: Vec<palimpsest::Hash> = component_hashes.iter()
        .map(|h| palimpsest::Hash::from(*h)).collect();
    let palimp = palimpsest::encode(&hashes, SIGNATURE_INITIAL_CADENCE, SIGNATURE_REGULAR_CADENCE);
    palimp.bytes().to_vec()
}

// ── File layout (§6) ─────────────────────────────────────────────────────────

/// Encode a complete BinTEL document in external-schema mode (§6.1):
/// magic number, schema signature, then the document root encoding.
///
/// `component_hashes` is the ordered sequence of component value hashes that
/// identify the composed schema. For a base schema with no layers, pass a
/// single-element slice containing the base schema's value hash.
pub fn encode_document_with_signature(
    doc: &Document,
    schema: &Schema,
    component_hashes: &[[u8; 32]],
) -> Vec<u8> {
    encode_document_with_signature_and_codecs(doc, schema, component_hashes, None)
        .expect("schema declares encodings; use encode_document_with_signature_and_codecs")
}

/// `encode_document_with_signature` with a codec binding (TEL §21.7), for
/// schemas that declare encodings.
pub fn encode_document_with_signature_and_codecs(
    doc: &Document,
    schema: &Schema,
    component_hashes: &[[u8; 32]],
    codec_binding: Option<&CodecBindingFn>,
) -> Result<Vec<u8>, EncodeError> {
    // §6.1: the document length counts everything after the length field, so
    // the body is built first and framed afterwards — one pass, no
    // self-referential fixed point.
    let mut body = Vec::new();
    let signature = schema_signature_from_hashes(component_hashes);
    body.extend(encode_varint(signature.len() as u64));
    body.extend_from_slice(&signature);
    body.extend(encode_root_with_codecs(doc, schema, codec_binding)?);

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend(encode_varint(body.len() as u64));
    out.extend_from_slice(&body);
    Ok(out)
}

// ── Schema-document hash helpers (§8.1) ──────────────────────────────────────

/// Compute the BinTEL value hash of a schema document's **base** component
/// (the schema document with all `layer` compounds removed), per §8.1 of the
/// BinTEL Specification. Used both when computing a schema's full signature
/// and when verifying a self-contained document's embedded schema.
pub fn schema_base_hash(schema_doc: &Document) -> [u8; 32] {
    let base_doc = Document {
        interpreter_directive: schema_doc.interpreter_directive.clone(),
        pragma: schema_doc.pragma.clone(),
        line_endings: schema_doc.line_endings,
        children: schema_doc.children.iter().map(|b| Block {
            comments: b.comments.clone(),
            tabulation: b.tabulation.clone(),
            compounds: b.compounds.iter()
                .filter(|c| c.keyword != "layer")
                .cloned().collect(),
            trailing_blank_lines: b.trailing_blank_lines,
        }).collect(),
    };
    value_hash(&base_doc, &builtin_tels())
}

/// Compute the BinTEL value hash of a single `layer` compound, per §8.1.
/// The layer's children are treated as the document root of a virtual schema
/// whose `document` Struct is the tels `Layer` Definition; the
/// Definition namespace is inherited from tels unchanged.
pub fn schema_layer_hash(layer_compound: &Compound) -> [u8; 32] {
    let layer_doc = Document {
        interpreter_directive: None,
        pragma: None,
        line_endings: LineEndings::LF,
        children: layer_compound.children.clone(),
    };
    let tel = builtin_tels();
    let layer_def = tel.records.iter().find(|d| d.name == "Layer")
        .expect("builtin tels must define the Layer record");
    let synth_schema = Schema {
        name: "tel-layer".to_string(),
        document: Struct {
            members: layer_def.members.clone(),
            validators: layer_def.validators.clone(),
        },
        layers: Vec::new(),
        sigil: tel.sigil,
        records: tel.records.clone(),
        scalars: tel.scalars.clone(),
        selects: tel.selects.clone(),
    };
    value_hash(&layer_doc, &synth_schema)
}

/// Compute the full composed signature (§8.2 palimpsest) of a schema
/// document: the base hash followed by each layer hash in source order.
/// Returns 33 bytes for a no-layer schema, `37 + 2·(n − 2)` bytes for an
/// `n`-component schema with `n ≥ 2`.
pub fn schema_full_signature(schema_doc: &Document) -> Vec<u8> {
    let mut component_hashes: Vec<[u8; 32]> = vec![schema_base_hash(schema_doc)];
    for block in &schema_doc.children {
        for c in &block.compounds {
            if c.keyword == "layer" {
                component_hashes.push(schema_layer_hash(c));
            }
        }
    }
    schema_signature_from_hashes(&component_hashes)
}

/// Return the ordered component hashes (base hash followed by each layer
/// hash in source order) for a schema document. Useful for callers that
/// want to populate a per-component library.
pub fn schema_component_hashes(schema_doc: &Document) -> Vec<[u8; 32]> {
    let mut out: Vec<[u8; 32]> = vec![schema_base_hash(schema_doc)];
    for block in &schema_doc.children {
        for c in &block.compounds {
            if c.keyword == "layer" {
                out.push(schema_layer_hash(c));
            }
        }
    }
    out
}

// ── Self-contained mode (§6.2) ───────────────────────────────────────────────

/// Result of decoding a BinTEL byte sequence in self-contained mode
/// (§6.2): the carried signature, the embedded schema (both as a TEL
/// document and as the composed `Schema`), and the decoded data document.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSelfContained {
    pub signature: Vec<u8>,
    /// The embedded schema as a TEL document (the bytes that were
    /// length-prefixed in §6.2 field 4, parsed under tels).
    pub schema_document: Document,
    /// The composed `Schema` obtained from the embedded schema document
    /// via `construct_schema` + `compose_schema`.
    pub schema: Schema,
    /// The decoded data document.
    pub document: Document,
    /// Where this document's continuation begins (§6.3); see
    /// [`Decoded::continuation`].
    pub continuation: usize,
}

/// Encode a complete BinTEL document in self-contained mode (§6.2).
///
/// - `doc` is the data document to encode as the outer document root.
/// - `schema_doc` is the schema as a TEL document; it is encoded as the
///   embedded schema body using `tels` (the schema-for-schemas) and
///   provides the §8.2 resolution-protocol step-0 lookup.
/// - `composed_schema` is the composed schema (base + layers merged) used
///   to encode `doc` itself.
/// - `component_hashes` is the ordered sequence of component value hashes
///   that identify `composed_schema`; the resulting signature MUST also be
///   the composed signature of `schema_doc`.
pub fn encode_document_self_contained(
    doc: &Document,
    schema_doc: &Document,
    composed_schema: &Schema,
    component_hashes: &[[u8; 32]],
) -> Vec<u8> {
    encode_document_self_contained_with_codecs(doc, schema_doc, composed_schema, component_hashes, None)
        .expect("schema declares encodings; use encode_document_self_contained_with_codecs")
}

/// `encode_document_self_contained` with a codec binding (TEL §21.7). The
/// embedded schema body is governed by `tels`, which declares no
/// encodings, so only the outer document root uses the binding.
pub fn encode_document_self_contained_with_codecs(
    doc: &Document,
    schema_doc: &Document,
    composed_schema: &Schema,
    component_hashes: &[[u8; 32]],
    codec_binding: Option<&CodecBindingFn>,
) -> Result<Vec<u8>, EncodeError> {
    let mut body = Vec::new();
    let signature = schema_signature_from_hashes(component_hashes);
    body.extend(encode_varint(signature.len() as u64));
    body.extend_from_slice(&signature);
    let schema_bytes = encode_root(schema_doc, &builtin_tels());
    body.extend(encode_varint(schema_bytes.len() as u64));
    body.extend_from_slice(&schema_bytes);
    body.extend(encode_root_with_codecs(doc, composed_schema, codec_binding)?);

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC_SELF_CONTAINED);
    out.extend(encode_varint(body.len() as u64));
    out.extend_from_slice(&body);
    Ok(out)
}

/// Encode a schema as a complete BinTEL document (external-schema mode)
/// under the `tels` axiom. The resulting bytes are a portable
/// representation of the schema — the BinTEL counterpart to TEL schema
/// source text — usable directly with `Resolver::add_bintel_to_library`.
///
/// `schema_doc` is the schema as a TEL document; it is encoded under
/// tels and the carried signature is tels's own signature
/// (because tels is the schema being used to type the bytes).
pub fn schema_to_bintel(schema_doc: &Document) -> Vec<u8> {
    let tel = builtin_tels();
    let tel_hash = crate::builtin_tels_value_hash();
    encode_document_with_signature(schema_doc, &tel, &[tel_hash])
}

/// Decode a BinTEL byte sequence in self-contained mode (§6.2). Returns
/// the carried signature, the embedded schema (both as a TEL document and
/// composed), and the decoded data document.
pub fn decode_document_self_contained(
    bytes: &[u8],
) -> Result<DecodedSelfContained, DecodeError> {
    decode_document_self_contained_with_codecs(bytes, None, false)
}

/// `decode_document_self_contained` with a codec binding (TEL §21.7).
/// The embedded schema body is governed by `tels`, which declares
/// no encodings, so the bootstrap never needs the binding; only the outer
/// data root does (↯/t.4.13/↯/t.4.14/↯/t.4.15 semantics as in
/// `decode_document_with_codecs`).
pub fn decode_document_self_contained_with_codecs(
    bytes: &[u8],
    codec_binding: Option<&CodecBindingFn>,
    check_canonical: bool,
) -> Result<DecodedSelfContained, DecodeError> {
    let codecs = CodecResolver::new(codec_binding);
    let mut cur = 0;

    // ↯/t.4.1: magic — self-contained mode only.
    if bytes.len() < MAGIC_SELF_CONTAINED.len() {
        return Err(DecodeError::new(BCode::B09, "magic number truncated"));
    }
    if bytes[0..MAGIC_SELF_CONTAINED.len()] != MAGIC_SELF_CONTAINED {
        let hint = if bytes[0..MAGIC.len()] == MAGIC {
            "; document is in external-schema mode (§6.1) — use `decode_document`"
        } else { "" };
        return Err(DecodeError::new(BCode::B01,
            format!("magic bytes were {:?}; expected {:?}{}",
                &bytes[0..MAGIC_SELF_CONTAINED.len()], MAGIC_SELF_CONTAINED, hint)));
    }
    cur += MAGIC_SELF_CONTAINED.len();

    // §6.2 field 2: the document length, exactly as in external mode.
    let (declared, len_consumed) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed document-length varint"))?;
    cur += len_consumed;
    let body_start = cur;
    let declared = declared as usize;
    if bytes.len() - cur < declared {
        return Err(DecodeError::new(BCode::B09,
            format!("document declares {} byte(s) but only {} remain",
                    declared, bytes.len() - cur)));
    }
    let end = body_start + declared;
    let bytes = &bytes[..end];

    let (signature, sig_consumed) = read_signature(&bytes[cur..])?;
    cur += sig_consumed;

    // Embedded schema body: length varint + schema bytes.
    let (schema_len, n) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed embedded-schema length varint"))?;
    cur += n;
    let schema_len = schema_len as usize;
    let schema_end = cur + schema_len;
    if schema_end > bytes.len() {
        return Err(DecodeError::new(BCode::B09, "embedded schema body truncated"));
    }
    let schema_bytes = &bytes[cur..schema_end];
    cur = schema_end;

    // Decode the embedded schema body as a bare document-root under the
    // hardwired tels axiom.
    let tel = builtin_tels();
    let (schema_blocks, schema_consumed) = decode_root_into_blocks(schema_bytes, &tel)
        .map_err(|e| DecodeError::new(BCode::B12,
            format!("embedded schema body does not decode under tels: {}", e.context)))?;
    if schema_consumed != schema_bytes.len() {
        return Err(DecodeError::new(BCode::B12,
            format!("embedded schema body has {} trailing bytes after decode",
                schema_bytes.len() - schema_consumed)));
    }
    let schema_document = Document {
        interpreter_directive: None,
        pragma: None,
        line_endings: LineEndings::LF,
        children: schema_blocks,
    };

    // Construct + compose the embedded schema.
    let staged = construct_schema(&schema_document);
    let (composed, compose_errors) = compose_schema(&staged);
    if !compose_errors.is_empty() {
        return Err(DecodeError::new(BCode::B12,
            format!("embedded schema fails composition: {} error(s)", compose_errors.len())));
    }

    // ↯/t.4.11: verify the recomputed signature matches the carried one.
    let recomputed = schema_full_signature(&schema_document);
    if recomputed != signature {
        return Err(DecodeError::new(BCode::B11,
            format!("embedded schema body's recomputed signature ({} bytes) does not equal the carried signature ({} bytes)",
                recomputed.len(), signature.len())));
    }

    // Decode the outer data document root under the composed schema.
    let (root_blocks, root_consumed) =
        decode_root_into_blocks_with_codecs(&bytes[cur..], &composed, &codecs, check_canonical)?;
    cur += root_consumed;

    // ↯/t.4.16: the declared and structural extents must agree exactly.
    if cur != end {
        return Err(DecodeError::new(BCode::B16,
            format!("declared length {} but the structure consumed {}",
                    declared, cur - body_start)));
    }

    Ok(DecodedSelfContained {
        continuation: end,
        signature,
        schema_document,
        schema: composed,
        document: Document {
            interpreter_directive: None,
            pragma: None,
            line_endings: LineEndings::LF,
            children: root_blocks,
        },
    })
}

/// A whole-document reader (§6.3): decodes exactly one document and requires
/// that nothing follow it. Use this when the contract is "these bytes are one
/// BinTEL document and nothing else"; use [`decode_document`] when a
/// continuation is expected, and [`decode_stream`] to consume a sequence.
///
/// Rejecting a non-empty continuation is a property of *this reader's*
/// contract, not of the bytes: the same input is a well-formed two-document
/// stream to [`decode_stream`].
pub fn decode_document_whole(bytes: &[u8], schema: &Schema) -> Result<Decoded, DecodeError> {
    let decoded = decode_document(bytes, schema)?;
    if decoded.continuation < bytes.len() {
        return Err(DecodeError::new(BCode::B08,
            format!("{} byte(s) remained after the document ended",
                    bytes.len() - decoded.continuation)));
    }
    Ok(decoded)
}

/// Decode a stream of external-schema BinTEL documents (§6.3), yielding each
/// in order. Defined by recursion on the continuation: decode one document,
/// then apply the same procedure to whatever follows, until nothing does.
///
/// Iteration stops at the first error, which is yielded. Every document in the
/// stream is typed by `schema`; a stream whose documents use different schemas
/// must be driven by the caller, reading each signature and selecting a schema
/// before decoding (which the continuation offset makes straightforward).
pub fn decode_stream<'a>(
    bytes: &'a [u8],
    schema: &'a Schema,
) -> impl Iterator<Item = Result<Decoded, DecodeError>> + 'a {
    let mut offset = 0usize;
    let mut stopped = false;
    std::iter::from_fn(move || {
        if stopped || offset >= bytes.len() {
            return None;
        }
        match decode_document(&bytes[offset..], schema) {
            Ok(d) => {
                // The continuation is relative to the slice just decoded.
                offset += d.continuation;
                Some(Ok(d))
            }
            Err(e) => {
                stopped = true;
                Some(Err(e))
            }
        }
    })
}

/// The extent of the BinTEL document beginning at `bytes[0]`, in bytes,
/// **without decoding it** — read the magic number and the declared length
/// (§6.1 field 2) and stop. This is the schema-independent framing operation
/// of §6.3: it resolves no schema, allocates nothing proportional to the
/// document, and costs the same for a one-byte body as for a gigabyte one.
///
/// Returns the total length including the magic number and the length field,
/// so `&bytes[document_extent(bytes)?..]` is the continuation.
pub fn document_extent(bytes: &[u8]) -> Result<usize, DecodeError> {
    if bytes.len() < MAGIC.len() {
        return Err(DecodeError::new(BCode::B09, "magic number truncated"));
    }
    let magic = &bytes[0..MAGIC.len()];
    if magic != MAGIC && magic != MAGIC_SELF_CONTAINED {
        return Err(DecodeError::new(BCode::B01,
            format!("magic bytes were {:?}; expected {:?} or {:?}",
                    magic, MAGIC, MAGIC_SELF_CONTAINED)));
    }
    let (declared, n) = decode_varint(&bytes[MAGIC.len()..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed document-length varint"))?;
    let total = MAGIC.len() + n + declared as usize;
    if total > bytes.len() {
        return Err(DecodeError::new(BCode::B09,
            format!("document declares a total extent of {} byte(s) but only {} are available",
                    total, bytes.len())));
    }
    Ok(total)
}

// ── Decoding (§§6–7) ─────────────────────────────────────────────────────────

/// Result of decoding a BinTEL byte sequence: the schema signature and the
/// reconstructed semantic content as a `Document` (with synthetic blocks and
/// inline atoms — no presentation-layer detail is recoverable from BinTEL).
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub signature: Vec<u8>,
    pub document: Document,
    /// The byte offset at which this document's **continuation** begins
    /// (§6.3): one past its last byte, as fixed by the declared document
    /// length. Bytes from here to the end of the input are the caller's to
    /// interpret — they may be a further BinTEL document, content in another
    /// format, or nothing at all.
    ///
    /// §6.3 requires a decoder to expose this. [`decode_stream`] is the
    /// recursion on it; a whole-document reader rejects a non-empty
    /// continuation as ↯/t.4.8 (see [`decode_document_whole`]).
    pub continuation: usize,
}

/// BinTEL decoder error code, corresponding to §10 of the BinTEL
/// Specification (↯/t.4.1–↯/t.4.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BCode {
    /// ↯/t.4.1: Magic number absent, or matching neither `B2 C4 B5 BB`
    /// (external-schema mode, §6.1) nor `B2 C4 B5 BC` (self-contained
    /// mode, §6.2).
    B01,
    /// ↯/t.4.2: A variable-length integer extends beyond end of input,
    /// exceeds the pinned range `[0, 2^64 − 1]`, or is not in the
    /// minimal form §4 requires.
    B02,
    /// ↯/t.4.3: Schema signature length is not `33` (n=1) and not
    /// `37 + 2·(n − 2)` for any `n ≥ 2`, or the XOR of every signature
    /// byte does not equal the BinTEL-pinned cadence byte `0x79`.
    B03,
    /// ↯/t.4.4: Schema signature does not decode against the available
    /// hash library. (Currently surfaced only when a layered signature
    /// cannot be reconstructed; non-applicable for single-component
    /// signatures, which a decoder can use verbatim.)
    B04,
    /// ↯/t.4.5: A keyword index read from the stream is out of range.
    B05,
    /// ↯/t.4.6: A Scalar value's byte length extends beyond end of input.
    B06,
    /// ↯/t.4.7: A Scalar value's UTF-8 bytes are not a valid UTF-8 sequence.
    B07,
    /// ↯/t.4.8: The document-root decoding procedure terminates with input
    /// bytes remaining (framing error).
    B08,
    /// ↯/t.4.9: The document-root decoding procedure requests bytes beyond
    /// end of input.
    B09,
    /// ↯/t.4.10: A `Reference` type appears in the schema but resolves to
    /// no `Definition` (schema configuration error).
    B10,
    /// ↯/t.4.11: In self-contained mode (§6.2), the composed signature
    /// recomputed from the embedded schema body does not equal the
    /// carried signature byte-for-byte.
    B11,
    /// ↯/t.4.12: In self-contained mode (§6.2), the embedded schema body
    /// does not decode as a valid TEL document under `tels`
    /// (structural error during bootstrap).
    B12,
    /// ↯/t.4.13: The composed schema declares an `encoding` for a Scalar but
    /// the decoder's codec binding (TEL §21.7) does not resolve that name.
    B13,
    /// ↯/t.4.14: An encoded Scalar's value bytes are rejected by the bound
    /// codec's decoder — the bytes are not the encoding of any accepted
    /// text (including corrupt or non-canonical bytes, per law C3).
    B14,
    /// ↯/t.4.15: The OPTIONAL re-encode verification of TEL §21.7 found
    /// `encode(decode(b)) ≠ b` — a canonicality violation indicating a
    /// non-conforming codec or corrupted input.
    B15,
    /// ↯/t.4.16: The document length declared in §6.1 field 2 / §6.2 field 2
    /// disagrees with the extent the structural decode consumed.
    B16,
}

impl BCode {
    /// The error's bolt code: BinTEL's errors are the family `↯/t.4` of
    /// TEL's, so that `B05` is `↯/t.4.5`.
    pub fn bolt(&self) -> String {
        let number: u32 = format!("{:?}", self)[1..].parse().expect("an error code is B and a number");
        format!("↯/t.4.{}", number)
    }

    pub fn description(&self) -> &'static str {
        match self {
            BCode::B01 => "magic number absent or invalid",
            BCode::B02 => "malformed variable-length integer",
            BCode::B03 => "invalid schema signature length",
            BCode::B04 => "schema signature does not decode against the library",
            BCode::B05 => "keyword index out of range",
            BCode::B06 => "scalar value length extends beyond end of input",
            BCode::B07 => "scalar value is not valid UTF-8",
            BCode::B08 => "framing error: input bytes remain after document root",
            BCode::B09 => "end of input reached mid-decode",
            BCode::B10 => "Reference type does not resolve to a Definition",
            BCode::B11 => "embedded schema body signature mismatch (self-contained mode)",
            BCode::B12 => "embedded schema body is not a valid tels document (self-contained mode)",
            BCode::B13 => "scalar's declared encoding is not resolved by the codec binding",
            BCode::B14 => "encoded scalar's value bytes rejected by the codec decoder",
            BCode::B15 => "codec canonicality check failed: re-encoded bytes differ",
            BCode::B16 => "declared document length disagrees with the structural extent",
        }
    }
}

/// A decoder error carries a ↯/t.4 code (§10 of the BinTEL Specification)
/// plus a human-readable context describing where in the stream the
/// error was detected.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeError {
    pub code: BCode,
    pub context: String,
}

impl DecodeError {
    pub fn new(code: BCode, context: impl Into<String>) -> Self {
        Self { code, context: context.into() }
    }
}

pub fn decode_document(bytes: &[u8], schema: &Schema) -> Result<Decoded, DecodeError> {
    decode_document_with_codecs(bytes, schema, None, false)
}

/// `decode_document` with a codec binding (TEL §21.7). Required whenever
/// the composed schema declares any encoding: an unresolved encoding name
/// is ↯/t.4.13 and a codec decode failure is ↯/t.4.14. With `check_canonical` set,
/// the OPTIONAL re-encode verification runs on every encoded scalar (↯/t.4.15
/// on mismatch) — a hardening measure with a per-value cost.
pub fn decode_document_with_codecs(
    bytes: &[u8],
    schema: &Schema,
    codec_binding: Option<&CodecBindingFn>,
    check_canonical: bool,
) -> Result<Decoded, DecodeError> {
    let codecs = CodecResolver::new(codec_binding);
    let mut cur = 0;

    // ↯/t.4.1: magic. This entry point handles external-schema mode only
    // (§6.1). Self-contained mode (§6.2) is handled by
    // `decode_document_self_contained`; encountering its magic here is
    // a usage error (the caller supplied an external-mode schema).
    if bytes.len() < MAGIC.len() {
        return Err(DecodeError::new(BCode::B09, "magic number truncated"));
    }
    if bytes[0..MAGIC.len()] != MAGIC {
        let hint = if bytes[0..MAGIC.len()] == MAGIC_SELF_CONTAINED {
            "; document is in self-contained mode (§6.2) — use `decode_document_self_contained`"
        } else { "" };
        return Err(DecodeError::new(BCode::B01,
            format!("magic bytes were {:?}; expected {:?}{}",
                &bytes[0..MAGIC.len()], MAGIC, hint)));
    }
    cur += MAGIC.len();

    // §6.1 field 2: the document length delimits this document without
    // reference to any schema, so the continuation is known before the body
    // is decoded at all.
    let (declared, len_consumed) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed document-length varint"))?;
    cur += len_consumed;
    let body_start = cur;
    let declared = declared as usize;
    if bytes.len() - cur < declared {
        return Err(DecodeError::new(BCode::B09,
            format!("document declares {} byte(s) but only {} remain",
                    declared, bytes.len() - cur)));
    }
    let end = body_start + declared;

    let (signature, sig_consumed) = read_signature(&bytes[cur..end])?;
    cur += sig_consumed;

    let (root_blocks, root_consumed) =
        decode_root_into_blocks_with_codecs(&bytes[cur..end], schema, &codecs, check_canonical)?;
    cur += root_consumed;

    // ↯/t.4.16: the declared and structural extents must agree exactly.
    if cur != end {
        return Err(DecodeError::new(BCode::B16,
            format!("declared length {} but the structure consumed {}",
                    declared, cur - body_start)));
    }

    Ok(Decoded {
        continuation: end,
        signature,
        document: Document {
            interpreter_directive: None,
            pragma: None,
            line_endings: LineEndings::LF,
            children: root_blocks,
        },
    })
}

/// Read the schema-signature field (length varint + signature bytes)
/// starting at `bytes[0]`. Returns the signature bytes and the number of
/// input bytes consumed. Performs the ↯/t.4.2 / ↯/t.4.3 / ↯/t.4.9 checks of §6 and §8.2.
fn read_signature(bytes: &[u8]) -> Result<(Vec<u8>, usize), DecodeError> {
    let mut cur = 0;
    let (sig_len, n) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed schema-signature length varint"))?;
    cur += n;
    let sig_len = sig_len as usize;
    // §6 / §8.2: length is 33 (n=1) or 37 + 2*(n − 2) for some n ≥ 2.
    let valid_length = sig_len == 33 || (sig_len >= 37 && (sig_len - 37) % 2 == 0);
    if !valid_length {
        return Err(DecodeError::new(BCode::B03,
            format!("signature length {} is not 33 (n=1) or 37 + 2·(n-2) for n ≥ 2", sig_len)));
    }
    let end = cur + sig_len;
    if end > bytes.len() {
        return Err(DecodeError::new(BCode::B09, "schema-signature bytes truncated"));
    }
    let signature = bytes[cur..end].to_vec();
    // §8.2 step 1: XOR of every signature byte must equal the
    // BinTEL-pinned cadence byte 0x79.
    let sig_xor = signature.iter().fold(0u8, |acc, &b| acc ^ b);
    if sig_xor != SIGNATURE_CADENCE_BYTE {
        return Err(DecodeError::new(BCode::B03,
            format!("signature byte XOR {:#04x} does not equal pinned cadence byte {:#04x}",
                sig_xor, SIGNATURE_CADENCE_BYTE)));
    }
    cur = end;
    Ok((signature, cur))
}

/// Decode a bare document-root encoding (§7.1) under the given composed
/// schema, returning the synthetic blocks and the number of input bytes
/// consumed. Used by both `decode_document` (external mode) and the
/// self-contained-mode decoder for the embedded schema body and the
/// outer data root.
fn decode_root_into_blocks(
    bytes: &[u8],
    schema: &Schema,
) -> Result<(Vec<Block>, usize), DecodeError> {
    decode_root_into_blocks_with_codecs(bytes, schema, &CodecResolver::new(None), false)
}

fn decode_root_into_blocks_with_codecs(
    bytes: &[u8],
    schema: &Schema,
    codecs: &CodecResolver,
    check_canonical: bool,
) -> Result<(Vec<Block>, usize), DecodeError> {
    let mut cur = 0;
    let (child_count, n) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed root child-count varint"))?;
    cur += n;
    let mut compounds = Vec::new();
    for _ in 0..child_count {
        let (comp, consumed) = decode_child(
            &bytes[cur..], &schema.document.members, schema, codecs, check_canonical)?;
        cur += consumed;
        compounds.push(comp);
    }
    let blocks = vec![Block {
        comments: Vec::new(),
        tabulation: None,
        compounds,
        trailing_blank_lines: 0,
    }];
    Ok((blocks, cur))
}

/// Decode a single child given the parent's member list. Returns the compound
/// and the number of bytes consumed.
fn decode_child(
    bytes: &[u8],
    parent_members: &[Member],
    schema: &Schema,
    codecs: &CodecResolver,
    check_canonical: bool,
) -> Result<(Compound, usize), DecodeError> {
    let mut cur = 0;
    let (kidx, n) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed keyword-index varint"))?;
    cur += n;
    let (keyword, t) = lookup_by_index(parent_members, kidx, schema)
        .ok_or_else(|| DecodeError::new(BCode::B05,
            format!("keyword index {} out of range [0, {})",
                kidx, keyword_count(parent_members, schema))))?;
    match resolve(t, schema) {
        ResolvedType::Struct(child_members) => {
            let (cc, n) = decode_varint(&bytes[cur..])
                .ok_or_else(|| DecodeError::new(BCode::B02,
                    format!("malformed child-count varint for `{}`", keyword)))?;
            cur += n;
            let mut grand = Vec::new();
            for _ in 0..cc {
                let (gc, used) = decode_child(
                    &bytes[cur..], child_members, schema, codecs, check_canonical)?;
                cur += used;
                grand.push(gc);
            }
            Ok((Compound {
                keyword: keyword.to_string(),
                atoms: Vec::new(),
                remark: None,
                children: vec![Block {
                    comments: Vec::new(),
                    tabulation: None,
                    compounds: grand,
                    trailing_blank_lines: 0,
                }],
            }, cur))
        }
        ResolvedType::Scalar(sc) => {
            let (vlen, n) = decode_varint(&bytes[cur..])
                .ok_or_else(|| DecodeError::new(BCode::B02,
                    format!("malformed value-length varint for `{}`", keyword)))?;
            cur += n;
            let end = cur + vlen as usize;
            if end > bytes.len() {
                return Err(DecodeError::new(BCode::B06,
                    format!("scalar `{}` length {} exceeds remaining {} bytes",
                        keyword, vlen, bytes.len() - cur)));
            }
            let value_bytes = &bytes[cur..end];
            let value = match &sc.encoding {
                None => std::str::from_utf8(value_bytes)
                    .map_err(|e| DecodeError::new(BCode::B07,
                        format!("scalar `{}`: {}", keyword, e)))?
                    .to_string(),
                Some(name) => {
                    let codec = codecs.resolve(name)
                        .ok_or_else(|| DecodeError::new(BCode::B13,
                            format!("scalar `{}` declares encoding `{}`, which the codec binding does not resolve",
                                keyword, name)))?;
                    let text = codec.decode(value_bytes)
                        .map_err(|e| DecodeError::new(BCode::B14,
                            format!("scalar `{}`: codec `{}` rejected value bytes: {}",
                                keyword, name, e)))?;
                    if check_canonical {
                        // OPTIONAL hardening (TEL §21.7 law C3).
                        let reencoded = codec.encode(&text).map_err(|d| {
                            DecodeError::new(BCode::B15,
                                format!("scalar `{}`: codec `{}` rejected its own decode output: {}",
                                    keyword, name, diagnostic_message(&d)))
                        })?;
                        if reencoded != value_bytes {
                            return Err(DecodeError::new(BCode::B15,
                                format!("scalar `{}`: re-encoded bytes differ from input (codec `{}` canonicality violation)",
                                    keyword, name)));
                        }
                    }
                    text
                }
            };
            cur = end;
            Ok((Compound {
                keyword: keyword.to_string(),
                atoms: vec![Atom::Inline { text: value, preceding_spaces: 1 }],
                remark: None,
                children: Vec::new(),
            }, cur))
        }
        ResolvedType::Flag => {
            Ok((Compound {
                keyword: keyword.to_string(),
                atoms: Vec::new(),
                remark: None,
                children: Vec::new(),
            }, cur))
        }
        ResolvedType::Unresolved | ResolvedType::KindMismatch => Err(DecodeError::new(BCode::B10,
            format!("Reference for `{}` does not resolve cleanly to a Definition of the expected kind", keyword))),
    }
}

fn keyword_count(members: &[Member], schema: &Schema) -> usize {
    members.iter().map(|m| match m {
        Member::Field(_) => 1,
        Member::SelectRef(s) => crate::resolve_select_ref(&s.reference, schema)
            .map(|vs| vs.len())
            .unwrap_or(0),
        Member::Exclude(_) => 0,
    }).sum()
}

fn lookup_by_index<'a>(members: &'a [Member], k: u64, schema: &'a Schema) -> Option<(&'a str, &'a Type)> {
    let mut idx: u64 = 0;
    for m in members {
        match m {
            Member::Field(f) => {
                if idx == k { return Some((f.keyword.as_str(), &f.r#type)); }
                idx += 1;
            }
            Member::SelectRef(s) => {
                if let Some(variants) = crate::resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        if idx == k { return Some((v.keyword.as_str(), &v.r#type)); }
                        idx += 1;
                    }
                }
            }
            Member::Exclude(_) => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_examples() {
        // Test vectors from spec/bintel.md §4.
        for (n, expected) in [
            (0u64, vec![0x00u8]),
            (1, vec![0x01]),
            (127, vec![0x7F]),
            (128, vec![0x80, 0x01]),
            (255, vec![0xFF, 0x01]),
            (16383, vec![0xFF, 0x7F]),
            (16384, vec![0x80, 0x80, 0x01]),
        ] {
            let enc = encode_varint(n);
            assert_eq!(enc, expected, "encoding {} should be {:?}, got {:?}", n, expected, enc);
            let (dec, used) = decode_varint(&enc).expect("decode should succeed");
            assert_eq!(dec, n);
            assert_eq!(used, expected.len());
        }
    }

    /// §4 pins the varint range and requires the minimal encoding, so that
    /// whether a byte sequence is a valid BinTEL document is a property of the
    /// bytes rather than of the decoder reading them.
    #[test]
    fn varint_rejects_overlong_encodings() {
        // `80 00` is zero written in two bytes; `00` is the minimal form.
        assert_eq!(decode_varint(&[0x00]), Some((0, 1)));
        assert_eq!(decode_varint(&[0x80, 0x00]), None);
        // `AC 82 00` is 300 written in three bytes; `AC 02` is minimal.
        assert_eq!(decode_varint(&[0xAC, 0x02]), Some((300, 2)));
        assert_eq!(decode_varint(&[0xAC, 0x82, 0x00]), None);
        // A minimal encoding may of course end in a byte whose value is small
        // but non-zero.
        assert_eq!(decode_varint(&[0x80, 0x01]), Some((128, 2)));
    }

    #[test]
    fn varint_range_is_pinned_to_64_bits() {
        // 2^64 − 1 is representable: nine 0x7F groups plus a final 0x01.
        let max = encode_varint(u64::MAX);
        assert_eq!(max.len(), 10);
        assert_eq!(decode_varint(&max), Some((u64::MAX, 10)));
        // A tenth byte above 0x01 would exceed 64 bits and is rejected rather
        // than silently truncated.
        let mut too_wide = max.clone();
        *too_wide.last_mut().unwrap() = 0x02;
        assert_eq!(decode_varint(&too_wide), None);
        // So is an eleventh byte.
        let mut eleven = vec![0xFFu8; 10];
        eleven.push(0x00);
        assert_eq!(decode_varint(&eleven), None);
    }

    #[test]
    fn varint_roundtrip_random() {
        for n in [0u64, 1, 7, 63, 64, 127, 128, 200, 500, 1234, 16_000, 16_384, 50_000, 1_000_000] {
            let enc = encode_varint(n);
            let (dec, used) = decode_varint(&enc).unwrap();
            assert_eq!(dec, n);
            assert_eq!(used, enc.len());
        }
    }

    #[test]
    fn encode_root_minimal_scalar() {
        // Schema: one required scalar field `name` (validator=string).
        let schema = crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(), validators: vec!["string".to_string()]}), default: None,
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        // Document containing `name Alice`.
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: vec![crate::Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![crate::Compound {
                    keyword: "name".to_string(),
                    atoms: vec![crate::Atom::Inline {
                        text: "Alice".to_string(), preceding_spaces: 1,
                    }],
                    remark: None, children: Vec::new(),
                }],
                trailing_blank_lines: 0,
            }],
        };
        // Expected:
        //   child_count: 1 (varint 0x01)
        //   keyword_index: 0 (varint 0x00)
        //   value_len: 5 (varint 0x05)
        //   value: "Alice" (0x41 0x6c 0x69 0x63 0x65)
        let expected = vec![0x01, 0x00, 0x05, b'A', b'l', b'i', b'c', b'e'];
        assert_eq!(encode_root(&doc, &schema), expected);
    }

    #[test]
    fn encode_root_with_default_substitution() {
        // Schema: required scalar `name` with default "anon" (Field.default).
        let schema = crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(), validators: vec!["string".to_string()] }),
                    default: Some("anon".to_string()),
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        // Document with no children (name absent — default applies).
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: Vec::new(),
        };
        let expected = vec![0x01, 0x00, 0x04, b'a', b'n', b'o', b'n'];
        assert_eq!(encode_root(&doc, &schema), expected);
    }

    #[test]
    fn encode_then_decode_minimal() {
        let schema = crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(), validators: vec!["string".to_string()]}), default: None,
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: vec![crate::Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![crate::Compound {
                    keyword: "name".to_string(),
                    atoms: vec![crate::Atom::Inline {
                        text: "Alice".to_string(), preceding_spaces: 1,
                    }],
                    remark: None, children: Vec::new(),
                }],
                trailing_blank_lines: 0,
            }],
        };
        let hash = value_hash(&doc, &schema);
        let bytes = encode_document_with_signature(&doc, &schema, &[hash]);
        let decoded = decode_document(&bytes, &schema).expect("decode should succeed");
        // BinTEL §6: n=1 signature is 33 bytes (32-byte hash + cadence trailer).
        assert_eq!(decoded.signature.len(), 33);
        // First 32 bytes are the value hash; trailing byte is the cadence
        // selector chosen so the XOR of every signature byte = 0x79.
        assert_eq!(&decoded.signature[..32], &hash[..]);
        assert_eq!(decoded.document.children.len(), 1);
        assert_eq!(decoded.document.children[0].compounds.len(), 1);
        assert_eq!(decoded.document.children[0].compounds[0].keyword, "name");
    }

    #[test]
    fn flag_encoding_is_keyword_only() {
        let schema = crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Loose, repeatable: Polarity::Default,
                    keyword: "ok".to_string(),
                    r#type: crate::Type::Flag, default: None,
                })], validators: Vec::new(),
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: vec![crate::Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![crate::Compound {
                    keyword: "ok".to_string(),
                    atoms: Vec::new(),
                    remark: None, children: Vec::new(),
                }],
                trailing_blank_lines: 0,
            }],
        };
        // child_count=1, keyword_index=0 (no value bytes for Flag).
        assert_eq!(encode_root(&doc, &schema), vec![0x01, 0x00]);
    }

    #[test]
    fn struct_encoding_round_trip() {
        // Schema: struct member `person` with two scalar children `first` and `last`.
        let schema = crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "person".to_string(),
                    r#type: crate::Type::Struct(crate::Struct {
                        members: vec![
                            crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                                required: Polarity::Default, repeatable: Polarity::Default,
                                keyword: "first".to_string(),
                                r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(), validators: vec!["string".to_string()]}), default: None,
                            }),
                            crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                                required: Polarity::Default, repeatable: Polarity::Default,
                                keyword: "last".to_string(),
                                r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(), validators: vec!["string".to_string()]}), default: None,
                            }),
                        ],
                        validators: vec![],
                    }), default: None,
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: vec![crate::Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![crate::Compound {
                    keyword: "person".to_string(),
                    atoms: Vec::new(), remark: None,
                    children: vec![crate::Block {
                        comments: Vec::new(), tabulation: None,
                        compounds: vec![
                            crate::Compound {
                                keyword: "first".to_string(),
                                atoms: vec![crate::Atom::Inline {
                                    text: "Alice".to_string(), preceding_spaces: 1,
                                }],
                                remark: None, children: Vec::new(),
                            },
                            crate::Compound {
                                keyword: "last".to_string(),
                                atoms: vec![crate::Atom::Inline {
                                    text: "Anderson".to_string(), preceding_spaces: 1,
                                }],
                                remark: None, children: Vec::new(),
                            },
                        ],
                        trailing_blank_lines: 0,
                    }],
                }],
                trailing_blank_lines: 0,
            }],
        };
        let hash = value_hash(&doc, &schema);
        let bytes = encode_document_with_signature(&doc, &schema, &[hash]);
        let decoded = decode_document(&bytes, &schema).expect("decode round-trips");
        assert_eq!(decoded.signature.len(), 33);
        assert_eq!(&decoded.signature[..32], &hash[..]);
        let person = &decoded.document.children[0].compounds[0];
        assert_eq!(person.keyword, "person");
        assert_eq!(person.children[0].compounds[0].keyword, "first");
        assert_eq!(person.children[0].compounds[1].keyword, "last");
    }

    #[test]
    fn schema_signature_single_component_carries_hash_and_cadence_byte() {
        // Per BinTEL §8.2, n=1 signature is the 32-byte value hash followed
        // by the trailing cadence byte (33 bytes total). The trailing byte
        // is chosen so that XOR(every byte) == 0x79.
        let h = [0xABu8; 32];
        let sig = schema_signature_from_hashes(&[h]);
        assert_eq!(sig.len(), 33);
        assert_eq!(&sig[..32], &h[..]);
        let xor = sig.iter().fold(0u8, |a, &b| a ^ b);
        assert_eq!(xor, SIGNATURE_CADENCE_BYTE);
    }

    #[test]
    fn schema_signature_two_components_length() {
        // Per BinTEL §8.2 with (H, k_i, k_r) = (32, 4, 2), n=2 signature is
        // 32 + 4 + 1 = 37 bytes.
        let sig = schema_signature_from_hashes(&[[0x11u8; 32], [0x22u8; 32]]);
        assert_eq!(sig.len(), 37);
        let xor = sig.iter().fold(0u8, |a, &b| a ^ b);
        assert_eq!(xor, SIGNATURE_CADENCE_BYTE);
    }

    #[test]
    fn schema_signature_three_components_length() {
        // n=3: 32 + 4 + 2 + 1 = 39 bytes.
        let sig = schema_signature_from_hashes(&[[0x11u8; 32], [0x22u8; 32], [0x33u8; 32]]);
        assert_eq!(sig.len(), 39);
    }

    fn trivial_schema() -> crate::Schema {
        crate::Schema {
            name: "demo".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(),
                        validators: vec!["string".to_string()]}), default: None,
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        }
    }

    #[test]
    fn bcode_b01_bad_magic() {
        let bytes = b"XXXX\x20\x01\x00";  // wrong magic
        let err = decode_document(bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B01,
                   "expected ↯/t.4.1 for bad magic, got: {:?}", err);
    }

    #[test]
    fn bcode_b03_bad_signature_length() {
        // Magic + a signature length of 35 (not a valid n=1 or n≥2 length
        // under the BinTEL-pinned parameters) → ↯/t.4.3. Varint for 35 is 0x23.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x23);                  // sig_len = 35
        bytes.extend_from_slice(&[0u8; 35]);
        bytes.push(0x00);                  // root child_count = 0
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B03,
                   "expected ↯/t.4.3 for sig_len 35, got: {:?}", err);
    }

    #[test]
    fn bcode_b03_bad_signature_cadence_xor() {
        // Magic + 33-byte signature whose byte-XOR is NOT 0x79 → ↯/t.4.3.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                  // sig_len = 33
        bytes.extend_from_slice(&[0u8; 33]); // XOR = 0x00, expected 0x79
        bytes.push(0x00);                  // root child_count = 0
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B03,
                   "expected ↯/t.4.3 for bad signature XOR, got: {:?}", err);
    }

    /// Build a hand-crafted 33-byte BinTEL signature whose first 32 bytes are
    /// `hash` and whose trailing byte is chosen so XOR(all 33 bytes) == 0x79.
    /// Frame a hand-built body (everything after §6.1 field 2 — signature,
    /// optional embedded schema, root) as a complete document, prepending the
    /// magic number and the declared document length.
    fn frame(magic: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = magic.to_vec();
        out.extend(encode_varint(body.len() as u64));
        out.extend_from_slice(body);
        out
    }

    fn craft_signature(hash: [u8; 32]) -> Vec<u8> {
        let body_xor = hash.iter().fold(0u8, |a, &b| a ^ b);
        let mut sig = hash.to_vec();
        sig.push(body_xor ^ SIGNATURE_CADENCE_BYTE);
        sig
    }

    #[test]
    fn bcode_b05_keyword_index_out_of_range() {
        // Magic + minimal 33-byte signature + root child_count=1 +
        // child keyword_index=99 (out of range).
        let mut body = vec![0x21];              // sig_len = 33
        body.extend_from_slice(&craft_signature([0u8; 32]));
        body.push(0x01);                        // root child_count = 1
        body.push(0x63);                        // keyword_index = 99 (varint)
        let bytes = frame(&MAGIC, &body);
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B05,
                   "expected ↯/t.4.5 for out-of-range keyword index, got: {:?}", err);
    }

    // ── §6.1 field 2 / §6.3: document length, continuation, streams ────────

    /// The length field counts the bytes *after* itself, so the full extent is
    /// `4 (magic) + len(varint) + declared` — no self-referential fixed point.
    #[test]
    fn document_length_frames_the_document() {
        let schema = trivial_schema();
        let doc = name_doc("Alice");
        let hash = value_hash(&doc, &schema);
        let bytes = encode_document_with_signature(&doc, &schema, &[hash]);

        assert_eq!(&bytes[0..4], &MAGIC);
        let (declared, n) = decode_varint(&bytes[4..]).unwrap();
        assert_eq!(4 + n + declared as usize, bytes.len(),
                   "declared length must count exactly the bytes following it");
        // Signature (1 + 33) plus the root encoding.
        let root = encode_root(&doc, &schema);
        assert_eq!(declared as usize, 1 + 33 + root.len());
    }

    /// ↯/t.4.16: the declared and structural extents must agree. A forged length
    /// must not be able to conceal bytes inside a document or expose bytes of
    /// the next one.
    #[test]
    fn b16_declared_length_disagrees_with_structure() {
        let schema = trivial_schema();
        let doc = name_doc("Alice");
        let hash = value_hash(&doc, &schema);
        let good = encode_document_with_signature(&doc, &schema, &[hash]);
        let (declared, n) = decode_varint(&good[4..]).unwrap();

        // Too short: the structure runs past the declared end.
        let mut short = good.clone();
        short.splice(4..4 + n, encode_varint(declared - 1));
        let err = decode_document(&short, &schema).unwrap_err();
        assert!(matches!(err.code, BCode::B16 | BCode::B06 | BCode::B09),
                "a short declared length must be caught, got {:?}", err);

        // Too long: the structure ends before the declared end, and the extra
        // byte is inside the document rather than in the continuation.
        let mut long = good.clone();
        long.splice(4..4 + n, encode_varint(declared + 1));
        long.push(0xAB);
        let err = decode_document(&long, &schema).unwrap_err();
        assert_eq!(err.code, BCode::B16,
                   "an over-long declared length is ↯/t.4.16, got {:?}", err);
    }

    /// §6.3: stream decoding is recursion on the continuation.
    #[test]
    fn continuation_is_exposed_and_stream_recurses() {
        let schema = trivial_schema();
        let names = ["Alice", "Bob", "Carol"];
        let mut stream = Vec::new();
        for n in names {
            let d = name_doc(n);
            let h = value_hash(&d, &schema);
            stream.extend(encode_document_with_signature(&d, &schema, &[h]));
        }

        // Single-document decoding yields the first document and points at
        // the rest.
        let first = decode_document(&stream, &schema).unwrap();
        assert!(first.continuation > 0 && first.continuation < stream.len());
        assert_eq!(scalar_text(&first.document), "Alice");

        // Applying the same procedure to the continuation yields the second.
        let second = decode_document(&stream[first.continuation..], &schema).unwrap();
        assert_eq!(scalar_text(&second.document), "Bob");

        // Which is exactly what the stream decoder does.
        let all: Vec<_> = decode_stream(&stream, &schema)
            .map(|r| scalar_text(&r.unwrap().document))
            .collect();
        assert_eq!(all, names);

        // An empty input is an empty stream, not an error.
        assert_eq!(decode_stream(&[], &schema).count(), 0);
    }

    /// §6.3: framing is schema-independent — a reader can delimit and skip
    /// documents while resolving no schema at all. `document_extent` reads
    /// only the magic number and the length.
    #[test]
    fn document_extent_needs_no_schema() {
        let schema = trivial_schema();
        let ext = {
            let d = name_doc("Alice");
            let h = value_hash(&d, &schema);
            encode_document_with_signature(&d, &schema, &[h])
        };
        let (schema_doc, composed, hashes) = small_schema();
        let selfc = encode_document_self_contained(
            &name_doc("Bob"), &schema_doc, &composed, &hashes);

        // A mixed stream: external mode, then self-contained mode.
        let mut stream = ext.clone();
        stream.extend_from_slice(&selfc);

        // Walk it with no schema in hand.
        let mut at = 0usize;
        let mut extents = Vec::new();
        while at < stream.len() {
            let n = document_extent(&stream[at..]).unwrap();
            extents.push(n);
            at += n;
        }
        assert_eq!(extents, vec![ext.len(), selfc.len()]);
        assert_eq!(at, stream.len());

        // And the self-contained document really was skippable without ever
        // resolving its embedded schema.
        let decoded = decode_document_self_contained(&stream[ext.len()..]).unwrap();
        assert_eq!(scalar_text(&decoded.document), "Bob");
        assert_eq!(decoded.continuation, selfc.len());
    }

    /// The keyword of the sole scalar child, for the stream tests above.
    fn scalar_text(doc: &Document) -> String {
        doc.children.iter()
            .flat_map(|b| b.compounds.iter())
            .map(crate::scalar_value_text)
            .next()
            .unwrap_or_default()
    }

    #[test]
    fn bcode_b08_trailing_bytes() {
        // A valid document plus a stray trailing byte. §6.3: this is an error
        // only for a *whole-document* reader; `decode_document` returns the
        // stray byte as the continuation.
        let schema = trivial_schema();
        let doc = crate::Document {
            interpreter_directive: None, pragma: None,
            line_endings: crate::LineEndings::LF,
            children: vec![crate::Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![crate::Compound {
                    keyword: "name".to_string(),
                    atoms: vec![crate::Atom::Inline {
                        text: "Alice".to_string(), preceding_spaces: 1,
                    }],
                    remark: None, children: Vec::new(),
                }],
                trailing_blank_lines: 0,
            }],
        };
        let hash = value_hash(&doc, &schema);
        let clean = encode_document_with_signature(&doc, &schema, &[hash]);
        let mut bytes = clean.clone();
        bytes.push(0xAB);  // stray byte

        // The whole-document reader rejects it.
        let err = decode_document_whole(&bytes, &schema).unwrap_err();
        assert_eq!(err.code, BCode::B08,
                   "expected ↯/t.4.8 for trailing bytes, got: {:?}", err);

        // The single-document reader hands it back as the continuation, and
        // the document it decoded is exactly the clean one.
        let decoded = decode_document(&bytes, &schema).unwrap();
        assert_eq!(decoded.continuation, clean.len());
        assert_eq!(&bytes[decoded.continuation..], &[0xAB]);

        // With nothing after it, the whole-document reader is happy.
        assert!(decode_document_whole(&clean, &schema).is_ok());
    }

    #[test]
    fn bcode_b09_truncated() {
        // Just the magic, no signature.
        let bytes = MAGIC.to_vec();
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        // §10 precedence: the truncation falls inside the signature-length
        // varint, so ↯/t.4.2 is required — not merely one of ↯/t.4.2/↯/t.4.9.
        assert_eq!(err.code, BCode::B02,
                   "expected ↯/t.4.2 for a truncation inside a varint, got: {:?}", err);
    }

    #[test]
    fn bcode_b02_malformed_varint() {
        // Magic + a varint byte with the continuation bit set but no
        // following byte → ↯/t.4.2 (malformed varint).
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x80);  // continuation bit set, but no follow-up byte
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B02,
                   "expected ↯/t.4.2 for malformed varint, got: {:?}", err);
    }

    #[test]
    fn bcode_b06_scalar_length_overruns_input() {
        // Magic + valid 33-byte signature + root child_count=1 +
        // keyword_index=0 (the only `name` field, Scalar string) +
        // value_length = 99, but no value bytes follow → ↯/t.4.6.
        let mut body = vec![0x21];              // sig_len = 33
        body.extend_from_slice(&craft_signature([0u8; 32]));
        body.push(0x01);                        // root child_count = 1
        body.push(0x00);                        // keyword_index = 0 (`name`)
        body.push(0x63);                        // value_length = 99 (varint)
        // No further bytes — claimed value length far exceeds remaining input.
        let bytes = frame(&MAGIC, &body);
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B06,
                   "expected ↯/t.4.6 for scalar overruns, got: {:?}", err);
    }

    #[test]
    fn bcode_b07_scalar_invalid_utf8() {
        // Magic + 33-byte sig + root child_count=1 + keyword_index=0 +
        // value_length=2 + two invalid UTF-8 bytes → ↯/t.4.7.
        let mut body = vec![0x21];              // sig_len = 33
        body.extend_from_slice(&craft_signature([0u8; 32]));
        body.push(0x01);                        // root child_count = 1
        body.push(0x00);                        // keyword_index = 0 (`name`)
        body.push(0x02);                        // value_length = 2
        body.push(0xC3);                        // lead byte of 2-byte UTF-8 seq
        body.push(0x28);                        // invalid continuation (not 10xxxxxx)
        let bytes = frame(&MAGIC, &body);
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B07,
                   "expected ↯/t.4.7 for invalid UTF-8, got: {:?}", err);
    }

    #[test]
    fn bcode_b10_reference_does_not_resolve() {
        // Construct a malformed schema whose document has a Reference Field
        // pointing at a Definition that doesn't exist. The decoder treats
        // this as a resolver-configuration error and emits ↯/t.4.10.
        let bad_schema = crate::Schema {
            name: "bad".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field { key: false, description: None, indexes: Vec::new(),
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "child".to_string(),
                    r#type: crate::Type::Reference("missing-definition".to_string()), default: None,
                })],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
        };
        // Encode (against a different schema; the decoder will reach the
        // dangling Reference). Simpler: hand-craft a minimal stream that
        // reaches the Reference resolution path.
        let mut body = vec![0x21];              // sig_len = 33
        body.extend_from_slice(&craft_signature([0u8; 32]));
        body.push(0x01);                        // root child_count = 1
        body.push(0x00);                        // keyword_index = 0 (child)
        let bytes = frame(&MAGIC, &body);
        // The decoder will look up `child`'s type, see Reference("missing-definition"),
        // attempt to resolve, fail, and emit ↯/t.4.10.
        let err = decode_document(&bytes, &bad_schema).unwrap_err();
        assert_eq!(err.code, BCode::B10,
                   "expected ↯/t.4.10 for dangling Reference, got: {:?}", err);
    }

    // ── Self-contained mode (§6.2) ───────────────────────────────────────

    /// Compose a small test schema with a single required `name` Scalar
    /// member, returning the schema document, the composed schema, and
    /// its component hashes. Used by the self-contained-mode tests below.
    fn small_schema() -> (Document, Schema, Vec<[u8; 32]>) {
        let schema_src = "tel 1.0\n\nname my-schema\n\ndocument\n  field name String\n";
        let parsed = crate::parse(schema_src);
        assert!(parsed.errors.is_empty(), "schema source must parse");
        let composed = crate::construct_schema(&parsed.document);
        let (composed, errors) = crate::compose_schema(&composed);
        assert!(errors.is_empty(), "schema must compose");
        let hashes = schema_component_hashes(&parsed.document);
        (parsed.document, composed, hashes)
    }

    /// Build a data document with one scalar child `name <text>`.
    fn name_doc(text: &str) -> Document {
        Document {
            interpreter_directive: None, pragma: None,
            line_endings: LineEndings::LF,
            children: vec![Block {
                comments: Vec::new(), tabulation: None,
                compounds: vec![Compound {
                    keyword: "name".to_string(),
                    atoms: vec![Atom::Inline {
                        text: text.to_string(), preceding_spaces: 1,
                    }],
                    remark: None, children: Vec::new(),
                }],
                trailing_blank_lines: 0,
            }],
        }
    }

    #[test]
    fn self_contained_round_trip() {
        let (schema_doc, composed, hashes) = small_schema();
        let data = name_doc("Alice");
        let bytes = encode_document_self_contained(&data, &schema_doc, &composed, &hashes);
        // Begins with the self-contained magic, not the external magic.
        assert_eq!(&bytes[0..4], &MAGIC_SELF_CONTAINED);
        let decoded = decode_document_self_contained(&bytes)
            .expect("self-contained decode should succeed");
        assert_eq!(decoded.signature.len(), 33);
        assert_eq!(decoded.document.children.len(), 1);
        assert_eq!(decoded.document.children[0].compounds.len(), 1);
        assert_eq!(decoded.document.children[0].compounds[0].keyword, "name");
        // The decoded schema document round-trips structurally.
        assert_eq!(decoded.schema.name, "my-schema");
    }

    #[test]
    fn self_contained_value_hash_invariant() {
        // §3 of the BinTEL Specification: the value hash is mode-invariant.
        // The same document encoded under the same composed schema in
        // external-schema mode (§6.1) and self-contained mode (§6.2) must
        // produce identical document-root bytes and therefore identical
        // value hashes.
        let (schema_doc, composed, hashes) = small_schema();
        let data = name_doc("Bob");
        let h_ext = value_hash(&data, &composed);
        // Encode self-contained; extract the doc-root portion and verify
        // it hashes to the same value as encoding under composed alone.
        let sc_bytes = encode_document_self_contained(&data, &schema_doc, &composed, &hashes);
        // Decode and recompute the value hash of the decoded document.
        let decoded = decode_document_self_contained(&sc_bytes).unwrap();
        let h_sc = value_hash(&decoded.document, &decoded.schema);
        assert_eq!(h_ext, h_sc,
                   "value hash must be identical between external-schema and self-contained modes");
    }

    #[test]
    fn self_contained_b11_on_tampered_schema() {
        let (schema_doc, composed, hashes) = small_schema();
        let data = name_doc("Charlie");
        let mut bytes = encode_document_self_contained(&data, &schema_doc, &composed, &hashes);
        // Find the embedded-schema-bytes region and flip a byte there. The
        // layout is: 4 magic + sig_len_varint + 33 sig + schema_len_varint +
        // schema_bytes + root. sig_len = 33 → encoded as 0x21 (single byte).
        // Locate the embedded schema body by walking the header rather than
        // hard-coding an offset: magic, document length (§6.2 field 2),
        // signature length, signature, then the schema-body length.
        let mut at = MAGIC_SELF_CONTAINED.len();
        let (_doc_len, n) = decode_varint(&bytes[at..]).unwrap();
        at += n;
        let (sig_len, n) = decode_varint(&bytes[at..]).unwrap();
        at += n + sig_len as usize;
        let (schema_len, n) = decode_varint(&bytes[at..]).unwrap();
        let schema_start = at + n;
        // Flip a byte in the middle of the embedded schema body.
        let mid = schema_start + (schema_len as usize) / 2;
        bytes[mid] ^= 0xFF;
        // Decoder should now report ↯/t.4.11 (recomputed signature mismatch) or
        // ↯/t.4.12 (decode of corrupted schema failed). Either is acceptable;
        // both are fatal and indicate the embedded body is corrupt.
        let err = decode_document_self_contained(&bytes).unwrap_err();
        assert!(matches!(err.code, BCode::B11 | BCode::B12),
                "expected ↯/t.4.11 or ↯/t.4.12 after tampering, got: {:?}", err);
    }

    #[test]
    fn decode_document_rejects_self_contained_magic() {
        // The external-mode decoder must not silently accept self-contained
        // bytes; it should emit ↯/t.4.1 with a hint pointing at the right entry.
        let (schema_doc, composed, hashes) = small_schema();
        let data = name_doc("Dana");
        let bytes = encode_document_self_contained(&data, &schema_doc, &composed, &hashes);
        let err = decode_document(&bytes, &composed).unwrap_err();
        assert_eq!(err.code, BCode::B01);
        assert!(err.context.contains("self-contained"),
                "↯/t.4.1 message should hint at self-contained mode; got: {}", err.context);
    }

    #[test]
    fn self_contained_decode_rejects_external_magic() {
        let (_doc, composed, hashes) = small_schema();
        let data = name_doc("Eve");
        let bytes = encode_document_with_signature(&data, &composed, &hashes);
        let err = decode_document_self_contained(&bytes).unwrap_err();
        assert_eq!(err.code, BCode::B01);
        assert!(err.context.contains("external"),
                "↯/t.4.1 message should hint at external mode; got: {}", err.context);
    }

    #[test]
    fn schema_to_bintel_round_trip_under_tels() {
        // schema_to_bintel produces a complete BinTEL document carrying
        // tels's signature; decoding it under tels yields back
        // the schema's semantic model.
        let (schema_doc, _composed, _hashes) = small_schema();
        let bytes = schema_to_bintel(&schema_doc);
        // External-mode magic with tels's signature.
        assert_eq!(&bytes[0..4], &MAGIC);
        let tel = crate::builtin_tels();
        let decoded = decode_document(&bytes, &tel)
            .expect("schema bytes must decode under tels");
        // Carried signature equals tels's full signature.
        let tel_sig = schema_signature_from_hashes(&[crate::builtin_tels_value_hash()]);
        assert_eq!(decoded.signature, tel_sig);
        // The decoded Document reconstructs into the same schema (name etc.).
        let reconstructed = crate::construct_schema(&decoded.document);
        assert_eq!(reconstructed.name, "my-schema");
    }

    // ── Encoded scalars (§7.1 / TEL §21.7) ──────────────────────────────

    use crate::Codec;
    use std::rc::Rc;

    /// Toy codec for tests: canonical decimal integer text ↔ BinTEL varint
    /// bytes. The decoder is deliberately lenient about overlong varints,
    /// which the ↯/t.4.15 canonicality test exploits.
    struct DecimalVarint;
    impl Codec for DecimalVarint {
        fn encode(&self, text: &str) -> Result<Vec<u8>, Diagnostic> {
            let canonical = !text.is_empty()
                && text.bytes().all(|b| b.is_ascii_digit())
                && (text.len() == 1 || !text.starts_with('0'));
            if !canonical {
                return Err(Diagnostic::Scalar {
                    message: "not a canonical decimal integer".to_string(),
                    span: None,
                });
            }
            let n: u64 = text.parse().map_err(|_| Diagnostic::Scalar {
                message: "integer too large".to_string(), span: None,
            })?;
            Ok(encode_varint(n))
        }
        fn decode(&self, bytes: &[u8]) -> Result<String, String> {
            // Deliberately *lenient*: this toy codec accepts overlong
            // encodings so that the ↯/t.4.15 canonicality check has something to
            // catch. It must not reuse BinTEL's framing `decode_varint`,
            // which §4 requires to be strict about minimality — a codec is
            // application-defined and independent of BinTEL's own framing.
            let mut value: u64 = 0;
            let mut shift: u32 = 0;
            let mut used = 0usize;
            let mut terminated = false;
            for &b in bytes {
                used += 1;
                if shift >= 64 { return Err("varint too wide".to_string()); }
                value |= ((b & 0x7F) as u64) << shift;
                if b & 0x80 == 0 { terminated = true; break; }
                shift += 7;
            }
            if !terminated { return Err("malformed varint".to_string()); }
            if used != bytes.len() { return Err("trailing bytes after varint".to_string()); }
            Ok(value.to_string())
        }
    }

    fn varint_binding(name: &str) -> Option<Rc<dyn Codec>> {
        if name == "decimal-varint" { Some(Rc::new(DecimalVarint)) } else { None }
    }

    fn amount_schema() -> Schema {
        let src = "tel 1.0\n\nname codec-demo\n\nscalar Amount\n  validate string\n  encoding decimal-varint\n\ndocument\n  field amount Amount\n";
        let parsed = crate::parse(src);
        assert!(parsed.errors.is_empty(), "schema must parse: {:?}", parsed.errors);
        crate::construct_schema(&parsed.document)
    }

    #[test]
    fn encoded_scalar_exact_bytes_and_roundtrip() {
        let schema = amount_schema();
        let doc = crate::parse("tel 1.0\n\namount 300\n").document;
        let root = encode_root_with_codecs(&doc, &schema, Some(&varint_binding)).unwrap();
        // child count 1, keyword index 0, byte length 2, varint(300) = AC 02.
        assert_eq!(root, vec![0x01, 0x00, 0x02, 0xAC, 0x02]);

        let full = encode_document_with_signature_and_codecs(
            &doc, &schema, &[[0u8; 32]], Some(&varint_binding)).unwrap();
        let decoded = decode_document_with_codecs(
            &full, &schema, Some(&varint_binding), true).unwrap();
        let amount = decoded.document.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "amount").unwrap();
        assert_eq!(crate::scalar_value_text(amount), "300");
    }

    #[test]
    fn atom_scalar_and_default_scalar_use_codec() {
        // `amount` is filled by an inline atom on `item`'s line (AtomScalar)
        // and `count` is a required defaulted scalar that is absent
        // (DefaultScalar); both must pass through the codec.
        let src = "tel 1.0\n\nname codec-demo\n\nrecord Item\n  field amount Amount\n  field count Count 7\n\nscalar Amount\n  validate string\n  encoding decimal-varint\n\nscalar Count\n  validate string\n  encoding decimal-varint\n\ndocument\n  field item Item\n";
        let parsed = crate::parse(src);
        assert!(parsed.errors.is_empty(), "schema must parse: {:?}", parsed.errors);
        let schema = crate::construct_schema(&parsed.document);
        let doc = crate::parse("tel 1.0\n\nitem 300\n").document;
        let root = encode_root_with_codecs(&doc, &schema, Some(&varint_binding)).unwrap();
        // root: child count 1, item kidx 0, item child count 2,
        //   amount kidx 0 + len 2 + AC 02 (atom-derived),
        //   count kidx 1 + len 1 + 07 (default-derived).
        assert_eq!(root, vec![0x01, 0x00, 0x02, 0x00, 0x02, 0xAC, 0x02, 0x01, 0x01, 0x07]);
    }

    #[test]
    fn value_hash_reflects_codec_bytes() {
        let schema = amount_schema();
        let doc = crate::parse("tel 1.0\n\namount 300\n").document;
        let hash = value_hash_with_codecs(&doc, &schema, Some(&varint_binding)).unwrap();
        let expected = *blake3::hash(&[0x01, 0x00, 0x02, 0xAC, 0x02]).as_bytes();
        assert_eq!(hash, expected);
    }

    #[test]
    fn encode_rejected_value_is_error() {
        let schema = amount_schema();
        let doc = crate::parse("tel 1.0\n\namount 007\n").document; // non-canonical decimal
        let err = encode_root_with_codecs(&doc, &schema, Some(&varint_binding)).unwrap_err();
        assert!(err.message.contains("rejected by codec"), "{:?}", err);
    }

    #[test]
    fn encode_missing_codec_is_error() {
        let schema = amount_schema();
        let doc = crate::parse("tel 1.0\n\namount 300\n").document;
        let err = encode_root_with_codecs(&doc, &schema, None).unwrap_err();
        assert!(err.message.contains("does not resolve"), "{:?}", err);
    }

    #[test]
    fn b13_unknown_codec_at_decode() {
        let schema = amount_schema();
        let doc = crate::parse("tel 1.0\n\namount 300\n").document;
        let full = encode_document_with_signature_and_codecs(
            &doc, &schema, &[[0u8; 32]], Some(&varint_binding)).unwrap();
        let err = decode_document(&full, &schema).unwrap_err();
        assert_eq!(err.code, BCode::B13);
    }

    #[test]
    fn b14_codec_decode_failure() {
        let schema = amount_schema();
        // Hand-craft: magic + signature + root with a truncated varint as
        // the value bytes (0x80 alone never terminates).
        let signature = schema_signature_from_hashes(&[[0u8; 32]]);
        let mut body = Vec::new();
        body.extend(encode_varint(signature.len() as u64));
        body.extend_from_slice(&signature);
        body.extend_from_slice(&[0x01, 0x00, 0x01, 0x80]); // 1 child, kidx 0, len 1, bad varint
        let bytes = frame(&MAGIC, &body);
        let err = decode_document_with_codecs(
            &bytes, &schema, Some(&varint_binding), false).unwrap_err();
        assert_eq!(err.code, BCode::B14);
    }

    #[test]
    fn b15_reencode_canonicality_mismatch() {
        let schema = amount_schema();
        // Overlong varint for 300: AC 82 00 — DecimalVarint's lenient
        // decoder accepts it, but re-encoding yields AC 02, so the
        // OPTIONAL canonicality check reports ↯/t.4.15.
        let signature = schema_signature_from_hashes(&[[0u8; 32]]);
        let mut body = Vec::new();
        body.extend(encode_varint(signature.len() as u64));
        body.extend_from_slice(&signature);
        body.extend_from_slice(&[0x01, 0x00, 0x03, 0xAC, 0x82, 0x00]);
        let bytes = frame(&MAGIC, &body);
        // Without the check the non-canonical bytes decode "successfully"…
        let lenient = decode_document_with_codecs(
            &bytes, &schema, Some(&varint_binding), false).unwrap();
        let amount = lenient.document.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "amount").unwrap();
        assert_eq!(crate::scalar_value_text(amount), "300");
        // …with it, the canonicality violation is detected.
        let err = decode_document_with_codecs(
            &bytes, &schema, Some(&varint_binding), true).unwrap_err();
        assert_eq!(err.code, BCode::B15);
    }

    #[test]
    fn self_contained_roundtrip_with_encoding() {
        let src = "tel 1.0\n\nname codec-demo\n\nscalar Amount\n  validate string\n  encoding decimal-varint\n\ndocument\n  field amount Amount\n";
        let schema_doc = crate::parse(src).document;
        let composed = crate::construct_schema(&schema_doc);
        let hashes = schema_component_hashes(&schema_doc);
        let doc = crate::parse("tel 1.0\n\namount 300\n").document;
        let bytes = encode_document_self_contained_with_codecs(
            &doc, &schema_doc, &composed, &hashes, Some(&varint_binding)).unwrap();
        let decoded = decode_document_self_contained_with_codecs(
            &bytes, Some(&varint_binding), true).unwrap();
        assert_eq!(decoded.schema.scalars[0].encoding, Some("decimal-varint".to_string()));
        let amount = decoded.document.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "amount").unwrap();
        assert_eq!(crate::scalar_value_text(amount), "300");
    }
}
