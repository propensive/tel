//! Canonical text serialization of a TEL document, per §22.3 of the
//! TEL Specification.
//!
//! `canonicalize(doc, schema)` produces a deterministic TEL text whose
//! semantic model equals the input's:
//!
//! - margin = 0, LF line endings, no interpreter directive
//! - pragma line included, carrying the bare BASE-256 schema signature
//!   (§22.3: any schema reference and layer selections are omitted)
//! - no comments, remarks, tabulations, or blank lines
//! - children emitted in **member order** (canonical, not source)
//! - Scalar values use the atom-form escalation of §22.2 (inline →
//!   source → literal), with the first valid form chosen
//!
//! The result satisfies properties P1, P3, and P4 of §22.4: parsing
//! and re-canonicalising produces byte-identical output, and the
//! BinTEL value hash is invariant under canonicalisation.

use crate::{
    Atom, Block, Compound, Document, Member, Schema, Type,
    resolve, ResolvedType, scalar_value_text,
};

/// Canonicalize a `Document` against a `Schema` (§22.3). The schema
/// fixes the member order for each Struct; the result is byte-equal
/// for any two documents with the same semantic model.
///
/// The signature emitted on the pragma line is the one the document carries.
/// §22.3 requires the canonical form to *always* carry a signature, computing
/// one when the document does not — but the composed signature is derived from
/// the schema *document*'s BinTEL encoding (§8 of the BinTEL Specification),
/// which a `Schema` model alone does not determine. A caller that resolved the
/// schema knows its signature and should pass it to
/// [`canonicalize_with_signature`]; this function is the convenience form for
/// a document that already carries one.
pub fn canonicalize(doc: &Document, schema: &Schema) -> String {
    let carried = doc.pragma.as_ref().and_then(|p| p.signature.clone());
    canonicalize_with_signature(doc, schema, carried.as_deref())
}

/// As [`canonicalize`], but emitting `signature` on the pragma line. Passing
/// the composed signature of the schema in force is what makes the output the
/// full canonical form of §22.3, which is always self-identifying.
pub fn canonicalize_with_signature(
    doc: &Document,
    schema: &Schema,
    signature: Option<&str>,
) -> String {
    let mut out = String::new();
    emit_pragma(doc, signature, &mut out);
    emit_struct_body(&[], &doc.children, &schema.document.members, schema, 0, &mut out);
    out
}

// ── Pragma ───────────────────────────────────────────────────────────────────

fn emit_pragma(doc: &Document, signature: Option<&str>, out: &mut String) {
    out.push_str("tel ");
    // §22.3: the document's pragma version, or 1.0 when it has no pragma.
    let (major, minor) = doc.pragma.as_ref()
        .map(|p| p.version)
        .unwrap_or((1, 0));
    out.push_str(&format!("{}.{}", major, minor));
    // §22.3: the bare BASE-256 schema signature alone — any schema reference
    // and layer selections are presentation-layer conveniences and are omitted,
    // and the sigil is never specified (the default `#` is used).
    if let Some(sig) = signature {
        out.push(' ');
        out.push_str(sig);
    }
    out.push('\n');
}

// ── Struct body emission ─────────────────────────────────────────────────────

/// Emit a Struct's body. `atoms` are the parent compound's inline atoms
/// (always empty at the document root). `blocks` are the compound
/// children. `members` is the parent's member list. `indent` is the
/// column at which compound children should appear.
fn emit_struct_body(
    atoms: &[Atom],
    blocks: &[Block],
    members: &[Member],
    schema: &Schema,
    indent: usize,
    out: &mut String,
) {
    // Enumerate the semantic children in canonical order (§7.2 of the BinTEL
    // Specification / §22.3): member order, atom-derived elements before
    // compound-derived ones. Every element is emitted as a child line, so an
    // inline atom (`field city String`, `index /country /city`) reappears
    // as an explicit `keyword value` line (`keyword city`, `path /country`).
    // Default substitutions are not materialized: an absent member stays
    // absent, and the default is re-applied on re-parse.
    for element in crate::bintel::enumerate_children(atoms, blocks, members, schema) {
        match element {
            crate::bintel::Element::Compound(c) => {
                if let Some(i) = member_index_for_keyword(members, &c.keyword, schema) {
                    emit_member_child(c, &members[i], schema, indent, out);
                }
            }
            crate::bintel::Element::AtomScalar { keyword, text } => {
                push_indent(out, indent);
                out.push_str(keyword);
                emit_scalar_payload(&text, indent, out);
            }
            crate::bintel::Element::AtomFlag { keyword } => {
                push_indent(out, indent);
                out.push_str(keyword);
                out.push('\n');
            }
            crate::bintel::Element::DefaultScalar { .. } => {}
        }
    }
}

fn emit_member_child(
    c: &Compound,
    member: &Member,
    schema: &Schema,
    indent: usize,
    out: &mut String,
) {
    let t: Type = match member {
        Member::Field(f) => f.r#type.clone(),
        Member::SelectRef(s) => {
            match crate::resolve_select_ref(&s.reference, schema)
                .and_then(|vs| vs.iter().find(|v| v.keyword == c.keyword))
            {
                Some(v) => v.r#type.clone(),
                None => return,
            }
        }
        Member::Exclude(_) => return,
    };
    emit_compound_line(c, &t, schema, indent, out);
}

fn emit_compound_line(
    c: &Compound,
    t: &Type,
    schema: &Schema,
    indent: usize,
    out: &mut String,
) {
    push_indent(out, indent);
    out.push_str(&c.keyword);

    match resolve(t, schema) {
        ResolvedType::Flag => {
            out.push('\n');
        }
        ResolvedType::Scalar(_) => {
            emit_scalar_payload(&scalar_value_text(c), indent, out);
        }
        ResolvedType::Struct(child_members) => {
            // Inline atoms on this compound's line MAY include the values
            // of an initial run of non-repeatable Scalar Fields, then
            // optionally either an all-Flag Select's present variants or
            // a single repeatable Scalar Field's values (§22.2 `construct`).
            // Every element — atom-derived or compound-derived — is emitted
            // as a compound child; this preserves the semantic model (P1)
            // and is canonical.
            out.push('\n');
            emit_struct_body(&c.atoms, &c.children, child_members, schema, indent + 2, out);
        }
        ResolvedType::Unresolved | ResolvedType::KindMismatch => {
            // Schema invalid; emit just the keyword line.
            out.push('\n');
        }
    }
}

// ── Scalar value emission with atom-form escalation (§22.2) ─────────────────

fn emit_scalar_payload(value: &str, indent: usize, out: &mut String) {
    if value.is_empty() {
        // An empty scalar has no atom text: emitting a leading space would
        // leave a trailing space on the keyword line (↯/t.1.8). The keyword line
        // alone re-parses to a compound with no atom, i.e. an empty value.
        out.push('\n');
        return;
    }
    // Canonical serialization always uses the default sigil `#` (§22.3).
    if can_inline(value, '#') {
        // A value containing a soft space must be separated from the keyword by
        // a hard-space run (two spaces) so the parser switches to hard-space
        // mode and keeps the space as content (§10.3); otherwise a single space
        // would split the value into two atoms. A space-free value uses one.
        out.push(' ');
        if value.contains(' ') { out.push(' '); }
        out.push_str(value);
        out.push('\n');
    } else if can_source(value) {
        out.push('\n');
        for line in value.split('\n') {
            push_indent(out, indent + 4);
            out.push_str(line);
            out.push('\n');
        }
    } else {
        // Literal atom: the closing delimiter line is byte-identical to the
        // opening one, at +6 indent (§15).
        out.push('\n');
        let delim = choose_literal_delim(value, indent + 6);
        push_indent(out, indent + 6);
        out.push_str(&delim);
        out.push('\n');
        for line in value.split('\n') {
            out.push_str(line);
            out.push('\n');
        }
        push_indent(out, indent + 6);
        out.push_str(&delim);
        out.push('\n');
    }
}

/// `inline-safe` predicate (§22.2). `sigil` is the document's sigil character.
/// The value is emitted as `keyword` + separator + value, with a two-space
/// (hard-space) separator when the value contains a space so its soft spaces
/// survive as content (§10.3) and a single-space separator otherwise.
pub(crate) fn can_inline(value: &str, sigil: char) -> bool {
    if value.is_empty() { return true; }
    if value.contains('\n') { return false; }
    if value.starts_with(' ') || value.ends_with(' ') { return false; }
    // A hard-space run (2+ consecutive spaces) would split the value into
    // separate atoms in the hard-space mode a spaced value relies on (§10.3).
    if value.contains("  ") { return false; }
    // Remark risk (§11.2): the value begins a phrase, so a leading sigil
    // immediately followed by a soft space would be parsed as a remark. An
    // internal soft space is content (hard-space mode), so an internal `<space>
    // <sigil>` is safe; only the phrase-start position matters.
    let mut chars = value.chars();
    if chars.next() == Some(sigil) && chars.next() == Some(' ') { return false; }
    true
}

/// `source-safe` predicate (§22.2). Under Convention A a source atom carries
/// one indented line per `\n`-separated segment and cannot represent an empty
/// line: a blank line neither begins nor continues an atom, and trailing blank
/// lines are dropped (§14). So every segment must be non-empty — which also
/// makes a leading or trailing newline (a leading/trailing empty line) require
/// a literal atom.
pub(crate) fn can_source(value: &str) -> bool {
    if value.is_empty() { return false; }
    // No empty line anywhere (covers leading `\n`, trailing `\n`, and `\n\n`).
    if value.split('\n').any(|l| l.is_empty()) { return false; }
    // Trailing spaces are stripped by the parser (§14).
    if value.split('\n').any(|l| l.ends_with(' ')) { return false; }
    // The first line's indentation is stripped from every captured line (§14),
    // so a leading space on the first line could not be recovered.
    if value.starts_with(' ') { return false; }
    true
}

/// Dash-extension (§22.3): lengthen `---` until the atom's delimiter line —
/// the opening indentation followed by the delimiter — no longer appears as a
/// line in the payload.
pub(crate) fn choose_literal_delim(payload: &str, indent_chars: usize) -> String {
    let indent = " ".repeat(indent_chars);
    let mut delim = "---".to_string();
    while payload.split('\n').any(|l| l.len() == indent.len() + delim.len()
        && l.starts_with(&indent) && l.ends_with(&delim)) {
        delim.push('-');
    }
    delim
}

/// True if `line` would match a closing delimiter line for `delim` at *some*
/// indentation — i.e. it consists of zero or more spaces followed exactly by
/// `delim`. A position-independent sufficient collision test (§23).
pub(crate) fn collides_at_any_indent(line: &str, delim: &str) -> bool {
    line.strip_suffix(delim).is_some_and(|prefix| prefix.chars().all(|c| c == ' '))
}

/// Position-independent dash-extension for callers that do not know the
/// indentation at which the atom will be serialized (§23 machine editing).
pub(crate) fn choose_literal_delim_any_indent(payload: &str) -> String {
    let mut delim = "---".to_string();
    while payload.split('\n').any(|l| collides_at_any_indent(l, &delim)) {
        delim.push('-');
    }
    delim
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn member_index_for_keyword(members: &[Member], keyword: &str, schema: &Schema) -> Option<usize> {
    for (i, m) in members.iter().enumerate() {
        match m {
            Member::Field(f) if f.keyword == keyword => return Some(i),
            Member::SelectRef(s) => {
                if let Some(variants) = crate::resolve_select_ref(&s.reference, schema) {
                    if variants.iter().any(|v| v.keyword == keyword) {
                        return Some(i);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n { out.push(' '); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parse, type_assign, Field, Member, Polarity, Scalar, Struct, Type};

    fn schema_string_field(keyword: &str, required: bool) -> Schema {
        Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![Member::Field(Field { key: false, description: None, indexes: Vec::new(),
                    required: if required { Polarity::Default } else { Polarity::Loose },
                    repeatable: Polarity::Default,
                    keyword: keyword.to_string(),
                    r#type: Type::Scalar(Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(),
                        validators: vec!["string".to_string()]}), default: None,
                })],
                validators: vec![],
            },
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
        }
    }

    /// Canonicalize a single `note` scalar with `value`, reparse, and return
    /// the recovered value text together with the atom form chosen. The form
    /// is re-derived from the value by canonicalization, so the input atom
    /// kind is irrelevant.
    fn round_trip_scalar(value: &str) -> (String, &'static str) {
        let schema = schema_string_field("note", true);
        let doc = Document {
            interpreter_directive: None,
            pragma: Some(crate::Pragma {
                version: (1, 0), reference: None, layers: Vec::new(),
                signature: None, sigil: None,
            }),
            line_endings: crate::LineEndings::LF,
            children: vec![Block {
                comments: vec![], tabulation: None, trailing_blank_lines: 0,
                compounds: vec![Compound {
                    keyword: "note".to_string(),
                    atoms: vec![Atom::Inline { text: value.to_string(), preceding_spaces: 1 }],
                    remark: None, children: vec![],
                }],
            }],
        };
        let canon = canonicalize(&doc, &schema);
        let re = parse(&canon).document;
        let c = re.children.iter().flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "note").expect("note present after round-trip");
        let form = match c.atoms.first() {
            None => "inline", // empty value: keyword line only, no atom
            Some(Atom::Inline { .. }) => "inline",
            Some(Atom::Source { .. }) => "source",
            Some(Atom::Literal { .. }) => "literal",
        };
        (scalar_value_text(c), form)
    }

    #[test]
    fn scalar_round_trips_exactly_across_all_forms() {
        // (value, expected canonical atom form). Every value MUST survive
        // parse(canonicalize(value)) byte-for-byte (property P1, §22.4).
        let cases: &[(&str, &str)] = &[
            ("",                "inline"),   // empty
            ("plain",           "inline"),
            ("a b",             "inline"),   // single soft space
            ("a  b",            "source"),   // hard space → not inline
            ("line1\nline2",    "source"),   // multi-line, no trailing newline
            ("a\nb\nc",         "source"),
            (" leading",        "literal"),  // leading space (single line)
            ("trailing ",       "literal"),  // trailing space (single line)
            (" a\nb",           "literal"),  // leading space on first line
            ("a\nb ",           "literal"),  // trailing space on a line
            ("a\nb\n",          "literal"),  // trailing newline → not source-safe
            ("\nab",            "literal"),  // leading newline
            ("a\n\nb",          "literal"),  // internal blank line → literal (canonical)
            ("a\n---\nb",       "source"),   // contains "---" but source-safe
            ("a\n---\nb\n",     "literal"),  // flush-left "---" is inert: closing line carries the opening indent
            ("a\n      ---\nb\n", "literal"), // line matching the indented delimiter line; dash-extension must dodge
            ("a#b",             "inline"),   // sigil mid-word → fine inline
            ("#b",              "inline"),   // leading sigil not followed by a space → atom, not remark
            ("a #b",            "inline"),   // internal space before sigil is content (hard-space mode)
            ("x #y",            "inline"),   // ditto
            ("# x",             "source"),   // leading sigil + soft space → remark → not inline
        ];
        for (value, want_form) in cases {
            let (got_value, got_form) = round_trip_scalar(value);
            assert_eq!(&got_value, value,
                       "value did not round-trip exactly: input {:?} -> {:?}", value, got_value);
            assert_eq!(got_form, *want_form,
                       "unexpected atom form for {:?}: got {}, want {}", value, got_form, want_form);
        }
    }

    #[test]
    fn canonical_pragma_only() {
        let schema = schema_string_field("name", true);
        let doc = parse("tel 1.0\n\nname Alice\n").document;
        let text = canonicalize(&doc, &schema);
        // The source document carries no signature, so none is emitted.
        assert!(text.starts_with("tel 1.0\n"), "got: {:?}", text);
        assert!(text.contains("name Alice\n"), "got: {:?}", text);
    }

    /// §22.3: the canonical form always carries a bare BASE-256 schema
    /// signature — never a reference, never layer selections, never a sigil.
    #[test]
    fn canonical_pragma_carries_the_bare_signature() {
        let schema = schema_string_field("name", true);
        // A pragma with a reference, a layer selection and a sigil: canonical
        // serialization must reduce all of it to the signature alone.
        let src = "tel 1.0 example.org/thing +extra ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠ %\n\nname Alice\n";
        let doc = parse(src).document;
        let sig = doc.pragma.as_ref().and_then(|p| p.signature.clone());
        assert!(sig.is_some(), "the 33-character phrase must classify as a signature");
        let text = canonicalize(&doc, &schema);
        let first = text.lines().next().unwrap();
        assert_eq!(first, format!("tel 1.0 {}", sig.unwrap()));
        assert!(!first.contains("example.org"), "reference must be omitted: {:?}", first);
        assert!(!first.contains('+'), "layer selections must be omitted: {:?}", first);
        assert!(!first.ends_with('%'), "the sigil must not be emitted: {:?}", first);
    }

    /// A caller that resolved the schema supplies the composed signature, so
    /// the canonical form is self-identifying even when the source document
    /// carried only a reference (§22.3).
    #[test]
    fn canonical_pragma_accepts_a_supplied_signature() {
        let schema = schema_string_field("name", true);
        let doc = parse("tel 1.0 example.org/thing\n\nname Alice\n").document;
        let supplied = "ḀḁЂЃĄąĆćȈȉЊḋЌḍĎďȐȑĒГДȕЖЗĘęȚțĜĝḞḟḠ";
        let text = canonicalize_with_signature(&doc, &schema, Some(supplied));
        assert_eq!(text.lines().next().unwrap(), format!("tel 1.0 {}", supplied));
    }

    #[test]
    fn canonical_round_trip_simple() {
        let schema = schema_string_field("name", true);
        let orig = parse("tel 1.0\n\nname Alice\n").document;
        let canon = canonicalize(&orig, &schema);
        // Re-parse the canonical form; the semantic content must match.
        let reparsed = parse(&canon).document;
        // Both documents should have one name field with value "Alice".
        let v1 = reparsed.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "name")
            .map(scalar_value_text);
        assert_eq!(v1.as_deref(), Some("Alice"));
        // Type-check the re-parsed doc against the schema.
        let ta = type_assign(&reparsed, &schema, None);
        assert!(ta.errors.is_empty(), "re-parsed canonical form must validate: {:?}", ta.errors);
    }

    #[test]
    fn canonical_round_trips_width_intern_and_index() {
        // tels 2.1.0's layout annotations are ordinary members, emitted in
        // member order: `width`/`intern` after `description` on a Scalar,
        // `index` (with its `path` atoms) last on a Field. Canonicalizing a
        // schema document under the built-in tels and re-parsing it must
        // construct the identical Schema, and canonicalization must be
        // idempotent.
        let tels = crate::builtin_tels();
        let src = concat!(
            "tel 1.0 specification.tel/tels:2.1.0\n\nname layout\n\n",
            "scalar Uuid\n  validate uuid\n  encoding uuid\n  width 16\n\n",
            "scalar Tag\n  intern\n\n",
            "record Address\n  field city String\n  field country String\n\n",
            "document\n  field id Uuid\n  field tag Tag repeatable\n    index /\n",
            "  field address Address repeatable\n    index /country /city\n    index .city\n",
        );
        let orig = parse(src).document;
        let ta = type_assign(&orig, &tels, None);
        assert!(ta.errors.is_empty(), "source must type-check against tels: {:?}", ta.errors);
        let canon = canonicalize(&orig, &tels);
        assert!(canon.contains("\n  width 16\n"), "width must be emitted as a child line:\n{}", canon);
        assert!(canon.contains("\n  intern\n"), "intern must be emitted as a child line:\n{}", canon);
        assert!(canon.contains("\n    index\n      path /country\n      path /city\n"),
                "index paths are emitted as `path` child lines in order:\n{}", canon);
        assert!(canon.contains("\n    index\n      path .city\n"),
                "a path's delimiter is preserved verbatim:\n{}", canon);
        let reparsed = parse(&canon);
        assert!(reparsed.errors.is_empty(), "canonical form must parse: {:?}\n{}", reparsed.errors, canon);
        let ta2 = type_assign(&reparsed.document, &tels, None);
        assert!(ta2.errors.is_empty(), "canonical form must type-check: {:?}", ta2.errors);
        let a = crate::construct_schema(&orig);
        let b = crate::construct_schema(&reparsed.document);
        assert_eq!(a, b, "canonicalization must preserve the constructed Schema");
        assert_eq!(a.scalars[0].width, Some(16));
        assert!(a.scalars[1].intern);
        match &a.document.members[2] {
            Member::Field(f) => assert_eq!(
                f.indexes,
                vec![crate::Index { paths: vec!["/country".into(), "/city".into()] },
                     crate::Index { paths: vec![".city".into()] }]),
            _ => panic!("expected a Field"),
        }
        assert!(crate::validate_schema(&a).is_empty());
        assert_eq!(canonicalize(&reparsed.document, &tels), canon, "canonicalization must be idempotent");
    }

    #[test]
    fn canonical_source_form_for_multiline_value() {
        let schema = schema_string_field("note", true);
        let src = "tel 1.0\n\nnote\n    first line\n    second line\n";
        let doc = parse(src).document;
        let canon = canonicalize(&doc, &schema);
        // The canonical form keeps the value multi-line.
        assert!(canon.contains("first line"));
        assert!(canon.contains("second line"));
        // Re-parsing canonical form preserves the semantic content
        // (both lines appear in the scalar value text).
        let reparsed = parse(&canon).document;
        let v = reparsed.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == "note")
            .map(scalar_value_text)
            .expect("note compound present");
        assert!(v.contains("first line"), "value missing first line, got: {:?}", v);
        assert!(v.contains("second line"), "value missing second line, got: {:?}", v);
    }

    #[test]
    fn canonical_determinism() {
        // Two parses of the same document must produce equal canonical forms.
        let schema = schema_string_field("name", true);
        let doc1 = parse("tel 1.0\n\nname Alice\n").document;
        let doc2 = parse("tel 1.0\n\nname Alice\n").document;
        let c1 = canonicalize(&doc1, &schema);
        let c2 = canonicalize(&doc2, &schema);
        assert_eq!(c1, c2, "canonicalization must be deterministic");
    }

    #[test]
    fn canonical_member_order_independent_of_source_order() {
        // Two documents with the same semantic content but different
        // source-order produce the same canonical form (canonical order
        // is by member, not source).
        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field { key: false, description: None, indexes: Vec::new(),
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "a".to_string(),
                        r#type: Type::Scalar(Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(),
                            validators: vec!["string".to_string()]}), default: None,
                    }),
                    Member::Field(Field { key: false, description: None, indexes: Vec::new(),
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "b".to_string(),
                        r#type: Type::Scalar(Scalar { encoding: None, width: None, intern: false, patterns: Vec::new(),
                            validators: vec!["string".to_string()]}), default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        // Two source orderings.
        let d1 = parse("tel 1.0\n\na 1\nb 2\n").document;
        let d2 = parse("tel 1.0\n\nb 2\na 1\n").document;
        let c1 = canonicalize(&d1, &schema);
        let c2 = canonicalize(&d2, &schema);
        assert_eq!(c1, c2, "canonical form must be member-order based, not source-order");
        // And the order should be a-before-b per the member order.
        let a_pos = c1.find("a 1").expect("a is present");
        let b_pos = c1.find("b 2").expect("b is present");
        assert!(a_pos < b_pos, "a should come before b in canonical form");
    }
}

