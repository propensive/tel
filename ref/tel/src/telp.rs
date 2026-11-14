//! TELP — the TEL path language, per `spec/telp.md`.
//!
//! A TELP addresses an element of the **semantic model** of a schema-typed TEL
//! document. Resolution walks the same element sequence that BinTEL encodes:
//! this module reuses [`crate::bintel::enumerate_children`], so "semantic
//! order" here is by construction the order §18.3 of the TEL Specification
//! defines and §7.2 of the BinTEL Specification serialises — member order,
//! atom-derived elements before compound-derived ones.
//!
//! TELP is a query mechanism: it performs no mutation, and its failures are
//! outcomes of this API rather than document errors, so they carry no bolt codes
//! (telp.md §7).

use crate::bintel::{enumerate_children, member_index, Element};
use crate::{atom_text, resolve as resolve_type, Compound, Document, Member, ResolvedType, Schema, Type};

/// The twenty-two delimiter characters of telp.md §3: the sigil-valid set of
/// TEL §6, minus `-` and `'` (which may occur inside a kebab-case identifier),
/// plus `+` (excluded from sigils only so that it can introduce a layer
/// selection on a pragma line — a constraint no TELP meets).
pub const DELIMITERS: &str = "!\"#$%&*+,./:;=?@\\^_`|~";

/// A parsed TELP. Two paths are equal iff their component sequences are equal:
/// the delimiter is not part of a path's identity (telp.md §3), so `.foo.bar`
/// and `/foo/bar` compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Telp {
    components: Vec<String>,
}

/// The failure kinds of telp.md §7. These are API outcomes, not document
/// errors; each carries the zero-based index of the component at which
/// resolution failed, as §7 recommends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Syntax(String),
    UnknownKeyword { component: usize, keyword: String },
    NonStructDescent { component: usize },
    AbsentMember { component: usize, keyword: String },
    IndexOutOfRange { component: usize, index: usize, len: usize },
    TypeNotKeyed { component: usize, keyword: String },
    KeyNotFound { component: usize, key: String },
}

/// A resolved element (telp.md §4), mirroring the semantic model of §18.2 of
/// the TEL Specification.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved<'a> {
    /// The document root.
    Root,
    /// A `Struct`-typed node. Always compound-realized: a `Struct`-typed
    /// element is never atom-realized (§20).
    Node(&'a Compound),
    /// A `Scalar`-typed element — its semantic text, whether written as an
    /// inline atom, carried by a compound, or supplied by a default.
    Value(String),
    /// A `Flag`-typed element — its keyword.
    Flag(String),
    /// One keyword's ordered occurrence sequence, possibly empty.
    Occurrences(Vec<Resolved<'a>>),
}

impl Telp {
    /// Parse a TELP. The delimiter is the path's first character, and the
    /// remainder is split at every further occurrence of it.
    pub fn parse(path: &str) -> Result<Telp, Failure> {
        let mut chars = path.chars();
        let delim = match chars.next() {
            Some(c) => c,
            None => return Err(Failure::Syntax("a TELP is a non-empty string".into())),
        };
        if !DELIMITERS.contains(delim) {
            return Err(Failure::Syntax(format!(
                "`{}` is not one of the twenty-two delimiter characters",
                delim
            )));
        }
        if path.contains('\n') || path.contains('\r') {
            return Err(Failure::Syntax("a TELP may not contain LF or CR".into()));
        }
        let rest: String = chars.collect();
        // A path consisting of the delimiter alone is the root path.
        if rest.is_empty() {
            return Ok(Telp { components: Vec::new() });
        }
        let components: Vec<String> = rest.split(delim).map(str::to_string).collect();
        if components.iter().any(String::is_empty) {
            return Err(Failure::Syntax(
                "components are non-empty: no doubled or trailing delimiter".into(),
            ));
        }
        Ok(Telp { components })
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    /// Render the path using `delim`, which MUST be a delimiter character not
    /// occurring in any component (telp.md §8).
    pub fn to_string_with(&self, delim: char) -> Option<String> {
        if !DELIMITERS.contains(delim) || self.components.iter().any(|c| c.contains(delim)) {
            return None;
        }
        let mut out = String::new();
        for c in &self.components {
            out.push(delim);
            out.push_str(c);
        }
        if out.is_empty() {
            out.push(delim);
        }
        Some(out)
    }
}

/// Everything needed to descend into a `Struct`-typed element.
struct Cursor<'a> {
    atoms: &'a [crate::Atom],
    blocks: &'a [crate::Block],
    members: &'a [Member],
}

/// The keyword an enumerated element fills.
fn element_keyword<'a>(e: &Element<'a>) -> &'a str {
    match e {
        Element::Compound(c) => &c.keyword,
        Element::AtomScalar { keyword, .. } => keyword,
        Element::AtomFlag { keyword } => keyword,
        Element::DefaultScalar { keyword, .. } => keyword,
    }
}

/// Convert one enumerated element into a resolved element, given the type the
/// schema ascribes to its keyword.
fn to_resolved<'a>(e: &Element<'a>, ty: Option<&Type>, schema: &Schema) -> Resolved<'a> {
    match e {
        Element::AtomScalar { text, .. } => Resolved::Value(text.clone()),
        Element::AtomFlag { keyword } => Resolved::Flag((*keyword).to_string()),
        Element::DefaultScalar { value, .. } => Resolved::Value((*value).to_string()),
        Element::Compound(c) => match ty.map(|t| resolve_type(t, schema)) {
            Some(ResolvedType::Struct(_)) => Resolved::Node(c),
            Some(ResolvedType::Flag) => Resolved::Flag(c.keyword.clone()),
            // A Scalar-typed compound's value is the text of its single atom,
            // or the empty string when it has none (§18.3 step 2).
            _ => Resolved::Value(
                c.atoms.first().map(atom_text).unwrap_or_default(),
            ),
        },
    }
}

/// The key value of a `Struct`-typed element: the semantic text of the `Value`
/// filling its key-flagged field, including a default-supplied one (§21.6).
fn key_value(c: &Compound, members: &[Member], schema: &Schema) -> Option<String> {
    let key_kw = members.iter().find_map(|m| match m {
        Member::Field(f) if f.key => Some(f.keyword.clone()),
        _ => None,
    })?;
    let children = enumerate_children(&c.atoms, &c.children, members, schema);
    children.iter().find(|e| element_keyword(e) == key_kw).map(|e| match e {
        Element::AtomScalar { text, .. } => text.clone(),
        Element::DefaultScalar { value, .. } => (*value).to_string(),
        Element::Compound(cc) => cc.atoms.first().map(atom_text).unwrap_or_default(),
        Element::AtomFlag { keyword } => (*keyword).to_string(),
    })
}

/// Does this Struct declare a key-flagged field (telp.md §4, key selector)?
fn has_key_field(members: &[Member]) -> bool {
    members.iter().any(|m| matches!(m, Member::Field(f) if f.key))
}

/// Resolve `path` against `doc` typed by `schema`, from the document root.
pub fn resolve<'a>(
    doc: &'a Document,
    schema: &'a Schema,
    path: &Telp,
) -> Result<Resolved<'a>, Failure> {
    let mut cursor = Cursor {
        atoms: &[],
        blocks: &doc.children,
        members: &schema.document.members,
    };
    let mut current = Resolved::Root;
    let mut pending: Option<(String, usize)> = None;

    let n = path.components.len();
    for (i, comp) in path.components.iter().enumerate() {
        let last = i + 1 == n;

        match pending.take() {
            // ── Selector step (telp.md §4 step 2) ──
            Some((kw, _)) => {
                let seq = occurrences(&cursor, schema, &kw);
                let chosen: usize = if !comp.is_empty() && comp.chars().all(|c| c.is_ascii_digit())
                {
                    // Index selector. An all-digit component is *always* an
                    // index, never a key value (§5, the shadowing rule).
                    let idx: usize = comp.parse().map_err(|_| Failure::IndexOutOfRange {
                        component: i,
                        index: usize::MAX,
                        len: seq.len(),
                    })?;
                    if idx >= seq.len() {
                        return Err(Failure::IndexOutOfRange {
                            component: i,
                            index: idx,
                            len: seq.len(),
                        });
                    }
                    idx
                } else {
                    // Key selector: the keyword's type must be a Struct with a
                    // key-flagged field.
                    let kw_members = keyword_struct_members(cursor.members, &kw, schema);
                    match kw_members {
                        Some(ms) if has_key_field(ms) => {
                            let mut found = None;
                            for (j, el) in seq.iter().enumerate() {
                                if let Resolved::Node(c) = el {
                                    if key_value(c, ms, schema).as_deref() == Some(comp.as_str()) {
                                        found = Some(j);
                                        break;
                                    }
                                }
                            }
                            match found {
                                Some(j) => j,
                                None => {
                                    return Err(Failure::KeyNotFound {
                                        component: i,
                                        key: comp.clone(),
                                    })
                                }
                            }
                        }
                        _ => {
                            return Err(Failure::TypeNotKeyed {
                                component: i,
                                keyword: kw.clone(),
                            })
                        }
                    }
                };

                current = seq.into_iter().nth(chosen).unwrap();
                if last {
                    return Ok(current);
                }
                cursor = advance(&current, cursor, schema);
            }

            // ── Keyword step (telp.md §4 step 1) ──
            None => {
                if !matches!(current, Resolved::Root | Resolved::Node(_)) {
                    return Err(Failure::NonStructDescent { component: i });
                }
                // An all-digit component matches no keyword: a kebab-case
                // identifier is never all-digits (§20.7).
                let mi = if comp.chars().all(|c| c.is_ascii_digit()) {
                    None
                } else {
                    member_index(cursor.members, comp, schema)
                };
                let mi = match mi {
                    Some(mi) => mi,
                    None => {
                        return Err(Failure::UnknownKeyword {
                            component: i,
                            keyword: comp.clone(),
                        })
                    }
                };

                let repeatable = match &cursor.members[mi] {
                    Member::Field(f) => f.repeatable.effective_repeatable(),
                    Member::SelectRef(s) => s.repeatable.effective_repeatable(),
                    Member::Exclude(_) => false,
                };

                if repeatable {
                    if last {
                        return Ok(Resolved::Occurrences(occurrences(
                            &cursor, schema, comp,
                        )));
                    }
                    pending = Some((comp.clone(), i));
                } else {
                    let mut seq = occurrences(&cursor, schema, comp);
                    if seq.is_empty() {
                        return Err(Failure::AbsentMember {
                            component: i,
                            keyword: comp.clone(),
                        });
                    }
                    current = seq.remove(0);
                    if last {
                        return Ok(current);
                    }
                    cursor = advance(&current, cursor, schema);
                }
            }
        }
    }

    // The path ended on a pending keyword only if it was consumed above; an
    // empty component list is the root path.
    Ok(current)
}

/// The members of the Struct that `keyword` denotes within `members`.
fn keyword_struct_members<'a>(
    members: &'a [Member],
    keyword: &str,
    schema: &'a Schema,
) -> Option<&'a [Member]> {
    let ty = crate::bintel::keyword_type(members, keyword, schema)?;
    match resolve_type(ty, schema) {
        ResolvedType::Struct(s) => Some(s),
        _ => None,
    }
}

/// Move the cursor into a resolved element. A `Struct`-typed element always
/// yields a new cursor; for anything else the cursor is left unchanged and the
/// next keyword step reports *non-struct descent* against its own component
/// index, which is where §4 step 1 places the check.
fn advance<'a>(
    current: &Resolved<'a>,
    cursor: Cursor<'a>,
    schema: &'a Schema,
) -> Cursor<'a> {
    match current {
        Resolved::Node(c) => match keyword_struct_members(cursor.members, &c.keyword, schema) {
            Some(ms) => Cursor { atoms: &c.atoms, blocks: &c.children, members: ms },
            None => cursor,
        },
        _ => cursor,
    }
}

/// The children of the cursor bearing `keyword`, in semantic order.
fn occurrences<'a>(cursor: &Cursor<'a>, schema: &'a Schema, keyword: &str) -> Vec<Resolved<'a>> {
    let ty = crate::bintel::keyword_type(cursor.members, keyword, schema);
    enumerate_children(cursor.atoms, cursor.blocks, cursor.members, schema)
        .iter()
        .filter(|e| element_keyword(e) == keyword)
        .map(|e| to_resolved(e, ty, schema))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{construct_schema, parse};

    /// The schema of telp.md §10, verbatim.
    const SCHEMA: &str = "\
tel 1.0

name menagerie

record Contact
  field name Identifier key
  field email String optional repeatable

record Toy
  field label String key

record Cat
  field name Identifier key
  field toy Toy optional repeatable

record Dog
  field name Identifier key

select Pet
  variant cat Cat
  variant dog Dog

document
  field owner Contact
  field contact Contact optional repeatable
  select Pet optional repeatable
";

    /// The document of telp.md §10, verbatim.
    const DOC: &str = "\
tel 1.0

owner amy
contact bea
  email bea@example.com
  email bea@example.org
contact chu

cat felix
  toy  ball of string
dog rex
cat tom
";

    fn fixture() -> (crate::Document, Schema) {
        let s = parse(SCHEMA);
        assert!(s.errors.is_empty(), "§10 schema must parse: {:?}", s.errors);
        let schema = construct_schema(&s.document);
        let d = parse(DOC);
        assert!(d.errors.is_empty(), "§10 document must parse: {:?}", d.errors);
        (d.document, schema)
    }

    fn go<'a>(doc: &'a crate::Document, schema: &'a Schema, p: &str) -> Result<Resolved<'a>, Failure> {
        resolve(doc, schema, &Telp::parse(p).expect("path must parse"))
    }

    // ── §3 Grammar ──────────────────────────────────────────────────────────

    #[test]
    fn delimiter_set_is_the_twenty_two_of_section_3() {
        assert_eq!(DELIMITERS.chars().count(), 22);
        // Derived as: sigil-valid (TEL §6) minus `-` and `'`, plus `+`.
        for c in "-'".chars() {
            assert!(!DELIMITERS.contains(c), "`{}` must not be a delimiter", c);
        }
        assert!(DELIMITERS.contains('+'), "`+` is a delimiter even though it is not sigil-valid");
    }

    #[test]
    fn root_path_is_the_delimiter_alone() {
        for d in ['/', '.', ':', '+'] {
            let p = Telp::parse(&d.to_string()).unwrap();
            assert!(p.components().is_empty());
        }
    }

    #[test]
    fn delimiter_is_not_part_of_path_identity() {
        assert_eq!(Telp::parse("/foo/bar").unwrap(), Telp::parse(".foo.bar").unwrap());
        assert_eq!(Telp::parse("|foo|bar").unwrap(), Telp::parse("~foo~bar").unwrap());
    }

    #[test]
    fn syntax_failures() {
        // empty path, non-delimiter first character, empty component, LF/CR
        assert!(matches!(Telp::parse(""), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("foo/bar"), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("-foo-bar"), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("/foo//bar"), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("/foo/"), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("/foo\nbar"), Err(Failure::Syntax(_))));
        assert!(matches!(Telp::parse("/foo\rbar"), Err(Failure::Syntax(_))));
    }

    #[test]
    fn components_may_contain_spaces() {
        let p = Telp::parse("/cat/felix/toy/ball of string").unwrap();
        assert_eq!(p.components().last().unwrap(), "ball of string");
    }

    // ── §10 worked examples, row by row ─────────────────────────────────────

    #[test]
    fn section_10_example_table() {
        let (doc, schema) = fixture();

        // `/` — the document root (the context element)
        assert_eq!(go(&doc, &schema, "/").unwrap(), Resolved::Root);

        // `/owner` — non-repeatable, so no selector
        match go(&doc, &schema, "/owner").unwrap() {
            Resolved::Node(c) => assert_eq!(c.keyword, "owner"),
            other => panic!("expected a Node, got {:?}", other),
        }

        // `/owner/name` — the Value `amy`
        assert_eq!(go(&doc, &schema, "/owner/name").unwrap(), Resolved::Value("amy".into()));

        // `/contact` — the occurrence sequence of both `contact` elements
        match go(&doc, &schema, "/contact").unwrap() {
            Resolved::Occurrences(v) => assert_eq!(v.len(), 2),
            other => panic!("expected an occurrence sequence, got {:?}", other),
        }

        // `/contact/bea` — the first `contact`, by key
        // `/contact/1`   — the second `contact` (`chu`), by index
        let bea = go(&doc, &schema, "/contact/bea").unwrap();
        let chu = go(&doc, &schema, "/contact/1").unwrap();
        assert_ne!(bea, chu);
        assert_eq!(go(&doc, &schema, "/contact/0").unwrap(), bea);
        assert_eq!(go(&doc, &schema, "/contact/chu").unwrap(), chu);

        // `/contact/bea/email/1` — `email` has no key, so index-only
        assert_eq!(
            go(&doc, &schema, "/contact/bea/email/1").unwrap(),
            Resolved::Value("bea@example.org".into()));
        assert_eq!(
            go(&doc, &schema, "/contact/bea/email/0").unwrap(),
            Resolved::Value("bea@example.com".into()));

        // `/cat/tom` and `/cat/1` are the same element: `dog rex` between the
        // two cats does not affect the `cat` sequence (§6).
        let tom = go(&doc, &schema, "/cat/tom").unwrap();
        assert_eq!(go(&doc, &schema, "/cat/1").unwrap(), tom);

        // `/dog/0/name` — the Value `rex`
        assert_eq!(go(&doc, &schema, "/dog/0/name").unwrap(), Resolved::Value("rex".into()));

        // `/pet` — unknown keyword: `pet` is a Select *name*, not a keyword
        assert!(matches!(go(&doc, &schema, "/pet"),
                         Err(Failure::UnknownKeyword { component: 0, .. })));

        // `/cat/felix/toy/0` and `/cat/felix/toy/ball of string` — same toy
        let by_index = go(&doc, &schema, "/cat/felix/toy/0").unwrap();
        let by_key = go(&doc, &schema, "/cat/felix/toy/ball of string").unwrap();
        assert_eq!(by_index, by_key);
    }

    // ── §5 The shadowing rule ───────────────────────────────────────────────

    #[test]
    fn all_digit_selector_is_always_an_index() {
        let (doc, schema) = fixture();
        // Only two contacts exist, so index 7 is out of range — the path is
        // never read as the key value `007`.
        match go(&doc, &schema, "/contact/007") {
            Err(Failure::IndexOutOfRange { index, len, .. }) => {
                assert_eq!((index, len), (7, 2));
            }
            other => panic!("expected IndexOutOfRange, got {:?}", other),
        }
    }

    // ── §6 Selector scope ───────────────────────────────────────────────────

    #[test]
    fn index_counts_same_keyword_occurrences_only() {
        let (doc, schema) = fixture();
        // Two cats and one dog fill the same Select member. Cat indices are 0
        // and 1 regardless of the interleaved dog; the dog's index is 0.
        assert!(go(&doc, &schema, "/cat/0").is_ok());
        assert!(go(&doc, &schema, "/cat/1").is_ok());
        assert!(matches!(go(&doc, &schema, "/cat/2"),
                         Err(Failure::IndexOutOfRange { .. })));
        assert!(go(&doc, &schema, "/dog/0").is_ok());
        assert!(matches!(go(&doc, &schema, "/dog/1"),
                         Err(Failure::IndexOutOfRange { .. })));
    }

    // ── §7 Failures ─────────────────────────────────────────────────────────

    #[test]
    fn failure_kinds() {
        let (doc, schema) = fixture();

        // unknown keyword
        assert!(matches!(go(&doc, &schema, "/nope"),
                         Err(Failure::UnknownKeyword { component: 0, .. })));

        // non-struct descent: below a Value
        assert!(matches!(go(&doc, &schema, "/owner/name/more"),
                         Err(Failure::NonStructDescent { component: 2 })));

        // index out of range
        assert!(matches!(go(&doc, &schema, "/contact/9"),
                         Err(Failure::IndexOutOfRange { index: 9, len: 2, .. })));

        // type not keyed: `email` is a String, not a keyed Struct
        assert!(matches!(go(&doc, &schema, "/contact/bea/email/nope"),
                         Err(Failure::TypeNotKeyed { component: 3, .. })));

        // key not found
        assert!(matches!(go(&doc, &schema, "/contact/nobody"),
                         Err(Failure::KeyNotFound { component: 1, .. })));
    }

    #[test]
    fn absent_optional_member_fails() {
        // `toy` is optional and repeatable; a cat with no toy yields an empty
        // occurrence sequence rather than a failure.
        let (doc, schema) = fixture();
        match go(&doc, &schema, "/cat/tom/toy").unwrap() {
            Resolved::Occurrences(v) => assert!(v.is_empty()),
            other => panic!("expected an empty occurrence sequence, got {:?}", other),
        }
    }

    // ── §8 Addressability limits ────────────────────────────────────────────

    #[test]
    fn delimiter_switch_reaches_a_key_containing_slash_and_dot() {
        // telp.md §8: a key value containing a candidate delimiter is
        // unaddressable *under that delimiter*; another delimiter works.
        let schema_src = SCHEMA;
        let doc_src = "\
tel 1.0

owner amy
contact a.b/c
  email x@example.com
";
        let s = parse(schema_src);
        let schema = construct_schema(&s.document);
        let d = parse(doc_src);
        assert!(d.errors.is_empty());

        // Under `/` or `.` the key cannot be written as one component.
        let under_slash = Telp::parse("/contact/a.b/c/email/0").unwrap();
        assert_eq!(under_slash.components(), ["contact", "a.b", "c", "email", "0"]);

        // Under `:` it can.
        let under_colon = Telp::parse(":contact:a.b/c:email:0").unwrap();
        assert_eq!(under_colon.components(), ["contact", "a.b/c", "email", "0"]);
        assert_eq!(resolve(&d.document, &schema, &under_colon).unwrap(),
                   Resolved::Value("x@example.com".into()));
    }

    #[test]
    fn rendering_requires_a_delimiter_absent_from_every_component() {
        let p = Telp::parse(":contact:a.b/c:email:0").unwrap();
        assert_eq!(p.to_string_with('/'), None, "`/` occurs in a component");
        assert_eq!(p.to_string_with('.'), None, "`.` occurs in a component");
        assert_eq!(p.to_string_with(':').unwrap(), ":contact:a.b/c:email:0");
    }

    // ── §4: default-supplied key values (§21.6) ─────────────────────────────

    #[test]
    fn key_selection_sees_a_default_supplied_key_value() {
        let schema_src = "\
tel 1.0

name defaulted

record Item
  field id Identifier key anon
  field note String optional

document
  field item Item optional repeatable
";
        let doc_src = "\
tel 1.0

item
  note first
";
        let s = parse(schema_src);
        assert!(s.errors.is_empty(), "{:?}", s.errors);
        let schema = construct_schema(&s.document);
        let d = parse(doc_src);
        assert!(d.errors.is_empty(), "{:?}", d.errors);
        // The `id` field is elided, so its key value is the default `anon`.
        let p = Telp::parse("/item/anon/note").unwrap();
        assert_eq!(resolve(&d.document, &schema, &p).unwrap(),
                   Resolved::Value("first".into()));
    }
}
