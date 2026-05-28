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
    Atom, Block, Compound, Document, Member, Schema, Type,
    resolve, ResolvedType, scalar_value_text,
};
#[cfg(test)]
use crate::Polarity;

/// The BinTEL magic number: four bytes `B2 C4 B5 BB`. In BASE-256 textual
/// form (§9 of the spec) these render as the four Greek letters `βτελ`
/// — `β` for "binary", `τελ` the Greek root for *tel*-. None of the
/// bytes is below `0x80`, so a BinTEL stream cannot be mistaken for the
/// start of an ASCII or UTF-8 text file.
pub const MAGIC: [u8; 4] = [0xB2, 0xC4, 0xB5, 0xBB];
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
        value |= chunk.checked_shl(shift)?;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift = shift.checked_add(7)?;
        if shift > 63 { return None; }
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
fn member_index(members: &[Member], keyword: &str, schema: &Schema) -> Option<usize> {
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
enum Element<'a> {
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
fn enumerate_children<'a>(
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
                    // Non-atom-assignable; type assignment would flag E303.
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
                    // E304 territory; skip the atom.
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
            // Unknown keyword would trigger E306 at type-assignment time.
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

/// Encode the document root (§7.1). The result is the bytes hashed for the
/// value hash (§3); it excludes the magic number and the schema signature.
pub fn encode_root(doc: &Document, schema: &Schema) -> Vec<u8> {
    let mut out = Vec::new();
    // The document root has no atoms (it's a virtual struct).
    let children = enumerate_children(&[], &doc.children, &schema.document.members, schema);
    out.extend(encode_varint(children.len() as u64));
    for child in &children {
        encode_element(child, &schema.document.members, schema, &mut out);
    }
    out
}

/// Encode one semantic-model element (§7.1) into `out`.
fn encode_element<'a>(
    elem: &Element<'a>,
    parent_members: &'a [Member],
    schema: &'a Schema,
    out: &mut Vec<u8>,
) {
    match elem {
        Element::Compound(c) => {
            let kidx = keyword_index(parent_members, &c.keyword, schema)
                .expect("keyword must resolve; type assignment should have caught E306");
            let t = keyword_type(parent_members, &c.keyword, schema)
                .expect("keyword must resolve");
            out.extend(encode_varint(kidx as u64));
            match resolve(t, schema) {
                ResolvedType::Struct(child_members) => {
                    let grand = enumerate_children(&c.atoms, &c.children, child_members, schema);
                    out.extend(encode_varint(grand.len() as u64));
                    for gc in &grand {
                        encode_element(gc, child_members, schema, out);
                    }
                }
                ResolvedType::Scalar(_) => {
                    let value = scalar_value_text(c);
                    let bytes = value.as_bytes();
                    out.extend(encode_varint(bytes.len() as u64));
                    out.extend_from_slice(bytes);
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
            let bytes = text.as_bytes();
            out.extend(encode_varint(bytes.len() as u64));
            out.extend_from_slice(bytes);
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
            let bytes = value.as_bytes();
            out.extend(encode_varint(bytes.len() as u64));
            out.extend_from_slice(bytes);
        }
    }
}

// ── Value hash (§3) ──────────────────────────────────────────────────────────

/// Compute the value hash (§3): 256-bit BLAKE3 of the document root encoding
/// alone, excluding magic number and schema signature.
pub fn value_hash(doc: &Document, schema: &Schema) -> [u8; 32] {
    let bytes = encode_root(doc, schema);
    *blake3::hash(&bytes).as_bytes()
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

/// Encode a complete BinTEL document: magic number, schema signature, then
/// the document root encoding.
///
/// `component_hashes` is the ordered sequence of component value hashes that
/// identify the composed schema. For a base schema with no layers, pass a
/// single-element slice containing the base schema's value hash.
pub fn encode_document_with_signature(
    doc: &Document,
    schema: &Schema,
    component_hashes: &[[u8; 32]],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    let signature = schema_signature_from_hashes(component_hashes);
    out.extend(encode_varint(signature.len() as u64));
    out.extend_from_slice(&signature);
    out.extend(encode_root(doc, schema));
    out
}

// ── Decoding (§§6–7) ─────────────────────────────────────────────────────────

/// Result of decoding a BinTEL byte sequence: the schema signature and the
/// reconstructed semantic content as a `Document` (with synthetic blocks and
/// inline atoms — no presentation-layer detail is recoverable from BinTEL).
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub signature: Vec<u8>,
    pub document: Document,
}

/// BinTEL decoder error code, corresponding to §10 of the BinTEL
/// Specification (B01–B10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BCode {
    /// B01: Magic number absent or does not match `B2 C4 B5 BB`.
    B01,
    /// B02: A variable-length integer extends beyond end of input, or
    /// its accumulator overflows.
    B02,
    /// B03: Schema signature length is not `33` (n=1) and not
    /// `37 + 2·(n − 2)` for any `n ≥ 2`, or the XOR of every signature
    /// byte does not equal the BinTEL-pinned cadence byte `0x79`.
    B03,
    /// B04: Schema signature does not decode against the available
    /// hash library. (Currently surfaced only when a layered signature
    /// cannot be reconstructed; non-applicable for single-component
    /// signatures, which a decoder can use verbatim.)
    B04,
    /// B05: A keyword index read from the stream is out of range.
    B05,
    /// B06: A Scalar value's byte length extends beyond end of input.
    B06,
    /// B07: A Scalar value's UTF-8 bytes are not a valid UTF-8 sequence.
    B07,
    /// B08: The document-root decoding procedure terminates with input
    /// bytes remaining (framing error).
    B08,
    /// B09: The document-root decoding procedure requests bytes beyond
    /// end of input.
    B09,
    /// B10: A `Reference` type appears in the schema but resolves to
    /// no `Definition` (schema configuration error).
    B10,
}

impl BCode {
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
        }
    }
}

/// A decoder error carries a B-code (§10 of the BinTEL Specification)
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
    let mut cur = 0;

    // B01: magic
    if bytes.len() < MAGIC.len() {
        return Err(DecodeError::new(BCode::B09, "magic number truncated"));
    }
    if bytes[0..MAGIC.len()] != MAGIC {
        return Err(DecodeError::new(BCode::B01,
            format!("magic bytes were {:?}; expected {:?}",
                &bytes[0..MAGIC.len()], MAGIC)));
    }
    cur += MAGIC.len();

    // B02 + B03: signature length and bytes
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

    // Document root: child_count + children
    let (child_count, n) = decode_varint(&bytes[cur..])
        .ok_or_else(|| DecodeError::new(BCode::B02, "malformed root child-count varint"))?;
    cur += n;
    let mut blocks = Vec::new();
    let mut compounds = Vec::new();
    for _ in 0..child_count {
        let (comp, consumed) = decode_child(&bytes[cur..], &schema.document.members, schema)?;
        cur += consumed;
        compounds.push(comp);
    }
    blocks.push(Block {
        comments: Vec::new(),
        tabulation: None,
        compounds,
        trailing_blank_lines: 0,
    });

    // B08: framing — every byte must be consumed.
    if cur < bytes.len() {
        return Err(DecodeError::new(BCode::B08,
            format!("{} byte(s) remained after document root", bytes.len() - cur)));
    }

    Ok(Decoded {
        signature,
        document: Document {
            interpreter_directive: None,
            pragma: None,
            line_endings: crate::LineEndings::LF,
            children: blocks,
        },
    })
}

/// Decode a single child given the parent's member list. Returns the compound
/// and the number of bytes consumed.
fn decode_child(
    bytes: &[u8],
    parent_members: &[Member],
    schema: &Schema,
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
                let (gc, used) = decode_child(&bytes[cur..], child_members, schema)?;
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
        ResolvedType::Scalar(_) => {
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
            let value = std::str::from_utf8(&bytes[cur..end])
                .map_err(|e| DecodeError::new(BCode::B07,
                    format!("scalar `{}`: {}", keyword, e)))?
                .to_string();
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
                members: vec![crate::Member::Field(crate::Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { validators: vec!["string".to_string()]}), default: None,
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
                members: vec![crate::Member::Field(crate::Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { validators: vec!["string".to_string()] }),
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
                members: vec![crate::Member::Field(crate::Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar { validators: vec!["string".to_string()]}), default: None,
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
                members: vec![crate::Member::Field(crate::Field {
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
                members: vec![crate::Member::Field(crate::Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "person".to_string(),
                    r#type: crate::Type::Struct(crate::Struct {
                        members: vec![
                            crate::Member::Field(crate::Field {
                                required: Polarity::Default, repeatable: Polarity::Default,
                                keyword: "first".to_string(),
                                r#type: crate::Type::Scalar(crate::Scalar { validators: vec!["string".to_string()]}), default: None,
                            }),
                            crate::Member::Field(crate::Field {
                                required: Polarity::Default, repeatable: Polarity::Default,
                                keyword: "last".to_string(),
                                r#type: crate::Type::Scalar(crate::Scalar { validators: vec!["string".to_string()]}), default: None,
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
                members: vec![crate::Member::Field(crate::Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "name".to_string(),
                    r#type: crate::Type::Scalar(crate::Scalar {
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
                   "expected B01 for bad magic, got: {:?}", err);
    }

    #[test]
    fn bcode_b03_bad_signature_length() {
        // Magic + a signature length of 35 (not a valid n=1 or n≥2 length
        // under the BinTEL-pinned parameters) → B03. Varint for 35 is 0x23.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x23);                  // sig_len = 35
        bytes.extend_from_slice(&[0u8; 35]);
        bytes.push(0x00);                  // root child_count = 0
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B03,
                   "expected B03 for sig_len 35, got: {:?}", err);
    }

    #[test]
    fn bcode_b03_bad_signature_cadence_xor() {
        // Magic + 33-byte signature whose byte-XOR is NOT 0x79 → B03.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                  // sig_len = 33
        bytes.extend_from_slice(&[0u8; 33]); // XOR = 0x00, expected 0x79
        bytes.push(0x00);                  // root child_count = 0
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B03,
                   "expected B03 for bad signature XOR, got: {:?}", err);
    }

    /// Build a hand-crafted 33-byte BinTEL signature whose first 32 bytes are
    /// `hash` and whose trailing byte is chosen so XOR(all 33 bytes) == 0x79.
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
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                       // sig_len = 33
        bytes.extend_from_slice(&craft_signature([0u8; 32]));
        bytes.push(0x01);                       // root child_count = 1
        bytes.push(0x63);                       // keyword_index = 99 (varint)
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B05,
                   "expected B05 for out-of-range keyword index, got: {:?}", err);
    }

    #[test]
    fn bcode_b08_trailing_bytes() {
        // Valid stream + a stray trailing byte.
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
        let mut bytes = encode_document_with_signature(&doc, &schema, &[hash]);
        bytes.push(0xAB);  // stray byte
        let err = decode_document(&bytes, &schema).unwrap_err();
        assert_eq!(err.code, BCode::B08,
                   "expected B08 for trailing bytes, got: {:?}", err);
    }

    #[test]
    fn bcode_b09_truncated() {
        // Just the magic, no signature.
        let bytes = MAGIC.to_vec();
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert!(matches!(err.code, BCode::B02 | BCode::B09),
                "expected B02/B09 for truncated signature, got: {:?}", err);
    }

    #[test]
    fn bcode_b02_malformed_varint() {
        // Magic + a varint byte with the continuation bit set but no
        // following byte → B02 (malformed varint).
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x80);  // continuation bit set, but no follow-up byte
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B02,
                   "expected B02 for malformed varint, got: {:?}", err);
    }

    #[test]
    fn bcode_b06_scalar_length_overruns_input() {
        // Magic + valid 33-byte signature + root child_count=1 +
        // keyword_index=0 (the only `name` field, Scalar string) +
        // value_length = 99, but no value bytes follow → B06.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                       // sig_len = 33
        bytes.extend_from_slice(&craft_signature([0u8; 32]));
        bytes.push(0x01);                       // root child_count = 1
        bytes.push(0x00);                       // keyword_index = 0 (`name`)
        bytes.push(0x63);                       // value_length = 99 (varint)
        // No further bytes — claimed value length far exceeds remaining input.
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B06,
                   "expected B06 for scalar overruns, got: {:?}", err);
    }

    #[test]
    fn bcode_b07_scalar_invalid_utf8() {
        // Magic + 33-byte sig + root child_count=1 + keyword_index=0 +
        // value_length=2 + two invalid UTF-8 bytes → B07.
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                       // sig_len = 33
        bytes.extend_from_slice(&craft_signature([0u8; 32]));
        bytes.push(0x01);                       // root child_count = 1
        bytes.push(0x00);                       // keyword_index = 0 (`name`)
        bytes.push(0x02);                       // value_length = 2
        bytes.push(0xC3);                       // lead byte of 2-byte UTF-8 seq
        bytes.push(0x28);                       // invalid continuation (not 10xxxxxx)
        let err = decode_document(&bytes, &trivial_schema()).unwrap_err();
        assert_eq!(err.code, BCode::B07,
                   "expected B07 for invalid UTF-8, got: {:?}", err);
    }

    #[test]
    fn bcode_b10_reference_does_not_resolve() {
        // Construct a malformed schema whose document has a Reference Field
        // pointing at a Definition that doesn't exist. The decoder treats
        // this as a resolver-configuration error and emits B10.
        let bad_schema = crate::Schema {
            name: "bad".to_string(),
            document: crate::Struct {
                members: vec![crate::Member::Field(crate::Field {
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
        let mut bytes = MAGIC.to_vec();
        bytes.push(0x21);                       // sig_len = 33
        bytes.extend_from_slice(&craft_signature([0u8; 32]));
        bytes.push(0x01);                       // root child_count = 1
        bytes.push(0x00);                       // keyword_index = 0 (child)
        // The decoder will look up `child`'s type, see Reference("missing-definition"),
        // attempt to resolve, fail, and emit B10.
        let err = decode_document(&bytes, &bad_schema).unwrap_err();
        assert_eq!(err.code, BCode::B10,
                   "expected B10 for dangling Reference, got: {:?}", err);
    }
}
