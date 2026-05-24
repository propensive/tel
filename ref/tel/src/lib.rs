//! TEL reference implementation.
//!
//! This crate implements the [TEL Specification](../../../spec/tel.md):
//!
//! - **Parser** (`parse`) producing the presentation model of §17: `Document` ⊃ `Block` ⊃
//!   `Compound` with attached `Atom`, `Tabulation`, `Comment`, and `Remark` nodes.
//! - **Schema model** (`Schema`, `Layer`, `Definition`, `Struct`, `Scalar`, `Flag`, `Field`,
//!   `Select`, `Variant`, `Member::Exclude`) of §20.
//! - **Type assignment** (`type_assign`) implementing §20.2's atom-then-compound algorithm,
//!   the §20.1 schema-validity checks, and the §21 validator-callback model.
//! - **Schema composition** (`compose_schema`) implementing §20.3's MergeStruct algorithm,
//!   producing the subtype guaranteed by §24.4.
//! - **Indentation recovery** for E107 / E111 per §19.5.
//! - **Built-in `tel-schema`** (`builtin_tel_schema`) with pinned BinTEL value hash per §20.5.
//!
//! Sub-modules: [`bintel`] (§7 of BinTEL Specification), [`canonical`] (§22.3),
//! [`mutate`] (§22.2 machine operations), [`resolver`] (§8.2 schema resolution),
//! [`base256`] (BASE-256 codec).

pub use base256;
pub mod bintel;
pub mod canonical;
pub mod mutate;
pub mod resolver;

use std::fmt;

// ── Error types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct TelError {
    pub code: ErrorCode,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    E101, E102, E103, E104, E105,
    E106, E107, E108, E109, E111, E112, E113, E114, E115,
    E116, E117, E118, E119, E120, E121, E122, E123,
    // Schema validity errors (§20.1)
    E201, E202, E204, E205, E206, E207, E208, E209, E210, E211,
    E212, E213, E214, E215, E216, E217,
    // Validation errors (§20.2 + §21)
    E301, E302, E303, E304, E305, E306, E307, E308, E309, E310, E311,
}

impl ErrorCode {
    fn message(self) -> &'static str {
        match self {
            Self::E101 => "BOM present at start of document",
            Self::E102 => "Pragma is not the first non-blank line",
            Self::E103 => "Pragma line extends beyond first 4096 bytes",
            Self::E104 => "Invalid pragma version",
            Self::E105 => "Invalid sigil character",
            Self::E106 => "Line does not begin with the margin",
            Self::E107 => "Odd indentation",
            Self::E108 => "Trailing spaces on ordinary line",
            Self::E109 => "Comment must follow a blank line, another comment, or start of document",
            Self::E111 => "Over-indentation",
            Self::E112 => "Child of comment, tabulation, or tabulated row",
            Self::E113 => "Source atom already present on this compound",
            Self::E114 => "Literal atom already present on this compound",
            Self::E115 => "Unclosed literal atom",
            Self::E116 => "Tabulated row has wrong indentation",
            Self::E117 => "Hard space does not end at a column boundary",
            Self::E118 => "Consecutive spaces within column value",
            Self::E119 => "Column value exceeds maximum width",
            Self::E120 => "Malformed tabulation heading",
            Self::E121 => "Line-ending inconsistency",
            Self::E122 => "Invalid schema identifier",
            Self::E123 => "Pragma has extra atoms",
            Self::E201 => "Duplicate keyword within a Struct",
            Self::E202 => "Select member has empty variants list",
            Self::E204 => "Scalar has non-null default but member is not required",
            Self::E205 => "Two or more Layers share the same name",
            Self::E206 => "Layer Select variant keyword overlaps existing keyword in base Struct",
            Self::E207 => "Layer Field merge requires both base and layer types to be Struct",
            Self::E208 => "Schema.sigil character is not permitted",
            Self::E209 => "Keyword `tel` is reserved and must not be used as a Field or Variant keyword",
            Self::E210 => "Reference does not resolve to a Definition in the schema",
            Self::E211 => "Two or more Definitions share the same name",
            Self::E212 => "`exclude K` names a keyword that does not identify a Select variant in the merged Struct",
            Self::E213 => "`exclude K` would empty a required Select",
            Self::E214 => "Layer attempts to add a variant to an existing Select in a non-subtyping way",
            Self::E215 => "Layer cannot loosen a required member to optional",
            Self::E216 => "Layer cannot loosen an irrepeatable member to repeatable",
            Self::E217 => "Exclude operation appears outside a layer's root",
            Self::E301 => "Compound's type is not a Struct",
            Self::E302 => "More atoms than assignable member positions",
            Self::E303 => "Atom appears at a member position that is not atom-assignable",
            Self::E304 => "Atom text matches no variant keyword of a Select member",
            Self::E305 => "Atom text does not match a Field member's Flag keyword",
            Self::E306 => "Compound keyword is not recognized for its parent type",
            Self::E307 => "Required member absent and no default available",
            Self::E308 => "Non-repeatable member is filled more than once",
            Self::E309 => "Compound children of the same member are not contiguous",
            Self::E310 => "Scalar value failed validation",
            Self::E311 => "Flag-typed compound has atoms or compound children",
        }
    }
}

impl TelError {
    fn new(code: ErrorCode, start: usize, end: usize) -> Self {
        TelError { code, start, end, message: code.message().to_string() }
    }

    fn with_detail(code: ErrorCode, start: usize, end: usize, detail: impl fmt::Display) -> Self {
        TelError { code, start, end, message: format!("{}: {}", code.message(), detail) }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl fmt::Display for TelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{},{}): {}", self.code, self.start, self.end, self.message)
    }
}

// ── Presentation model ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub interpreter_directive: Option<String>,
    pub pragma: Option<Pragma>,
    pub line_endings: LineEndings,
    pub children: Vec<Block>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndings { LF, CRLF }

#[derive(Debug, Clone, PartialEq)]
pub struct Pragma {
    pub version: (u32, u32),
    pub schema: Option<String>,
    pub sigil: Option<char>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub comments: Vec<Comment>,
    pub tabulation: Option<Tabulation>,
    pub compounds: Vec<Compound>,
    pub trailing_blank_lines: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comment { pub text: String }

#[derive(Debug, Clone, PartialEq)]
pub struct Tabulation {
    pub marker_offsets: Vec<usize>,
    pub headings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compound {
    pub keyword: String,
    pub atoms: Vec<Atom>,
    pub remark: Option<String>,
    pub children: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
    Inline { text: String, preceding_spaces: usize },
    Source { text: String },
    Literal { delimiter: String, text: String },
}

// ── Schema model (§20) ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub name: String,
    pub document: Struct,
    pub layers: Vec<Layer>,
    pub sigil: Option<char>,
    /// User-declared `record` definitions (named struct types).
    pub types: Vec<Definition>,
    /// User-declared `scalar` definitions (named scalar types). Shares a
    /// single global namespace with `types` and the built-in type names
    /// (`flag`, `string`, `identifier`, `sigil`).
    pub scalars: Vec<ScalarDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Members merged into the composed document root (§20.3). Written as
    /// the `overlay` keyword in TEL source.
    pub overlay: Struct,
    pub types: Vec<Definition>,
    pub scalars: Vec<ScalarDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub members: Vec<Member>,
    /// Struct-level validators (§21.6) applying to instances of this
    /// Definition. Same semantics as `Struct.validators`.
    pub validators: Vec<String>,
}

/// A named scalar type declared via `scalar <name>` at schema or layer
/// scope. Its `validators` apply (in AND-conjunction) to every value
/// whose field/variant references this scalar by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarDefinition {
    pub name: String,
    pub validators: Vec<String>,
}

/// A `Type` is what a Field or Variant evaluates to. In the v1.0 schema
/// syntax every user-written field/variant type is a `Reference`; the
/// non-`Reference` variants exist only as resolution results.
/// `Type::Reference(name)` resolves (per §20.2) to either a `Struct` formed
/// from the named record's `members`, a `Scalar` formed from the named
/// scalar's `validators`, or one of the built-in types `flag`, `string`,
/// `identifier`, `sigil`.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Struct(Struct),
    Scalar(Scalar),
    Flag,
    Reference(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Struct {
    pub members: Vec<Member>,
    /// Struct-level validators (§21.6). Each name resolves through the
    /// shared validator namespace (§21.1) to a helper method that
    /// inspects the entire Struct element. Multiple validators apply in
    /// AND-conjunction.
    pub validators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    /// Scalar-level validators (§21.1). Each name resolves through the
    /// shared validator namespace to a helper method that inspects the
    /// scalar's value text. Multiple validators apply in AND-conjunction.
    /// An empty list means the Scalar accepts any text.
    pub validators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    Field(Field),
    Select(Select),
    /// Layer-only operation: exclude the variant with the given keyword
    /// from whichever `Select` in the merged Struct declares it (§20.3).
    /// `Exclude` MUST NOT appear in a base schema's `Schema.document` or
    /// in any `Definition` of `Schema.types`; appearing there is a
    /// schema validity error.
    Exclude(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub required: bool,
    pub repeatable: bool,
    pub keyword: String,
    pub r#type: Type,
    /// Per-use-site default value, applied when a required Scalar-typed
    /// field is absent from the document. Valid only when `required` is
    /// `true` and the resolved `type` is `Scalar` (E204 otherwise).
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Select {
    pub required: bool,
    pub repeatable: bool,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub keyword: String,
    pub r#type: Type,
}

// ── Validators (§21) ────────────────────────────────────────────────────────

/// A validation request carries the method name (the validator's
/// kebab-case identifier) plus the value being validated. The shape
/// distinguishes scalar requests (value is a string) from struct
/// requests (value is a structural view of a `Struct` element).
#[derive(Debug, Clone)]
pub enum ValidationRequest<'a> {
    Scalar { method: &'a str, value: &'a str },
    Struct { method: &'a str, element: StructView<'a> },
}

impl<'a> ValidationRequest<'a> {
    pub fn method(&self) -> &str {
        match self {
            ValidationRequest::Scalar { method, .. } => method,
            ValidationRequest::Struct { method, .. } => method,
        }
    }
}

/// A read-only view into a `Struct` semantic element, supplied to struct
/// validators (§21.6). Provides accessors for child values by keyword
/// without exposing the underlying parse representation.
#[derive(Debug, Clone)]
pub struct StructView<'a> {
    pub compound: &'a Compound,
    pub members: &'a [Member],
    pub schema: &'a Schema,
}

impl<'a> StructView<'a> {
    /// Return the value text of the Scalar child with the given keyword,
    /// or `None` if no such child is present. If the keyword refers to a
    /// non-Scalar member (Struct, Flag, Select-variant of non-Scalar
    /// type), returns `None` — a struct validator that wants to inspect
    /// such a child should use `struct_field` or `flag` instead.
    pub fn scalar(&self, keyword: &str) -> Option<String> {
        for block in &self.compound.children {
            for c in &block.compounds {
                if c.keyword == keyword {
                    return Some(scalar_value_text(c));
                }
            }
        }
        None
    }

    /// Return true iff a Flag-typed child with the given keyword is
    /// present (either as an inline atom on the parent's line — already
    /// reflected in the semantic model — or as a bare compound child).
    pub fn flag(&self, keyword: &str) -> bool {
        for block in &self.compound.children {
            for c in &block.compounds {
                if c.keyword == keyword && c.atoms.is_empty() && c.children.is_empty() {
                    return true;
                }
            }
        }
        false
    }

    /// Return a nested `StructView` over the Struct child with the given
    /// keyword, or `None` if no such child is present or the child is not
    /// Struct-typed. The nested view's `members` are resolved through
    /// the schema's Reference chain.
    pub fn struct_field(&self, keyword: &str) -> Option<StructView<'a>> {
        let child = self.compound.children.iter()
            .flat_map(|b| b.compounds.iter())
            .find(|c| c.keyword == keyword)?;
        let t = keyword_type_in(self.members, keyword)?;
        match resolve(t, self.schema) {
            ResolvedType::Struct(child_members) => Some(StructView {
                compound: child,
                members: child_members,
                schema: self.schema,
            }),
            _ => None,
        }
    }

    /// Iterate every child compound with its keyword. Useful for
    /// validators that need to inspect every present child without
    /// knowing the schema's member layout ahead of time.
    pub fn children(&self) -> impl Iterator<Item = &'a Compound> + '_ {
        self.compound.children.iter().flat_map(|b| b.compounds.iter())
    }
}

/// Look up a Field's keyword in a member list and return its declared
/// Type (before Reference resolution). Helper for `StructView`.
fn keyword_type_in<'a>(members: &'a [Member], keyword: &str) -> Option<&'a Type> {
    for m in members {
        match m {
            Member::Field(f) if f.keyword == keyword => return Some(&f.r#type),
            Member::Select(s) => {
                for v in &s.variants {
                    if v.keyword == keyword {
                        return Some(&v.r#type);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResponse {
    Valid,
    Invalid(Diagnostic),
}

/// A diagnostic returned by a validator. The variant matches the kind
/// of the request: a Scalar request returns `Diagnostic::Scalar`, a
/// Struct request returns `Diagnostic::Struct`. Mismatched kinds are a
/// contract violation.
#[derive(Debug, Clone, PartialEq)]
pub enum Diagnostic {
    /// Diagnostic on a scalar value: a message and an optional span
    /// pointing into the value's text (zero-based code-point indices,
    /// `[start, end)` half-open).
    Scalar {
        message: String,
        span: Option<Span>,
    },
    /// Diagnostic on a struct element: a message and an optional map of
    /// per-field diagnostics, keyed by child keyword. Each nested
    /// diagnostic MUST match the schema type of its keyword's child
    /// (Scalar for scalar children, Struct for struct children;
    /// span-less Scalar acceptable for Flag children).
    Struct {
        message: String,
        fields: std::collections::HashMap<String, Diagnostic>,
    },
}

/// A half-open span `[start, end)` of zero-based code-point indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Validator callback: maps a `ValidationRequest` to a `ValidationResponse`.
/// One callback handles every validator name; it dispatches internally on
/// the validator name and on the request kind (Scalar vs Struct).
pub type ValidatorFn = dyn Fn(&ValidationRequest) -> ValidationResponse + Send + Sync;

fn scalar_invalid(msg: &str, end: usize) -> ValidationResponse {
    ValidationResponse::Invalid(Diagnostic::Scalar {
        message: msg.to_string(),
        span: Some(Span { start: 0, end }),
    })
}

fn struct_not_applicable(method: &str) -> ValidationResponse {
    ValidationResponse::Invalid(Diagnostic::Struct {
        message: format!("built-in validator `{}` does not apply to struct values", method),
        fields: std::collections::HashMap::new(),
    })
}

/// Built-in validator: `identifier`. Accepts a kebab-case identifier per §20.7,
/// optionally including leading prime (`'`) characters.
pub fn validate_identifier(value: &str) -> ValidationResponse {
    let end = value.chars().count();
    let mk = |msg: &str| scalar_invalid(msg, end);
    if value.is_empty() { return mk("empty identifier"); }
    let mut chars = value.chars().peekable();
    // Skip leading primes.
    while chars.peek() == Some(&'\'') { chars.next(); }
    let first = match chars.next() {
        Some(c) => c,
        None => return mk("identifier consists only of primes"),
    };
    if !first.is_ascii_lowercase() {
        return mk("identifier must start with a lowercase ASCII letter (after any leading primes)");
    }
    let mut prev_hyphen = false;
    while let Some(c) = chars.next() {
        if c == '-' {
            if prev_hyphen { return mk("consecutive hyphens not allowed"); }
            if chars.peek().is_none() { return mk("trailing hyphen not allowed"); }
            prev_hyphen = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_hyphen = false;
        } else {
            return mk("identifier may contain only lowercase ASCII letters, digits, and hyphens");
        }
    }
    ValidationResponse::Valid
}

/// Built-in validator: `sigil`. Accepts a single-character string whose
/// character satisfies the sigil constraints in §8.
pub fn validate_sigil(value: &str) -> ValidationResponse {
    let end = value.chars().count();
    let mk = |msg: &str| scalar_invalid(msg, end);
    let mut chars = value.chars();
    let ch = match chars.next() {
        Some(c) => c,
        None => return mk("empty sigil"),
    };
    if chars.next().is_some() { return mk("sigil must be a single character"); }
    if !ch.is_ascii() { return mk("sigil must be an ASCII character"); }
    if ch == ' ' || ch == '\n' || ch == '\r' { return mk("sigil must not be whitespace"); }
    if ch.is_ascii_alphabetic() { return mk("sigil must not be a letter"); }
    if ch.is_ascii_digit() { return mk("sigil must not be a digit"); }
    if ch.is_ascii_control() { return mk("sigil must not be a control character"); }
    if matches!(ch, '(' | ')' | '[' | ']' | '<' | '>' | '{' | '}') {
        return mk("sigil must not be a parenthetical symbol");
    }
    ValidationResponse::Valid
}

/// Built-in validator: `string`. Accepts any input; always returns `Valid`.
pub fn validate_string(_value: &str) -> ValidationResponse {
    ValidationResponse::Valid
}

/// Dispatch a validation request to a built-in validator if one matches,
/// otherwise delegate to the optional user callback. Built-ins respond to
/// Scalar requests; a Struct request for a built-in name returns Invalid
/// with `Diagnostic::Struct { message: "not applicable", fields: {}, validators: Vec::new() }`.
pub fn validate_with_builtins(
    req: &ValidationRequest,
    user: Option<&ValidatorFn>,
) -> ValidationResponse {
    let method = req.method();
    // Built-ins handle scalar kind only.
    let builtin = matches!(method, "identifier" | "sigil" | "string");
    if builtin {
        match req {
            ValidationRequest::Scalar { value, .. } => match method {
                "identifier" => return validate_identifier(value),
                "sigil" => return validate_sigil(value),
                "string" => return validate_string(value),
                _ => unreachable!(),
            },
            ValidationRequest::Struct { .. } => return struct_not_applicable(method),
        }
    }
    match user {
        Some(cb) => cb(req),
        None => ValidationResponse::Valid, // no callback → opt out per §21.4
    }
}

// ── Built-in tel-schema (§20.5 bootstrap requirement) ───────────────────────

/// The hardcoded `Schema` value describing TEL's schema language. This is
/// the schema referenced by every TEL schema document, and the closure
/// invariant of §20.5 requires it to match what `tel-schema.tel` describes.
pub fn builtin_tel_schema() -> Schema {
    // Helpers — all built-in scalar/flag types are reached via References
    // through `resolve_name`. This keeps the built-in and the
    // self-bootstrap parse of `tel-schema.tel` byte-identical.
    let scalar_id = || Type::Reference("identifier".to_string());
    let scalar_sigil = || Type::Reference("sigil".to_string());
    let scalar_str = || Type::Reference("string".to_string());
    let refn = |n: &str| Type::Reference(n.to_string());
    let field = |req: bool, rep: bool, kw: &str, t: Type| Member::Field(Field {
        required: req, repeatable: rep, keyword: kw.to_string(),
        r#type: t, default: None,
    });
    let select = |req: bool, rep: bool, variants: Vec<Variant>| Member::Select(Select {
        required: req, repeatable: rep, variants,
    });
    let variant = |kw: &str, t: Type| Variant { keyword: kw.to_string(), r#type: t };

    // Member Select used inside a record-body. `field`/`select`/`validate`.
    let record_member_select = || select(false, true, vec![
        variant("field", refn("field-body")),
        variant("select", refn("select-body")),
        variant("validate", scalar_id()),
    ]);

    // Member Select used inside an overlay-body. Includes `exclude` (layer-only).
    let overlay_member_select = || select(false, true, vec![
        variant("field", refn("field-body")),
        variant("select", refn("select-body")),
        variant("exclude", scalar_id()),
        variant("validate", scalar_id()),
    ]);

    // Member Select used inside the schema's document body. No `exclude`.
    let document_member_select = || select(false, true, vec![
        variant("field", refn("field-body")),
        variant("select", refn("select-body")),
        variant("validate", scalar_id()),
    ]);

    let layer_body = Definition {
        name: "layer-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
            field(false, true, "record", refn("record-body")),
            field(false, true, "scalar", refn("scalar-body")),
            field(false, false, "overlay", refn("overlay-body")),
        ], validators: Vec::new(),
    };

    let record_body = Definition {
        name: "record-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
            record_member_select(),
        ], validators: Vec::new(),
    };

    let scalar_body = Definition {
        name: "scalar-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
            field(true, true, "validate", scalar_id()),
        ], validators: Vec::new(),
    };

    let overlay_body = Definition {
        name: "overlay-body".to_string(),
        members: vec![overlay_member_select()], validators: Vec::new(),
    };

    let document_body = Definition {
        name: "document-body".to_string(),
        members: vec![document_member_select()], validators: Vec::new(),
    };

    // Field-body: keyword, four loosen/tighten flags, then the required
    // `type` Scalar and the optional `default` Scalar. This ordering lets
    // a Field declare itself as a one-liner — `field foo optional string
    // unknown`.
    let field_body = Definition {
        name: "field-body".to_string(),
        members: vec![
            field(true, false, "keyword", scalar_id()),
            field(false, false, "optional", Type::Reference("flag".to_string())),
            field(false, false, "required", Type::Reference("flag".to_string())),
            field(false, false, "repeatable", Type::Reference("flag".to_string())),
            field(false, false, "irrepeatable", Type::Reference("flag".to_string())),
            field(true, false, "type", scalar_id()),
            field(false, false, "default", scalar_str()),
        ], validators: Vec::new(),
    };

    let select_body = Definition {
        name: "select-body".to_string(),
        members: vec![
            field(false, false, "optional", Type::Reference("flag".to_string())),
            field(false, false, "required", Type::Reference("flag".to_string())),
            field(false, false, "repeatable", Type::Reference("flag".to_string())),
            field(false, false, "irrepeatable", Type::Reference("flag".to_string())),
            field(true, true, "variant", refn("variant-body")),
        ], validators: Vec::new(),
    };

    let variant_body = Definition {
        name: "variant-body".to_string(),
        members: vec![
            field(true, false, "keyword", scalar_id()),
            field(true, false, "type", scalar_id()),
        ], validators: Vec::new(),
    };

    // The schema-document Struct: top-level members of any schema document.
    let document = Struct {
        validators: Vec::new(),
        members: vec![
            field(true, false, "name", scalar_id()),
            field(false, false, "sigil", scalar_sigil()),
            field(false, true, "record", refn("record-body")),
            field(false, true, "scalar", refn("scalar-body")),
            field(true, false, "document", refn("document-body")),
            field(false, true, "layer", refn("layer-body")),
        ],
    };

    Schema {
        name: "tel-schema".to_string(),
        document,
        layers: vec![],
        sigil: None,
        types: vec![
            layer_body, record_body, scalar_body, overlay_body, document_body,
            field_body, select_body, variant_body,
        ],
        scalars: Vec::new(),
    }
}

// ── Reference resolution (§20.2) ────────────────────────────────────────────

/// The predefined type names that every TEL parser MUST recognize regardless
/// of the user schema. User schemas MAY NOT declare a `record` or `scalar`
/// with any of these names.
pub const BUILTIN_TYPE_NAMES: &[&str] = &["flag", "string", "identifier", "sigil"];

/// Resolve a `Reference` to a record's `Member` slice. Returns `None` if the
/// name doesn't resolve to a record (e.g. it's a built-in, a scalar
/// definition, or unknown).
pub(crate) fn resolve_reference<'a>(name: &str, schema: &'a Schema) -> Option<&'a [Member]> {
    schema.types.iter()
        .chain(schema.layers.iter().flat_map(|l| l.types.iter()))
        .find(|d| d.name == name)
        .map(|d| d.members.as_slice())
}

/// Per §20.2, resolve a Type that may be a Reference into a concrete
/// non-Reference type. Built-in names (`flag`, `string`, `identifier`,
/// `sigil`) short-circuit to owned built-in types. Records resolve to a
/// member-slice borrow; scalars resolve to an owned `Scalar` synthesized
/// from the definition's validators.
pub(crate) enum ResolvedType<'a> {
    Struct(&'a [Member]),
    /// An owned Scalar — used for both built-in scalar types and named
    /// `scalar` definitions. The `Cow` lets us return a borrowed Scalar
    /// when the source is a literal `Type::Scalar(_)`, and an owned one
    /// when it's a built-in or a named scalar definition.
    Scalar(std::borrow::Cow<'a, Scalar>),
    Flag,
    Unresolved, // Reference whose name doesn't resolve (E210 caught at schema-validity time)
}

pub(crate) fn resolve<'a>(t: &'a Type, schema: &'a Schema) -> ResolvedType<'a> {
    use std::borrow::Cow;
    match t {
        Type::Struct(s) => ResolvedType::Struct(&s.members),
        Type::Scalar(s) => ResolvedType::Scalar(Cow::Borrowed(s)),
        Type::Flag => ResolvedType::Flag,
        Type::Reference(n) => resolve_name(n, schema),
    }
}

pub(crate) fn resolve_name<'a>(name: &str, schema: &'a Schema) -> ResolvedType<'a> {
    use std::borrow::Cow;
    // Built-in names short-circuit.
    match name {
        "flag" => return ResolvedType::Flag,
        "string" | "identifier" | "sigil" => {
            return ResolvedType::Scalar(Cow::Owned(Scalar {
                validators: vec![name.to_string()],
            }));
        }
        _ => {}
    }
    // Record definitions
    if let Some(members) = resolve_reference(name, schema) {
        return ResolvedType::Struct(members);
    }
    // Scalar definitions
    for s in schema.scalars.iter()
        .chain(schema.layers.iter().flat_map(|l| l.scalars.iter()))
    {
        if s.name == name {
            return ResolvedType::Scalar(Cow::Owned(Scalar {
                validators: s.validators.clone(),
            }));
        }
    }
    ResolvedType::Unresolved
}

// ── Type assignment (§20.2) ─────────────────────────────────────────────────

/// Result of type-assigning a document against a schema. Carries E3xx errors
/// and (optionally) E310 errors from validator callbacks.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAssignment {
    pub errors: Vec<TelError>,
}

/// Type-assign a `Document` against a `Schema`. Implements §20.2 in full.
///
/// When the schema has layers, this function composes them first via
/// `compose_schema` (§20.3) and type-assigns against the composed schema.
/// Composition errors (E2xx) are not surfaced here — callers that wish
/// to see them should call `compose_schema` directly. (Composition is
/// idempotent: an already-composed schema is returned unchanged.)
pub fn type_assign(
    doc: &Document,
    schema: &Schema,
    validator_cb: Option<&ValidatorFn>,
) -> TypeAssignment {
    let mut errors = Vec::new();
    // Compose layers (§20.3) before walking, so layer-introduced
    // keywords are resolvable. The composed schema is also our
    // Reference-resolution context.
    let composed_owned;
    let schema: &Schema = if schema.layers.is_empty() {
        schema
    } else {
        composed_owned = compose_schema(schema).0;
        &composed_owned
    };
    let root_members = &schema.document.members;
    // Root has no atoms (per §20.2 Document root), so we go straight to compounds.
    assign_compound_children_at_root(&doc.children, root_members, schema, validator_cb, &mut errors);
    TypeAssignment { errors }
}

/// Walk the root's child blocks and apply the compound-child phase.
fn assign_compound_children_at_root(
    blocks: &[Block],
    members: &[Member],
    schema: &Schema,
    cb: Option<&ValidatorFn>,
    errors: &mut Vec<TelError>,
) {
    let k = build_keyword_map(members);
    let mut current_member: i32 = -1;
    let mut seen_members: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut fill_counts: Vec<usize> = vec![0; members.len()];

    for block in blocks {
        for compound in &block.compounds {
            match k.get(compound.keyword.as_str()) {
                None => {
                    errors.push(TelError::with_detail(
                        ErrorCode::E306, 0, 0,
                        format!("unrecognized keyword `{}` for the document root", compound.keyword),
                    ));
                }
                Some(&(i, ref child_type)) => {
                    // Contiguity check (E309)
                    if i as i32 != current_member {
                        if seen_members.contains(&i) {
                            errors.push(TelError::with_detail(
                                ErrorCode::E309, 0, 0,
                                format!("children for member `{}` are not contiguous", compound.keyword),
                            ));
                        }
                        if current_member >= 0 {
                            seen_members.insert(current_member as usize);
                        }
                        current_member = i as i32;
                    }
                    fill_counts[i] += 1;
                    // Recurse into the compound with the child's type
                    type_assign_compound(compound, child_type, schema, cb, errors);
                }
            }
        }
    }

    // Constraint check (§20.2 step 5)
    check_member_constraints(members, &fill_counts, errors);
}

/// `K`: keyword → (member index, type — already cloned for ownership ease).
fn build_keyword_map(members: &[Member]) -> std::collections::HashMap<&str, (usize, Type)> {
    let mut k = std::collections::HashMap::new();
    for (i, m) in members.iter().enumerate() {
        match m {
            Member::Field(f) => {
                k.insert(f.keyword.as_str(), (i, f.r#type.clone()));
            }
            Member::Exclude(_) => {
                // Remove operations are layer-only; they should not be
                // present in a composed schema being type-assigned. If
                // one appears here, it's a no-op for keyword mapping.
            }
            Member::Select(s) => {
                for v in &s.variants {
                    k.insert(v.keyword.as_str(), (i, v.r#type.clone()));
                }
            }
        }
    }
    k
}

/// Emit one E310 error per leaf diagnostic, walking the recursive
/// `Diagnostic` structure. Span resolution per §21.3 is the caller's
/// concern; this helper records start/end as the diagnostic's local
/// span when present, or (0, 0) when absent, with the diagnostic's
/// message folded into the TelError detail. A more elaborate
/// implementation would translate spans to document offsets; for now
/// this records the message and (if present) the value-relative span.
fn emit_e310(diag: &Diagnostic, ctx: &str, errors: &mut Vec<TelError>) {
    match diag {
        Diagnostic::Scalar { message, span } => {
            let (start, end) = match span {
                Some(s) => (s.start, s.end),
                None => (0, 0),
            };
            errors.push(TelError::with_detail(
                ErrorCode::E310, start, end,
                format!("`{}` failed validation: {}", ctx, message),
            ));
        }
        Diagnostic::Struct { message, fields } => {
            errors.push(TelError::with_detail(
                ErrorCode::E310, 0, 0,
                format!("`{}` failed struct validation: {}", ctx, message),
            ));
            for (kw, child) in fields {
                let child_ctx = format!("{}.{}", ctx, kw);
                emit_e310(child, &child_ctx, errors);
            }
        }
    }
}

/// Type-assign a single compound against a `Type` (after Reference resolution).
fn type_assign_compound(
    c: &Compound,
    t: &Type,
    schema: &Schema,
    cb: Option<&ValidatorFn>,
    errors: &mut Vec<TelError>,
) {
    match resolve(t, schema) {
        ResolvedType::Unresolved => {
            // Schema-validity reports E210; nothing to do here.
        }
        ResolvedType::Flag => {
            // E311: Flag compound must have no atoms and no compound children.
            if !c.atoms.is_empty() || !c.children.is_empty() {
                errors.push(TelError::with_detail(
                    ErrorCode::E311, 0, 0,
                    format!("Flag compound `{}` has atoms or children", c.keyword),
                ));
            }
        }
        ResolvedType::Scalar(sc) => {
            // Scalar compound's value is its inline atom text (or "" if none).
            let value = scalar_value_text(c);
            // The spec says compound's value = inline atom text. Multiple atoms
            // would be excess; report as E302 (more atoms than positions).
            if c.atoms.len() > 1 {
                errors.push(TelError::with_detail(
                    ErrorCode::E302, 0, 0,
                    format!("Scalar compound `{}` has more than one atom", c.keyword),
                ));
            }
            // A Scalar compound is a leaf and MUST NOT have child blocks
            // (§20.2 step 1: T MUST be a Struct to host children → E301).
            let has_children = c.children.iter().any(|b| !b.compounds.is_empty());
            if has_children {
                errors.push(TelError::with_detail(
                    ErrorCode::E301, 0, 0,
                    format!("compound `{}` has children but its type is Scalar, not Struct", c.keyword),
                ));
            }
            // E310: invoke each validator on the scalar's value; all must
            // return Valid (AND-conjunction).
            for validator in &sc.validators {
                let req = ValidationRequest::Scalar { method: validator, value: &value };
                if let ValidationResponse::Invalid(diag) = validate_with_builtins(&req, cb) {
                    emit_e310(&diag, &c.keyword, errors);
                }
            }
        }
        ResolvedType::Struct(members) => {
            // §20.2: T MUST be a Struct (it is, after resolution). Run atom phase
            // then compound child phase then constraint check.
            let k = build_keyword_map(members);
            let mut pos: usize = 0;
            let mut fill_counts: Vec<usize> = vec![0; members.len()];

            // Atom phase
            for atom in &c.atoms {
                let atom_text = atom_text(atom);
                // Advance pos while skip condition holds
                while pos < members.len() {
                    let m = &members[pos];
                    let (is_required, is_skippable_flag) = match m {
                        Member::Field(f) => {
                            let resolved_type = resolve(&f.r#type, schema);
                            let is_flag = matches!(resolved_type, ResolvedType::Flag);
                            let atom_matches = is_flag && f.keyword == atom_text;
                            // Skippable if not required AND (not atom-assignable OR (Flag and atom doesn't match))
                            let atom_assignable = matches!(resolved_type,
                                ResolvedType::Scalar(_) | ResolvedType::Flag);
                            let skip = !f.required && (!atom_assignable || (is_flag && !atom_matches));
                            (f.required, skip)
                        }
                        Member::Exclude(_) => (false, true), // Skip Exclude ops in atom phase.
                        Member::Select(s) => {
                            // Select is atom-assignable iff all variants are Flag.
                            let all_flag = s.variants.iter().all(|v|
                                matches!(resolve(&v.r#type, schema), ResolvedType::Flag));
                            let atom_matches_some = s.variants.iter().any(|v| v.keyword == atom_text);
                            let skip = !s.required && (!all_flag || (all_flag && !atom_matches_some));
                            (s.required, skip)
                        }
                    };
                    if is_required { break; }
                    if !is_skippable_flag { break; }
                    pos += 1;
                }

                if pos >= members.len() {
                    errors.push(TelError::with_detail(
                        ErrorCode::E302, 0, 0,
                        format!("more atoms than assignable member positions on `{}`", c.keyword),
                    ));
                    break;
                }

                let m = &members[pos];
                // Atom-assignability check (E303)
                let atom_assignable = match m {
                    Member::Field(f) => matches!(resolve(&f.r#type, schema),
                        ResolvedType::Scalar(_) | ResolvedType::Flag),
                    Member::Select(s) => s.variants.iter().all(|v|
                        matches!(resolve(&v.r#type, schema), ResolvedType::Flag)),
                    Member::Exclude(_) => false,
                };
                if !atom_assignable {
                    errors.push(TelError::with_detail(
                        ErrorCode::E303, 0, 0,
                        format!("atom `{}` at non-atom-assignable position on `{}`", atom_text, c.keyword),
                    ));
                    break;
                }

                // Assign atom to member
                match m {
                    Member::Exclude(_) => {
                        // Should not be reachable: Remove is skipped above.
                    }
                    Member::Field(f) => {
                        match resolve(&f.r#type, schema) {
                            ResolvedType::Flag => {
                                if f.keyword != atom_text {
                                    errors.push(TelError::with_detail(
                                        ErrorCode::E305, 0, 0,
                                        format!("atom `{}` does not match Flag keyword `{}`",
                                                atom_text, f.keyword),
                                    ));
                                }
                            }
                            ResolvedType::Scalar(sc) => {
                                for validator in &sc.validators {
                                    let req = ValidationRequest::Scalar {
                                        method: validator, value: &atom_text,
                                    };
                                    if let ValidationResponse::Invalid(diag) = validate_with_builtins(&req, cb) {
                                        emit_e310(&diag, &f.keyword, errors);
                                    }
                                }
                            }
                            _ => {}
                        }
                        fill_counts[pos] += 1;
                        if !f.repeatable { pos += 1; }
                    }
                    Member::Select(s) => {
                        // All variants must be Flag (checked above).
                        let matched = s.variants.iter().any(|v| v.keyword == atom_text);
                        if !matched {
                            errors.push(TelError::with_detail(
                                ErrorCode::E304, 0, 0,
                                format!("atom `{}` matches no variant keyword in Select", atom_text),
                            ));
                        }
                        fill_counts[pos] += 1;
                        if !s.repeatable { pos += 1; }
                    }
                }
            }

            // Compound child phase
            let mut current_member: i32 = -1;
            let mut seen_members: std::collections::HashSet<usize> = std::collections::HashSet::new();
            for block in &c.children {
                for child in &block.compounds {
                    match k.get(child.keyword.as_str()) {
                        None => {
                            errors.push(TelError::with_detail(
                                ErrorCode::E306, 0, 0,
                                format!("unrecognized keyword `{}` in `{}`", child.keyword, c.keyword),
                            ));
                        }
                        Some(&(i, ref child_type)) => {
                            if i as i32 != current_member {
                                if seen_members.contains(&i) {
                                    errors.push(TelError::with_detail(
                                        ErrorCode::E309, 0, 0,
                                        format!("children for member `{}` are not contiguous", child.keyword),
                                    ));
                                }
                                if current_member >= 0 {
                                    seen_members.insert(current_member as usize);
                                }
                                current_member = i as i32;
                            }
                            fill_counts[i] += 1;
                            type_assign_compound(child, child_type, schema, cb, errors);
                        }
                    }
                }
            }

            // Constraint check (E307, E308)
            check_member_constraints(members, &fill_counts, errors);

            // Struct-level validators (§21.6). The validators are
            // declared on the *Type*, not on the resolved members
            // alone, so look them up via the original Type. After
            // child validation has completed, invoke each validator;
            // a returned `Diagnostic::Struct` is emitted as E310.
            let validators = struct_validators(t, schema).unwrap_or(&[]);
            if !validators.is_empty() {
                let element = StructView { compound: c, members, schema };
                for validator in validators {
                    let req = ValidationRequest::Struct {
                        method: validator, element: element.clone(),
                    };
                    if let ValidationResponse::Invalid(diag) = validate_with_builtins(&req, cb) {
                        emit_e310(&diag, &c.keyword, errors);
                    }
                }
            }
        }
    }
}

/// Return the struct-level validators of the given Type, if it resolves
/// to a Struct (either directly or via a Reference). Returns `None`
/// for Scalar, Flag, and unresolved Reference types.
fn struct_validators<'a>(t: &'a Type, schema: &'a Schema) -> Option<&'a [String]> {
    match t {
        Type::Struct(s) => Some(&s.validators),
        Type::Reference(n) => schema.types.iter()
            .chain(schema.layers.iter().flat_map(|l| l.types.iter()))
            .find(|d| d.name == *n)
            .map(|d| d.validators.as_slice()),
        _ => None,
    }
}

fn check_member_constraints(
    members: &[Member],
    fill_counts: &[usize],
    errors: &mut Vec<TelError>,
) {
    for (i, m) in members.iter().enumerate() {
        let fc = fill_counts[i];
        let (required, repeatable, label) = match m {
            Member::Field(f) => (f.required, f.repeatable, f.keyword.clone()),
            Member::Select(s) => {
                let names: Vec<&str> = s.variants.iter().map(|v| v.keyword.as_str()).collect();
                (s.required, s.repeatable, names.join("|"))
            }
            Member::Exclude(_) => continue, // Skip; no constraint applies.
        };
        // E307: required and empty (defaults handled separately for Scalar)
        if required && fc == 0 {
            let has_default = matches!(m, Member::Field(f) if f.default.is_some());
            if !has_default {
                errors.push(TelError::with_detail(
                    ErrorCode::E307, 0, 0,
                    format!("required member `{}` is absent and has no default", label),
                ));
            }
        }
        // E308: non-repeatable filled twice
        if !repeatable && fc > 1 {
            errors.push(TelError::with_detail(
                ErrorCode::E308, 0, 0,
                format!("non-repeatable member `{}` is filled {} times", label, fc),
            ));
        }
    }
}

/// Extract a Compound's Scalar value text: the first inline atom's text, or
/// `""` if none.
pub(crate) fn scalar_value_text(c: &Compound) -> String {
    match c.atoms.first() {
        Some(Atom::Inline { text, .. }) => text.clone(),
        Some(Atom::Source { text }) => text.clone(),
        Some(Atom::Literal { text, .. }) => text.clone(),
        None => String::new(),
    }
}

pub(crate) fn atom_text(a: &Atom) -> String {
    match a {
        Atom::Inline { text, .. } => text.clone(),
        Atom::Source { text } => text.clone(),
        Atom::Literal { text, .. } => text.clone(),
    }
}

// ── Schema validity checking (§20.1) ────────────────────────────────────────

/// A schema-validity error carries only an `ErrorCode` and a human-readable
/// detail; spans are not meaningful for in-memory schemas.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub code: ErrorCode,
    pub detail: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

/// Check a `Schema` for validity per §20.1 and §20.3. Returns the full list
/// of E2xx errors (no recovery; every constraint violation is reported).
// ── Schema construction from semantic model (§20.6) ────────────────────────

/// Walk a parsed `Document` (which is presumed to be a schema document, i.e.
/// already type-checked against the built-in tel-schema) and build a `Schema`
/// value. The construction is deterministic per §20.6; source order is
/// preserved in all `Vec`s.
///
/// This function does NOT re-check tel-schema conformance — call `type_assign`
/// against `builtin_tel_schema()` first if you need that.
pub fn construct_schema(doc: &Document) -> Schema {
    let mut name = String::new();
    let mut sigil: Option<char> = None;
    let mut types: Vec<Definition> = Vec::new();
    let mut scalars: Vec<ScalarDefinition> = Vec::new();
    let mut layers: Vec<Layer> = Vec::new();
    let mut document = Struct { members: Vec::new(), validators: Vec::new() };

    for block in &doc.children {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "name" => name = scalar_value_text(c),
                "sigil" => sigil = scalar_value_text(c).chars().next(),
                "record" => types.push(construct_record(c)),
                "scalar" => scalars.push(construct_scalar_definition(c)),
                "document" => {
                    let (members, validators) = construct_members_and_validators(&c.children);
                    document = Struct { members, validators };
                }
                "layer" => layers.push(construct_layer(c)),
                _ => { /* unknown — type-assignment would have caught it */ }
            }
        }
    }

    Schema { name, document, layers, sigil, types, scalars }
}

fn construct_record(c: &Compound) -> Definition {
    // The `record` compound's first inline atom is the name.
    let name = first_inline_atom(c);
    let (members, validators) = construct_members_and_validators(&c.children);
    Definition { name, members, validators }
}

fn construct_scalar_definition(c: &Compound) -> ScalarDefinition {
    // `scalar <name>` with one or more `validate <name>` children.
    let name = first_inline_atom(c);
    let mut validators: Vec<String> = Vec::new();
    for block in &c.children {
        for child in &block.compounds {
            if child.keyword == "validate" {
                validators.push(scalar_value_text(child));
            }
        }
    }
    ScalarDefinition { name, validators }
}

/// Return the text of `c`'s first inline atom, or the empty string. Helper for
/// extracting the keyword/name from compounds whose first atom is the name.
fn first_inline_atom(c: &Compound) -> String {
    c.atoms.first().map(atom_text).unwrap_or_default()
}

fn construct_layer(c: &Compound) -> Layer {
    let mut name = String::new();
    let mut overlay = Struct { members: Vec::new(), validators: Vec::new() };
    let mut types: Vec<Definition> = Vec::new();
    let mut scalars: Vec<ScalarDefinition> = Vec::new();
    // First inline atom (if present) is the layer name.
    if let Some(atom) = c.atoms.first() {
        name = atom_text(atom);
    }
    // Children: `name` / `overlay` / `record` / `scalar` (per layer-body schema).
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "name" => name = scalar_value_text(child),
                "overlay" => {
                    let (members, validators) = construct_members_and_validators(&child.children);
                    overlay = Struct { members, validators };
                }
                "record" => types.push(construct_record(child)),
                "scalar" => scalars.push(construct_scalar_definition(child)),
                _ => {}
            }
        }
    }
    Layer { name, overlay, types, scalars }
}

/// Walk the children of a Struct-shaped compound and collect Members.
fn construct_members(blocks: &[Block]) -> Vec<Member> {
    construct_members_and_validators(blocks).0
}

/// Walk the children of a Struct-shaped compound and collect both its
/// Members (`field`/`select`/`exclude`) and its struct-level validators
/// (`validate K` lines). The two are returned separately because they
/// live in different fields of the `Struct` model.
fn construct_members_and_validators(blocks: &[Block]) -> (Vec<Member>, Vec<String>) {
    let mut members = Vec::new();
    let mut validators = Vec::new();
    for block in blocks {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "field" => members.push(Member::Field(construct_field(c))),
                "select" => members.push(Member::Select(construct_select(c))),
                "exclude" => {
                    // `exclude K` — K is the inline scalar atom (the variant
                    // keyword to exclude). This is valid in a Layer's root or
                    // any nested Struct inside a Layer; appearing in a base
                    // schema's document or definitions is checked by
                    // validate_schema (E212).
                    members.push(Member::Exclude(scalar_value_text(c)));
                }
                "validate" => {
                    // `validate K` — K is the validator name (§21.6).
                    validators.push(scalar_value_text(c));
                }
                _ => {}
            }
        }
    }
    (members, validators)
}

fn construct_field(c: &Compound) -> Field {
    // Atom phase against field-body's member order (§20.5):
    //   keyword (Scalar id), optional/required/repeatable/irrepeatable
    //   (Flags), type (Scalar id), default (Scalar string).
    //
    // Per §20.6, the four loosen/tighten flags combine into the internal
    // (required, repeatable) booleans per the rules:
    //   required   = required-flag-present  OR  NOT optional-flag-present
    //   repeatable = repeatable-flag-present AND NOT irrepeatable-flag-present
    // i.e. the tightening flag wins when both directions are asserted.
    let mut optional_flag = false;
    let mut required_flag = false;
    let mut repeatable_flag = false;
    let mut irrepeatable_flag = false;
    let mut keyword = String::new();
    let mut type_name = String::new();
    let mut default: Option<String> = None;
    // First atom = keyword.
    let mut iter = c.atoms.iter();
    if let Some(a) = iter.next() {
        keyword = atom_text(a);
    }
    // Remaining atoms: flags first (matched by keyword), then type-name
    // (first non-flag atom), then optional default (next atom).
    for a in iter {
        let t = atom_text(a);
        match t.as_str() {
            "optional" => optional_flag = true,
            "required" => required_flag = true,
            "repeatable" => repeatable_flag = true,
            "irrepeatable" => irrepeatable_flag = true,
            _ => {
                if type_name.is_empty() {
                    type_name = t;
                } else if default.is_none() {
                    default = Some(t);
                }
            }
        }
    }
    // Child compounds may override or supply fields.
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "keyword" => keyword = scalar_value_text(child),
                "optional" => optional_flag = true,
                "required" => required_flag = true,
                "repeatable" => repeatable_flag = true,
                "irrepeatable" => irrepeatable_flag = true,
                "type" => type_name = scalar_value_text(child),
                "default" => default = Some(scalar_value_text(child)),
                _ => {}
            }
        }
    }
    let required = required_flag || !optional_flag;
    let repeatable = repeatable_flag && !irrepeatable_flag;
    let r#type = Type::Reference(type_name);
    Field { required, repeatable, keyword, r#type, default }
}

fn construct_select(c: &Compound) -> Select {
    let mut optional_flag = false;
    let mut required_flag = false;
    let mut repeatable_flag = false;
    let mut irrepeatable_flag = false;
    for a in &c.atoms {
        match atom_text(a).as_str() {
            "optional" => optional_flag = true,
            "required" => required_flag = true,
            "repeatable" => repeatable_flag = true,
            "irrepeatable" => irrepeatable_flag = true,
            _ => {}
        }
    }
    let mut variants: Vec<Variant> = Vec::new();
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "optional" => optional_flag = true,
                "required" => required_flag = true,
                "repeatable" => repeatable_flag = true,
                "irrepeatable" => irrepeatable_flag = true,
                "variant" => variants.push(construct_variant(child)),
                _ => {}
            }
        }
    }
    let required = required_flag || !optional_flag;
    let repeatable = repeatable_flag && !irrepeatable_flag;
    Select { required, repeatable, variants }
}

fn construct_variant(c: &Compound) -> Variant {
    // Atom phase: keyword, then type-name.
    let mut keyword = String::new();
    let mut type_name = String::new();
    let mut iter = c.atoms.iter();
    if let Some(a) = iter.next() {
        keyword = atom_text(a);
    }
    if let Some(a) = iter.next() {
        type_name = atom_text(a);
    }
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "keyword" => keyword = scalar_value_text(child),
                "type" => type_name = scalar_value_text(child),
                _ => {}
            }
        }
    }
    Variant { keyword, r#type: Type::Reference(type_name) }
}

/// Build a `Type` from a type-variant child compound (`struct`, `scalar`,
/// `flag`, or `type`).
// (The old `construct_type` helper that built a Type from a `struct`/`scalar`/
// `flag`/`type` child compound is removed in v1.0: every Field/Variant type
// is now an inline `Reference` to a name in the composed namespace.)

pub fn validate_schema(s: &Schema) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    // E211: duplicate definition names in the BASE schema, across both
    // records (Schema.types) and scalars (Schema.scalars), since they
    // share a single namespace (§20.1). Same-name records across layers
    // merge per §20.3 and do not trigger E211; within a single layer's
    // own definitions, duplicates ARE an error (a layer cannot merge
    // with itself).
    let mut seen_base: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for d in &s.types {
        if !seen_base.insert(&d.name) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("duplicate definition name `{}` in base schema", d.name),
            });
        }
    }
    for sd in &s.scalars {
        if !seen_base.insert(&sd.name) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("duplicate definition name `{}` in base schema", sd.name),
            });
        }
        // Built-in name collision: user definitions MAY NOT redefine the
        // predefined names `flag`, `string`, `identifier`, `sigil`.
        if BUILTIN_TYPE_NAMES.contains(&sd.name.as_str()) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("scalar `{}` collides with a built-in type name", sd.name),
            });
        }
    }
    for d in &s.types {
        if BUILTIN_TYPE_NAMES.contains(&d.name.as_str()) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("record `{}` collides with a built-in type name", d.name),
            });
        }
    }
    for layer in &s.layers {
        let mut seen_in_layer: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for d in &layer.types {
            if !seen_in_layer.insert(&d.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!(
                        "duplicate definition name `{}` within layer `{}`",
                        d.name, layer.name
                    ),
                });
            }
        }
        for sd in &layer.scalars {
            if !seen_in_layer.insert(&sd.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!(
                        "duplicate definition name `{}` within layer `{}`",
                        sd.name, layer.name
                    ),
                });
            }
        }
    }
    // Compose the namespace: base types first, then each layer's
    // types in order. Definitions with shared names will be merged
    // by compose_schema; here we just gather all names that exist
    // (including scalar definitions and built-in names) so References
    // can be resolved.
    let mut def_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for n in BUILTIN_TYPE_NAMES { def_names.insert(*n); }
    let mut all_defs: Vec<&Definition> = s.types.iter().collect();
    for d in &s.types { def_names.insert(d.name.as_str()); }
    for sd in &s.scalars { def_names.insert(sd.name.as_str()); }
    for layer in &s.layers {
        all_defs.extend(layer.types.iter());
        for d in &layer.types { def_names.insert(d.name.as_str()); }
        for sd in &layer.scalars { def_names.insert(sd.name.as_str()); }
    }

    // E205: duplicate layer names
    let mut seen_layer_names = std::collections::HashSet::new();
    for l in &s.layers {
        if !seen_layer_names.insert(&l.name) {
            errors.push(SchemaError {
                code: ErrorCode::E205,
                detail: format!("duplicate Layer name `{}`", l.name),
            });
        }
    }

    // E208: sigil character check
    if let Some(c) = s.sigil {
        if matches!(validate_sigil(&c.to_string()), ValidationResponse::Invalid(_)) {
            errors.push(SchemaError {
                code: ErrorCode::E208,
                detail: format!("Schema.sigil `{}` is not a permitted sigil character", c),
            });
        }
    }

    // Walk every Struct in the schema (document, each Definition, every nested
    // Struct inside any Type) and check the per-Struct constraints.
    let mut to_visit: Vec<&Struct> = Vec::new();
    to_visit.push(&s.document);
    for d in &all_defs {
        check_members_recursive(&d.members, &def_names, &mut errors);
    }
    while let Some(st) = to_visit.pop() {
        check_members_recursive(&st.members, &def_names, &mut errors);
        for m in &st.members {
            collect_inner_structs(member_types(m), &mut to_visit);
        }
    }
    for l in &s.layers {
        check_members_recursive(&l.overlay.members, &def_names, &mut errors);
    }

    // E217: Exclude is layer-only (§20.3). An `exclude` appearing in
    // Schema.document or in any Schema.types Definition body — including
    // nested Structs within their members' types — is a validity error.
    // Layers may freely contain `exclude` operations in their root and in
    // any nested Struct under that root.
    check_no_exclude(&s.document.members, "document", &mut errors);
    for d in &s.types {
        check_no_exclude(&d.members, &format!("define `{}`", d.name), &mut errors);
    }

    // E206/E207: layer-merge constraints (§20.3).
    // Run a simulation of the Merge algorithm to detect keyword overlaps that
    // violate the layer extension rules.
    let mut composed_keywords: std::collections::HashMap<String, MergeKind> =
        std::collections::HashMap::new();
    for m in &s.document.members {
        match m {
            Member::Field(f) => {
                composed_keywords.insert(f.keyword.clone(),
                    MergeKind::Field(is_struct_type(&f.r#type)));
            }
            Member::Select(sel) => {
                for v in &sel.variants {
                    composed_keywords.insert(v.keyword.clone(), MergeKind::Variant);
                }
            }
            Member::Exclude(name) => {
                // Remove operations are invalid in a base schema's document.
                errors.push(SchemaError {
                    code: ErrorCode::E212,
                    detail: format!(
                        "`exclude {}` appears in the base schema's document; exclude operations are layer-only",
                        name
                    ),
                });
            }
        }
    }
    for layer in &s.layers {
        for m in &layer.overlay.members {
            match m {
                Member::Exclude(_) => {
                    // Detailed exclude validation happens at compose time
                    // (E212, E213). validate_schema only checks for the
                    // structural shape.
                }
                Member::Field(f) => {
                    if let Some(existing) = composed_keywords.get(&f.keyword) {
                        // E207: existing must be a Field whose type is Struct,
                        // and the layer's type must also be Struct.
                        match existing {
                            MergeKind::Field(base_is_struct) => {
                                let layer_is_struct = is_struct_type(&f.r#type);
                                if !base_is_struct || !layer_is_struct {
                                    errors.push(SchemaError {
                                        code: ErrorCode::E207,
                                        detail: format!(
                                            "layer `{}` overrides keyword `{}` but Field merge requires both base and layer types to be Struct",
                                            layer.name, f.keyword,
                                        ),
                                    });
                                }
                            }
                            MergeKind::Variant => {
                                errors.push(SchemaError {
                                    code: ErrorCode::E207,
                                    detail: format!(
                                        "layer `{}` adds Field `{}` but base member with that keyword is a Select variant, not a Field",
                                        layer.name, f.keyword,
                                    ),
                                });
                            }
                        }
                    } else {
                        composed_keywords.insert(f.keyword.clone(),
                            MergeKind::Field(is_struct_type(&f.r#type)));
                    }
                }
                Member::Select(sel) => {
                    for v in &sel.variants {
                        if composed_keywords.contains_key(&v.keyword) {
                            errors.push(SchemaError {
                                code: ErrorCode::E206,
                                detail: format!(
                                    "layer `{}` adds Select variant `{}` which collides with an existing keyword in the composed schema",
                                    layer.name, v.keyword,
                                ),
                            });
                        } else {
                            composed_keywords.insert(v.keyword.clone(), MergeKind::Variant);
                        }
                    }
                }
            }
        }
    }

    // Run the full composition algorithm to surface any merge-time errors
    // (E207 due to type mismatch, E212/E213 for excludes, etc.) that the
    // naive simulation above does not catch — particularly Reference vs.
    // Reference mismatches.
    if !s.layers.is_empty() {
        let (_, compose_errs) = compose_schema(s);
        errors.extend(compose_errs);
    }

    errors
}

// ── Schema composition (§20.3) ───────────────────────────────────────────────

/// Apply every `Layer` in `s.layers` to produce a fully composed schema
/// per §20.3. Returns the composed `Schema` (with empty `layers`) plus
/// any `SchemaError`s raised during composition (E206, E207, E212, E213,
/// E214). The returned schema is always a best-effort result: layers
/// whose operations produced errors are merged as far as possible so
/// callers can continue to inspect the composed structure.
pub fn compose_schema(s: &Schema) -> (Schema, Vec<SchemaError>) {
    let mut errors: Vec<SchemaError> = Vec::new();
    let mut composed_types: Vec<Definition> = s.types.clone();
    let mut composed_scalars: Vec<ScalarDefinition> = s.scalars.clone();
    let mut composed_root_members: Vec<Member> = s.document.members.clone();
    let mut composed_root_validators: Vec<String> = s.document.validators.clone();

    for layer in &s.layers {
        // Record merge: same-name record → merge members and validators;
        // otherwise → append. Collision with an existing scalar of the
        // same name is a kind mismatch.
        for def in &layer.types {
            if composed_scalars.iter().any(|sd| sd.name == def.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E207,
                    detail: format!(
                        "layer `{}` declares record `{}` but a scalar with the same name already exists",
                        layer.name, def.name,
                    ),
                });
                continue;
            }
            if let Some(pos) = composed_types.iter().position(|d| d.name == def.name) {
                let merged = merge_members(
                    &composed_types[pos].members,
                    &def.members,
                    &layer.name,
                    &format!("record `{}`", def.name),
                    &mut errors,
                );
                let merged_validators = merge_validators(
                    &composed_types[pos].validators,
                    &def.validators,
                );
                composed_types[pos] = Definition {
                    name: def.name.clone(),
                    members: merged,
                    validators: merged_validators,
                };
            } else {
                composed_types.push(def.clone());
            }
        }

        // Scalar merge: same-name scalar → append-deduplicate validators;
        // otherwise → append. Collision with an existing record is a
        // kind mismatch.
        for sd in &layer.scalars {
            if composed_types.iter().any(|d| d.name == sd.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E207,
                    detail: format!(
                        "layer `{}` declares scalar `{}` but a record with the same name already exists",
                        layer.name, sd.name,
                    ),
                });
                continue;
            }
            if let Some(pos) = composed_scalars.iter().position(|x| x.name == sd.name) {
                let merged = merge_validators(
                    &composed_scalars[pos].validators,
                    &sd.validators,
                );
                composed_scalars[pos] = ScalarDefinition {
                    name: sd.name.clone(),
                    validators: merged,
                };
            } else {
                composed_scalars.push(sd.clone());
            }
        }

        // Overlay merge: members merge as before; validators concatenate.
        composed_root_members = merge_members(
            &composed_root_members,
            &layer.overlay.members,
            &layer.name,
            "overlay",
            &mut errors,
        );
        composed_root_validators = merge_validators(
            &composed_root_validators,
            &layer.overlay.validators,
        );
    }

    (Schema {
        name: s.name.clone(),
        document: Struct {
            members: composed_root_members,
            validators: composed_root_validators,
        },
        layers: Vec::new(),
        sigil: s.sigil,
        types: composed_types,
        scalars: composed_scalars,
    }, errors)
}

/// Merge a layer's Field into an existing base Field with the same keyword
/// (§20.3). Returns `Some(new_field)` if the merge is valid, or `None` and
/// pushes a SchemaError if it is not.
///
/// Rules:
/// - **Types** must be structurally compatible: both Struct (recursive
///   merge) or otherwise structurally equal. A type mismatch is E207.
/// - **`required`** may be tightened by the layer (`false → true`) but
///   not loosened (`true → false` is E215).
/// - **`repeatable`** may be tightened by the layer (`true → false`) but
///   not loosened (`false → true` is E216).
fn merge_field_with(
    base: &Field,
    layer: &Field,
    layer_name: &str,
    where_: &str,
    errors: &mut Vec<SchemaError>,
) -> Option<Field> {
    // Required: tightening is base=false ∧ layer=true. Loosening is base=true ∧ layer=false.
    let merged_required = match (base.required, layer.required) {
        (true, false) => {
            errors.push(SchemaError {
                code: ErrorCode::E215,
                detail: format!(
                    "layer `{}` field `{}` in {}: required cannot be loosened to optional",
                    layer_name, layer.keyword, where_,
                ),
            });
            true
        }
        (b, l) => b || l,
    };
    // Repeatable: tightening is base=true ∧ layer=false. Loosening is base=false ∧ layer=true.
    let merged_repeatable = match (base.repeatable, layer.repeatable) {
        (false, true) => {
            errors.push(SchemaError {
                code: ErrorCode::E216,
                detail: format!(
                    "layer `{}` field `{}` in {}: irrepeatable cannot be loosened to repeatable",
                    layer_name, layer.keyword, where_,
                ),
            });
            false
        }
        (b, l) => b && l,
    };
    // Type merge.
    let merged_type = match (&base.r#type, &layer.r#type) {
        (Type::Struct(gs), Type::Struct(fs)) => {
            let merged_inner = merge_members(
                &gs.members, &fs.members,
                layer_name,
                &format!("{} → field `{}`", where_, layer.keyword),
                errors,
            );
            let merged_inner_validators = merge_validators(&gs.validators, &fs.validators);
            Type::Struct(Struct {
                members: merged_inner,
                validators: merged_inner_validators,
            })
        }
        (a, b) if a == b => a.clone(),
        _ => {
            errors.push(SchemaError {
                code: ErrorCode::E207,
                detail: format!(
                    "layer `{}` field `{}` in {}: type mismatch (base and layer must declare the same type, or both be Struct)",
                    layer_name, layer.keyword, where_,
                ),
            });
            base.r#type.clone()
        }
    };
    Some(Field {
        required: merged_required,
        repeatable: merged_repeatable,
        keyword: base.keyword.clone(),
        r#type: merged_type, default: None,
    })
}

/// Append-and-deduplicate merge of two validator lists, per §20.3. The base
/// list keeps its order; layer entries not already present are appended in
/// source order.
fn merge_validators(base: &[String], layer: &[String]) -> Vec<String> {
    let mut out: Vec<String> = base.to_vec();
    for v in layer {
        if !out.iter().any(|b| b == v) {
            out.push(v.clone());
        }
    }
    out
}

/// Merge a layer's member operations into a base member list. Implements
/// the inner loop of §20.3's MergeStruct algorithm.
fn merge_members(
    base: &[Member],
    layer_ops: &[Member],
    layer_name: &str,
    where_: &str,
    errors: &mut Vec<SchemaError>,
) -> Vec<Member> {
    let mut merged: Vec<Member> = base.to_vec();
    for op in layer_ops {
        match op {
            Member::Field(f) => {
                // Look up f.keyword in merged.
                let existing_idx = merged.iter().position(|m| match m {
                    Member::Field(g) => g.keyword == f.keyword,
                    Member::Select(s) => s.variants.iter().any(|v| v.keyword == f.keyword),
                    Member::Exclude(_) => false,
                });
                match existing_idx {
                    Some(idx) => match &merged[idx] {
                        Member::Field(g) => {
                            // Merge: types must be structurally equal (recursive
                            // Struct merge is supported and produces a subtype);
                            // required/repeatable may be tightened by the layer
                            // but not loosened.
                            let merged_field = merge_field_with(
                                g, f, layer_name, where_, errors,
                            );
                            if let Some(field) = merged_field {
                                merged[idx] = Member::Field(field);
                            }
                        }
                        Member::Select(_) => {
                            errors.push(SchemaError {
                                code: ErrorCode::E207,
                                detail: format!(
                                    "layer `{}` field `{}` in {}: keyword already declared as a Select variant in the base",
                                    layer_name, f.keyword, where_,
                                ),
                            });
                        }
                        Member::Exclude(_) => unreachable!(),
                    },
                    None => {
                        merged.push(Member::Field(f.clone()));
                    }
                }
            }
            Member::Select(s) => {
                // Every variant keyword MUST NOT exist in merged.
                let mut had_overlap = false;
                for v in &s.variants {
                    let collides = merged.iter().any(|m| match m {
                        Member::Field(g) => g.keyword == v.keyword,
                        Member::Select(ms) => ms.variants.iter().any(|mv| mv.keyword == v.keyword),
                        Member::Exclude(_) => false,
                    });
                    if collides {
                        had_overlap = true;
                        let owned_by_select = merged.iter().any(|m| matches!(m,
                            Member::Select(ms) if ms.variants.iter().any(|mv| mv.keyword == v.keyword)));
                        if owned_by_select {
                            errors.push(SchemaError {
                                code: ErrorCode::E214,
                                detail: format!(
                                    "layer `{}` Select variant `{}` in {} would widen an existing Select (subtyping requires removing variants, not adding to an existing Select)",
                                    layer_name, v.keyword, where_,
                                ),
                            });
                        } else {
                            errors.push(SchemaError {
                                code: ErrorCode::E206,
                                detail: format!(
                                    "layer `{}` Select variant `{}` in {} collides with an existing Field keyword",
                                    layer_name, v.keyword, where_,
                                ),
                            });
                        }
                    }
                }
                if !had_overlap {
                    merged.push(Member::Select(s.clone()));
                }
            }
            Member::Exclude(kw_to_exclude) => {
                // Find the Select in merged that owns kw_to_exclude.
                let mut found_in: Option<usize> = None;
                for (i, m) in merged.iter().enumerate() {
                    if let Member::Select(ms) = m {
                        if ms.variants.iter().any(|v| v.keyword == *kw_to_exclude) {
                            found_in = Some(i);
                            break;
                        }
                    }
                }
                match found_in {
                    None => {
                        errors.push(SchemaError {
                            code: ErrorCode::E212,
                            detail: format!(
                                "layer `{}` exclude `{}` in {}: no Select variant with that keyword exists in the merged Struct",
                                layer_name, kw_to_exclude, where_,
                            ),
                        });
                    }
                    Some(idx) => {
                        if let Member::Select(ms) = &mut merged[idx] {
                            ms.variants.retain(|v| v.keyword != *kw_to_exclude);
                            if ms.variants.is_empty() {
                                if ms.required {
                                    errors.push(SchemaError {
                                        code: ErrorCode::E213,
                                        detail: format!(
                                            "layer `{}` exclude `{}` in {}: would leave a required Select with no variants",
                                            layer_name, kw_to_exclude, where_,
                                        ),
                                    });
                                } else {
                                    merged.remove(idx);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    merged
}

/// Helper enum for tracking the kind of merged keyword during layer composition.
enum MergeKind { Field(bool /* type is Struct */), Variant }

fn is_struct_type(t: &Type) -> bool {
    matches!(t, Type::Struct(_) | Type::Reference(_))
    // A Reference to a Definition always resolves to a Struct (per §20).
}

/// Return all `Type`s reachable directly inside a Member (one per Field, or
/// one per Variant of a Select).
fn member_types(m: &Member) -> Vec<&Type> {
    match m {
        Member::Field(f) => vec![&f.r#type],
        Member::Select(s) => s.variants.iter().map(|v| &v.r#type).collect(),
        Member::Exclude(_) => Vec::new(),
    }
}

fn collect_inner_structs<'a>(types: Vec<&'a Type>, out: &mut Vec<&'a Struct>) {
    for t in types {
        if let Type::Struct(st) = t { out.push(st); }
    }
}

/// E217 (§20.3): recursively scan a member list and any nested Struct types
/// for `Member::Exclude` and report each as an error. Called with the document
/// root's members and each base Definition's members; layer roots are NOT
/// scanned (they are the only legitimate site for `exclude`).
fn check_no_exclude(members: &[Member], where_: &str, errors: &mut Vec<SchemaError>) {
    for m in members {
        match m {
            Member::Exclude(kw) => {
                errors.push(SchemaError {
                    code: ErrorCode::E217,
                    detail: format!(
                        "`exclude {}` appears in {} but is permitted only inside a layer's root",
                        kw, where_,
                    ),
                });
            }
            Member::Field(f) => {
                if let Type::Struct(st) = &f.r#type {
                    check_no_exclude(
                        &st.members,
                        &format!("{} → field `{}`", where_, f.keyword),
                        errors,
                    );
                }
            }
            Member::Select(s) => {
                for v in &s.variants {
                    if let Type::Struct(st) = &v.r#type {
                        check_no_exclude(
                            &st.members,
                            &format!("{} → variant `{}`", where_, v.keyword),
                            errors,
                        );
                    }
                }
            }
        }
    }
}

fn check_members_recursive(
    members: &[Member],
    def_names: &std::collections::HashSet<&str>,
    errors: &mut Vec<SchemaError>,
) {
    // E201: duplicate keyword within a Struct (across Field and Sum variant keywords)
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for m in members {
        let kws: Vec<&str> = match m {
            Member::Field(f) => vec![&f.keyword],
            Member::Select(s) => s.variants.iter().map(|v| v.keyword.as_str()).collect(),
            Member::Exclude(_) => Vec::new(),
        };
        for kw in &kws {
            if !seen.insert(*kw) {
                errors.push(SchemaError {
                    code: ErrorCode::E201,
                    detail: format!("duplicate keyword `{}` within a Struct", kw),
                });
            }
            // E209: reserved keyword `tel`
            if *kw == "tel" {
                errors.push(SchemaError {
                    code: ErrorCode::E209,
                    detail: "keyword `tel` is reserved (§8)".to_string(),
                });
            }
        }

        // E202: empty Select variants list
        if let Member::Select(s) = m {
            if s.variants.is_empty() {
                errors.push(SchemaError {
                    code: ErrorCode::E202,
                    detail: "Select member has empty variants list".to_string(),
                });
            }
        }

        // E204: Field.default is only valid when the field is required AND
        // its resolved type is a Scalar. (In v1.0 the default lives on the
        // enclosing Field, not on the Scalar value-type.)
        let (is_required, types_to_check): (bool, Vec<&Type>) = match m {
            Member::Field(f) => (f.required, vec![&f.r#type]),
            Member::Select(s) => (s.required, s.variants.iter().map(|v| &v.r#type).collect()),
            Member::Exclude(_) => (false, Vec::new()),
        };
        if let Member::Field(f) = m {
            if let Some(def_val) = &f.default {
                let resolves_to_scalar = matches!(&f.r#type,
                    Type::Scalar(_) | Type::Reference(_));
                if !is_required {
                    errors.push(SchemaError {
                        code: ErrorCode::E204,
                        detail: format!(
                            "Field `{}` has default `{}` but is not required",
                            f.keyword, def_val,
                        ),
                    });
                } else if !resolves_to_scalar {
                    errors.push(SchemaError {
                        code: ErrorCode::E204,
                        detail: format!(
                            "Field `{}` has default `{}` but its type is not Scalar",
                            f.keyword, def_val,
                        ),
                    });
                }
            }
        }

        // E210: Reference name must resolve
        for t in types_to_check {
            if let Type::Reference(n) = t {
                if !def_names.contains(n.as_str()) {
                    errors.push(SchemaError {
                        code: ErrorCode::E210,
                        detail: format!("Reference `{}` does not resolve to any Definition", n),
                    });
                }
            }
        }

        // Recurse into nested Struct types
        for t in member_types(m) {
            if let Type::Struct(st) = t {
                check_members_recursive(&st.members, def_names, errors);
            }
        }
    }
}

// ── Display ─────────────────────────────────────────────────────────────────

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#?}", self)
    }
}

// ── Raw line ────────────────────────────────────────────────────────────────

/// A physical line from the source.
#[derive(Debug, Clone)]
struct RawLine {
    /// Char offset where this line starts in the source.
    start: usize,
    /// Characters on this line (excluding CR/LF).
    chars: Vec<char>,
}

impl RawLine {
    fn is_blank(&self) -> bool { self.chars.iter().all(|&c| c == ' ') }
    fn text(&self) -> String { self.chars.iter().collect() }
}

// ── Parser ──────────────────────────────────────────────────────────────────

pub struct ParseResult {
    pub document: Document,
    pub errors: Vec<TelError>,
}

pub fn parse(input: &str) -> ParseResult {
    let mut p = ParserState::new(input);
    let doc = p.run();
    ParseResult { document: doc, errors: p.errors }
}

struct ParserState {
    all_chars: Vec<char>,
    errors: Vec<TelError>,
    sigil: char,
    margin: usize,
}

impl ParserState {
    fn new(input: &str) -> Self {
        ParserState {
            all_chars: input.chars().collect(),
            errors: Vec::new(),
            sigil: '#',
            margin: 0,
        }
    }

    fn run(&mut self) -> Document {
        let mut start = 0;

        // E101: BOM
        if self.all_chars.first() == Some(&'\u{FEFF}') {
            self.errors.push(TelError::new(ErrorCode::E101, 0, 1));
            start = 1;
        }

        // Detect line endings and check E121
        let line_endings = self.detect_line_endings(start);

        // Split into raw lines
        let raw_lines = self.split_lines(start);

        // Parse interpreter directive
        let mut line_idx = 0;
        let mut interpreter_directive = None;
        if !raw_lines.is_empty() && raw_lines[0].chars.len() >= 2
            && raw_lines[0].chars[0] == '#' && raw_lines[0].chars[1] == '!'
        {
            interpreter_directive = Some(raw_lines[0].chars[2..].iter().collect());
            self.margin = 0;
            line_idx = 1;
        }

        // Skip blank lines, find pragma or first content
        let first_nb = raw_lines[line_idx..].iter().position(|l| !l.is_blank()).map(|i| i + line_idx);
        let mut pragma = None;

        if let Some(fi) = first_nb {
            let text = raw_lines[fi].text();
            let trimmed = text.trim_start();
            if trimmed == "tel" || trimmed.starts_with("tel ") {
                // Check E103
                let byte_end: usize = self.all_chars[..raw_lines[fi].start + raw_lines[fi].chars.len()]
                    .iter().collect::<String>().len();
                if byte_end > 4096 {
                    self.errors.push(TelError::new(
                        ErrorCode::E103,
                        raw_lines[fi].start,
                        raw_lines[fi].start + raw_lines[fi].chars.len(),
                    ));
                }
                // Check E102 - is it the first non-blank after directive?
                // There shouldn't be non-blank lines between line_idx and fi
                if raw_lines[line_idx..fi].iter().any(|l| !l.is_blank()) {
                    self.errors.push(TelError::new(
                        ErrorCode::E102, raw_lines[fi].start, raw_lines[fi].start + 3,
                    ));
                }
                pragma = Some(self.parse_pragma(trimmed, raw_lines[fi].start));
                if let Some(ref pr) = pragma {
                    if let Some(s) = pr.sigil { self.sigil = s; }
                }
                line_idx = fi + 1;
            }
        }

        // Check E102: if pragma wasn't found at first non-blank, scan for misplaced pragma
        if pragma.is_none() {
            for rl in &raw_lines[line_idx..] {
                if rl.is_blank() { continue; }
                let t = rl.text();
                let tr = t.trim_start();
                if tr == "tel" || tr.starts_with("tel ") {
                    self.errors.push(TelError::new(ErrorCode::E102, rl.start, rl.start + 3));
                    break;
                }
            }
        }

        // Determine margin
        if interpreter_directive.is_none() {
            let search_start = line_idx;
            if let Some(fi) = raw_lines[search_start..].iter().position(|l| !l.is_blank()) {
                let line = &raw_lines[fi + search_start];
                self.margin = line.chars.iter().take_while(|&&c| c == ' ').count();
            }
        }

        // Build the tree from remaining lines
        let children = self.build_tree(&raw_lines, line_idx);

        Document { interpreter_directive, pragma, line_endings, children }
    }

    fn detect_line_endings(&mut self, start: usize) -> LineEndings {
        let chars = &self.all_chars;

        // Find literal atom payload ranges to skip (rough pre-scan)
        let literal_ranges = self.find_literal_ranges(start);

        let in_literal = |pos: usize| -> bool {
            literal_ranges.iter().any(|&(s, e)| pos >= s && pos < e)
        };

        let mut mode = LineEndings::LF;
        let mut established = false;

        for i in start..chars.len() {
            if in_literal(i) { continue; }
            if chars[i] == '\n' {
                if i > start && chars[i - 1] == '\r' {
                    mode = LineEndings::CRLF;
                }
                established = true;
                break;
            }
        }

        let mut i = start;
        while i < chars.len() {
            if in_literal(i) { i += 1; continue; }
            if chars[i] == '\r' {
                if i + 1 >= chars.len() || chars[i + 1] != '\n' {
                    self.errors.push(TelError::with_detail(
                        ErrorCode::E121, i, i + 1, "CR not followed by LF",
                    ));
                } else if established && mode == LineEndings::LF {
                    self.errors.push(TelError::with_detail(
                        ErrorCode::E121, i, i + 2, "CRLF in LF-mode document",
                    ));
                }
                i += 2;
                continue;
            }
            if chars[i] == '\n' && established && mode == LineEndings::CRLF {
                if i == start || chars[i - 1] != '\r' {
                    self.errors.push(TelError::with_detail(
                        ErrorCode::E121, i, i + 1, "bare LF in CRLF-mode document",
                    ));
                }
            }
            i += 1;
        }

        mode
    }

    /// Rough pre-scan to find literal atom payload char ranges (start, end).
    fn find_literal_ranges(&self, start: usize) -> Vec<(usize, usize)> {
        let chars = &self.all_chars;
        let mut ranges = Vec::new();
        // Very simple heuristic: look for lines that are heavily indented (6+ spaces from margin)
        // and have non-whitespace content that could be a delimiter, then scan for closing.
        // This is a rough scan — we just need to avoid false E121 inside literal payloads.

        let mut i = start;
        while i < chars.len() {
            // Find a LF
            if chars[i] == '\n' && i + 1 < chars.len() {
                // Check if next line is deeply indented (potential literal delimiter)
                let line_start = i + 1;
                let mut spaces = 0;
                let mut j = line_start;
                while j < chars.len() && chars[j] == ' ' { spaces += 1; j += 1; }
                // Literal atoms are at indent+3 = 6+ spaces from margin
                // We need at least 6 spaces of indentation from margin=0
                if spaces >= 6 && j < chars.len() && chars[j] != '\n' {
                    // Potential delimiter line
                    let delim_start = j;
                    while j < chars.len() && chars[j] != '\n' { j += 1; }
                    let delimiter: String = chars[delim_start..j].iter().collect();
                    let delimiter = delimiter.trim_end().to_string();
                    if !delimiter.is_empty() && delimiter.chars().all(|c| !c.is_ascii_whitespace()) {
                        // Scan for closing delimiter
                        let payload_start = if j < chars.len() { j + 1 } else { j };
                        let mut k = payload_start;
                        while k < chars.len() {
                            // Find next LF
                            let ls = k;
                            while k < chars.len() && chars[k] != '\n' { k += 1; }
                            let line_text: String = chars[ls..k].iter().collect();
                            if line_text == delimiter {
                                ranges.push((payload_start, ls));
                                break;
                            }
                            if k < chars.len() { k += 1; }
                        }
                    }
                }
            }
            i += 1;
        }
        ranges
    }

    fn split_lines(&self, start: usize) -> Vec<RawLine> {
        let mut lines = Vec::new();
        let mut line_start = start;
        let mut i = start;
        while i <= self.all_chars.len() {
            if i == self.all_chars.len() || self.all_chars[i] == '\n' {
                let end = if i > line_start && self.all_chars.get(i.wrapping_sub(1)) == Some(&'\r') {
                    i - 1
                } else {
                    i
                };
                lines.push(RawLine {
                    start: line_start,
                    chars: self.all_chars[line_start..end].to_vec(),
                });
                line_start = i + 1;
            }
            i += 1;
        }
        lines
    }

    fn parse_pragma(&mut self, trimmed: &str, line_start: usize) -> Pragma {
        let after = if trimmed.len() > 4 { trimmed[4..].trim_start() } else { "" };
        let atoms: Vec<&str> = if after.is_empty() {
            vec![]
        } else {
            after.split_whitespace().collect()
        };

        // E123: extra atoms or remark
        if atoms.len() > 3 {
            self.errors.push(TelError::new(ErrorCode::E123, line_start, line_start + trimmed.len()));
        }

        let version = if !atoms.is_empty() {
            self.parse_version(atoms[0], line_start + 4)
        } else {
            self.errors.push(TelError::new(ErrorCode::E104, line_start, line_start + 3));
            (1, 0)
        };

        let schema = if atoms.len() >= 2 {
            let s = atoms[1];
            if !self.is_valid_schema_id(s) {
                self.errors.push(TelError::new(ErrorCode::E122, line_start, line_start + trimmed.len()));
            }
            Some(s.to_string())
        } else {
            None
        };

        let sigil = if atoms.len() >= 3 {
            let s = atoms[2];
            let ch = s.chars().next().unwrap_or(' ');
            if s.len() != 1 || ch.is_ascii_alphanumeric() || ch.is_ascii_control()
                || ch == ' ' || ch == '\n' || ch == '\r'
            {
                self.errors.push(TelError::new(ErrorCode::E105, line_start, line_start + trimmed.len()));
                None
            } else {
                Some(ch)
            }
        } else {
            None
        };

        Pragma { version, schema, sigil }
    }

    fn parse_version(&mut self, s: &str, offset: usize) -> (u32, u32) {
        if let Some(dot) = s.find('.') {
            let (maj_s, min_s) = (&s[..dot], &s[dot + 1..]);
            if let (Ok(maj), Ok(min)) = (maj_s.parse::<u32>(), min_s.parse::<u32>()) {
                if !s.contains('-') {
                    return (maj, min);
                }
            }
        }
        self.errors.push(TelError::with_detail(ErrorCode::E104, offset, offset + s.len(), s));
        (1, 0)
    }

    fn is_valid_schema_id(&self, s: &str) -> bool {
        if s.contains("://") { return true; }
        // Bare BASE-256-encoded schema signature: one Unicode character per
        // signature byte. A single-component (no-layer) signature is 32 bytes
        // → 32 characters; with `n` components, 30 + 2n bytes → 30 + 2n
        // characters. Length therefore MUST be ≥ 32 and `(length − 32)` MUST
        // be a non-negative even number. Every character MUST be a member of
        // the BASE-256 alphabet (validated by base256::decode_strict).
        let char_count = s.chars().count();
        if char_count < 32 || (char_count - 32) % 2 != 0 { return false; }
        crate::base256::decode_strict(s).is_ok()
    }

    // ── Tree builder ────────────────────────────────────────────────────────

    fn build_tree(&mut self, raw_lines: &[RawLine], start_idx: usize) -> Vec<Block> {
        let mut bld = TreeCtx {
            raw: raw_lines,
            idx: start_idx,
            margin: self.margin,
            sigil: self.sigil,
            errors: Vec::new(),
        };
        let blocks = bld.parse_blocks(-1); // -1 = accept indent 0
        self.errors.append(&mut bld.errors);
        blocks
    }
}

/// Tree-building context. Works directly on raw lines.
struct TreeCtx<'a> {
    raw: &'a [RawLine],
    idx: usize,
    margin: usize,
    sigil: char,
    errors: Vec<TelError>,
}

/// What kind of line is this?
#[derive(Debug)]
enum LineKind {
    Blank,
    Comment(String),
    Tabulation(Tabulation),
    Ordinary {
        keyword: String,
        atoms: Vec<Atom>,
        remark: Option<String>,
    },
}

impl<'a> TreeCtx<'a> {
    /// Get indent of raw line, or None if blank. Also checks E106/E107/E108.
    fn line_indent(&mut self, ri: usize) -> Option<usize> {
        let line = &self.raw[ri];
        if line.is_blank() { return None; }
        let chars = &line.chars;
        let margin = self.margin;

        // Check margin (E106)
        if chars.len() < margin {
            self.errors.push(TelError::with_detail(
                ErrorCode::E106, line.start, line.start + chars.len(), "line shorter than margin",
            ));
            return Some(0);
        }
        for i in 0..margin {
            if chars[i] != ' ' {
                self.errors.push(TelError::with_detail(
                    ErrorCode::E106, line.start, line.start + i + 1, "non-space within margin",
                ));
                return Some(0);
            }
        }

        let after = &chars[margin..];
        let spaces = after.iter().take_while(|&&c| c == ' ').count();
        if spaces % 2 != 0 {
            // E107: odd indentation. §19.5's shallower-wins rule: record E107
            // and treat the line as if its indent were ⌊spaces / 2⌋ (integer
            // division), the shallower of the two adjacent even levels.
            self.errors.push(TelError::new(ErrorCode::E107, line.start, line.start + margin + spaces));
        }
        Some(spaces / 2)
    }

    /// Get content after margin+indent for a non-blank line.
    fn content_after_indent(&self, ri: usize) -> &[char] {
        let chars = &self.raw[ri].chars;
        let margin = self.margin;
        if chars.len() <= margin { return &[]; }
        let after = &chars[margin..];
        let spaces = after.iter().take_while(|&&c| c == ' ').count();
        &after[spaces..]
    }

    /// Classify a non-blank line.
    fn classify(&mut self, ri: usize) -> LineKind {
        let content = self.content_after_indent(ri);
        if content.is_empty() { return LineKind::Blank; }

        let sigil = self.sigil;
        let content = content.to_vec(); // clone to release borrow

        // Check comment/tabulation
        if content[0] == sigil {
            // Tabulation: another sigil preceded by hard space
            if has_tab_markers(&content, sigil) {
                let indent_spaces = self.line_indent_spaces(ri);
                let line_start = self.raw[ri].start;
                let tab = parse_tabulation(&content, sigil, indent_spaces, &mut self.errors, line_start);
                return LineKind::Tabulation(tab);
            }
            // Comment
            if content.len() == 1 {
                return LineKind::Comment(String::new());
            }
            if content[1] == ' ' {
                let payload: String = content[2..].iter().collect();
                return LineKind::Comment(payload);
            }
            // #foo — ordinary keyword
        }

        // Ordinary line
        let keyword_end = content.iter().position(|&c| c == ' ').unwrap_or(content.len());
        let keyword: String = content[..keyword_end].iter().collect();
        if keyword_end >= content.len() {
            return LineKind::Ordinary { keyword, atoms: vec![], remark: None };
        }
        let rest = &content[keyword_end..];
        let (atoms, remark) = parse_atoms(rest, sigil);
        LineKind::Ordinary { keyword, atoms, remark }
    }

    fn line_indent_spaces(&self, ri: usize) -> usize {
        let chars = &self.raw[ri].chars;
        let margin = self.margin;
        if chars.len() <= margin { return 0; }
        chars[margin..].iter().take_while(|&&c| c == ' ').count()
    }

    /// Check trailing spaces (E108) on a non-blank ordinary line.
    fn check_trailing(&mut self, ri: usize) {
        let chars = &self.raw[ri].chars;
        if !chars.is_empty() && *chars.last().unwrap() == ' ' {
            let ts = chars.iter().rposition(|&c| c != ' ').map(|i| i + 1).unwrap_or(0);
            self.errors.push(TelError::new(
                ErrorCode::E108, self.raw[ri].start + ts, self.raw[ri].start + chars.len(),
            ));
        }
    }

    /// Parse blocks at the given parent indent level.
    /// `parent_indent` is -1 for root (accepts indent 0).
    fn parse_blocks(&mut self, parent_indent: i32) -> Vec<Block> {
        let expected = (parent_indent + 1) as usize;
        let mut blocks: Vec<Block> = Vec::new();
        let mut cur = Block {
            comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0,
        };
        let mut blank_count: usize = 0;
        let mut prev_kind = PrevKind::Start; // what preceded current line

        while self.idx < self.raw.len() {
            let ri = self.idx;
            let line = &self.raw[ri];

            if line.is_blank() {
                // Peek ahead: if the next non-blank line belongs to a parent level,
                // don't consume these blanks — let the parent handle them.
                let mut peek = ri + 1;
                while peek < self.raw.len() && self.raw[peek].is_blank() { peek += 1; }
                if peek < self.raw.len() && !self.raw[peek].is_blank() {
                    let pi = self.peek_indent(peek);
                    if let Some(pi) = pi {
                        if pi < expected {
                            // These blanks precede a parent-level line — don't consume
                            break;
                        }
                    }
                }

                blank_count += 1;
                self.idx += 1;
                // Blank line terminates a tabulated block
                if cur.tabulation.is_some() && !cur.compounds.is_empty() {
                    cur.trailing_blank_lines = blank_count;
                    blocks.push(cur);
                    cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                    blank_count = 0;
                }
                prev_kind = PrevKind::Blank;
                continue;
            }

            let indent = match self.line_indent(ri) {
                Some(i) => i,
                None => { self.idx += 1; continue; } // shouldn't happen for non-blank
            };

            if indent != expected {
                if cur.tabulation.is_some() {
                    // Row at wrong indent inside a tabulated block → E116
                    self.errors.push(TelError::new(
                        ErrorCode::E116, self.raw[ri].start, self.raw[ri].start + self.margin + indent * 2,
                    ));
                    self.idx += 1;
                    continue;
                }
                if indent < expected {
                    break; // belongs to parent
                }
                // E111: over-indentation. Recovery (deliberate
                // simplification of §19.5's full backtracking): skip
                // the over-indented line and continue with the next
                // line at the originally-expected indent. The line is
                // omitted from the presentation model.
                self.errors.push(TelError::new(
                    ErrorCode::E111, self.raw[ri].start, self.raw[ri].start + self.margin + indent * 2,
                ));
                self.idx += 1;
                continue;
            }

            // indent == expected
            let kind = self.classify(ri);

            match kind {
                LineKind::Blank => {
                    self.idx += 1;
                    blank_count += 1;
                    continue;
                }

                LineKind::Comment(text) => {
                    // E109 check
                    let ok = matches!(prev_kind, PrevKind::Start | PrevKind::Blank | PrevKind::Comment);
                    if !ok {
                        self.errors.push(TelError::new(
                            ErrorCode::E109, self.raw[ri].start, self.raw[ri].start,
                        ));
                    }

                    // New block if previous had compounds
                    if blank_count > 0 && !cur.compounds.is_empty() {
                        cur.trailing_blank_lines = blank_count;
                        blocks.push(cur);
                        cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                        blank_count = 0;
                    }
                    if blank_count > 0 && !cur.comments.is_empty() && cur.compounds.is_empty() {
                        cur.trailing_blank_lines = blank_count;
                        blocks.push(cur);
                        cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                    }
                    blank_count = 0;

                    cur.comments.push(Comment { text });
                    prev_kind = PrevKind::Comment;
                    self.idx += 1;
                }

                LineKind::Tabulation(tab) => {
                    // Close prev tabulated block
                    if cur.tabulation.is_some() && !cur.compounds.is_empty() {
                        blocks.push(cur);
                        cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                    } else if blank_count > 0 && !cur.compounds.is_empty() {
                        cur.trailing_blank_lines = blank_count;
                        blocks.push(cur);
                        cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                    }
                    blank_count = 0;
                    cur.tabulation = Some(tab);
                    prev_kind = PrevKind::Tabulation;
                    self.idx += 1;
                }

                LineKind::Ordinary { keyword, atoms, remark } => {
                    self.check_trailing(ri);

                    // Validate tabulated row if in a tabulated block
                    if let Some(ref tab) = cur.tabulation {
                        let tab_clone = tab.clone();
                        self.validate_tabulated_row(ri, &tab_clone);
                    }

                    // New block on blank gap
                    if blank_count > 0 && !cur.compounds.is_empty() {
                        cur.trailing_blank_lines = blank_count;
                        blocks.push(cur);
                        cur = Block { comments: vec![], tabulation: None, compounds: vec![], trailing_blank_lines: 0 };
                    }
                    blank_count = 0;

                    let mut compound = Compound {
                        keyword, atoms, remark, children: vec![],
                    };
                    self.idx += 1;

                    // Look for source atom, literal atom, or children
                    let is_tab_row = cur.tabulation.is_some();
                    if is_tab_row {
                        // Tabulated rows must not have children (E112)
                        if self.idx < self.raw.len() && !self.raw[self.idx].is_blank() {
                            let next_indent = self.peek_indent(self.idx);
                            if let Some(ni) = next_indent {
                                if ni > expected {
                                    self.errors.push(TelError::new(
                                        ErrorCode::E112, self.raw[self.idx].start, self.raw[self.idx].start,
                                    ));
                                    self.idx += 1; // skip the offending line
                                }
                            }
                        }
                    } else {
                        self.parse_compound_body(&mut compound, expected as i32);
                    }

                    cur.compounds.push(compound);
                    prev_kind = PrevKind::Compound;
                }
            }
        }

        if blank_count > 0 && (!cur.compounds.is_empty() || !cur.comments.is_empty()) {
            cur.trailing_blank_lines = blank_count;
        }
        if !cur.compounds.is_empty() || !cur.comments.is_empty() || cur.tabulation.is_some() {
            blocks.push(cur);
        }
        blocks
    }

    fn validate_tabulated_row(&mut self, ri: usize, tab: &Tabulation) {
        let chars = &self.raw[ri].chars;
        let margin = self.margin;
        if chars.len() <= margin { return; }
        let after = &chars[margin..];
        let indent_spaces = after.iter().take_while(|&&c| c == ' ').count();
        let content = &after[indent_spaces..];

        // Find remark position to exempt from validation
        let sigil = self.sigil;
        let remark_pos = find_remark_pos(content, sigil);
        let check_end = remark_pos.unwrap_or(content.len());

        // Find all hard space runs in the content and check against marker offsets
        let mut i = 0;
        while i < check_end {
            if content[i] == ' ' {
                let space_start = i;
                while i < content.len() && content[i] == ' ' { i += 1; }
                let space_len = i - space_start;
                if space_len >= 2 {
                    // Hard space: must end at M_i - 1 for some column marker
                    let hard_end = indent_spaces + space_start + space_len; // position in after-margin
                    let valid = tab.marker_offsets.iter().any(|&m| m > 0 && hard_end == m);
                    if !valid {
                        self.errors.push(TelError::new(
                            ErrorCode::E117,
                            self.raw[ri].start + margin + indent_spaces + space_start,
                            self.raw[ri].start + margin + hard_end,
                        ));
                    }
                }
            } else {
                i += 1;
            }
        }

        // E119: column width check
        for col_idx in 0..tab.marker_offsets.len() {
            let m_i = tab.marker_offsets[col_idx];
            if m_i == 0 { continue; } // skip M_0

            // Check if column is present (row has content at M_i position)
            let pos_in_after = m_i; // marker offset is relative to after-margin
            if pos_in_after >= after.len() { continue; } // column not present

            // For non-final columns, check width
            if col_idx + 1 < tab.marker_offsets.len() {
                let m_next = tab.marker_offsets[col_idx + 1];
                let max_width = m_next - m_i - 2;
                // Find column value: from M_i to next hard space or end
                let col_start = pos_in_after;
                let mut col_end = col_start;
                while col_end < after.len() && !(after[col_end] == ' ' && col_end + 1 < after.len() && after[col_end + 1] == ' ') {
                    col_end += 1;
                }
                // Trim trailing space
                while col_end > col_start && after[col_end - 1] == ' ' { col_end -= 1; }
                let width = col_end - col_start;
                if width > max_width {
                    self.errors.push(TelError::new(
                        ErrorCode::E119,
                        self.raw[ri].start + margin + col_start,
                        self.raw[ri].start + margin + col_end,
                    ));
                }
            }
        }
    }

    fn parse_compound_body(&mut self, compound: &mut Compound, compound_indent: i32) {
        let ci = compound_indent as usize;

        // Must be immediately following (no blank line) for source/literal
        if self.idx >= self.raw.len() { return; }
        let ri = self.idx;

        // If blank, don't consume — let parent handle blank lines and children
        if self.raw[ri].is_blank() { return; }

        let indent = match self.line_indent(ri) {
            Some(i) => i,
            None => return,
        };

        if indent == ci + 2 {
            // Source atom (immediately after compound, no blank line)
            if compound.atoms.iter().any(|a| matches!(a, Atom::Source{..} | Atom::Literal{..})) {
                self.errors.push(TelError::new(
                    ErrorCode::E113, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
                ));
                self.idx += 1;
                return;
            }
            let text = self.consume_source_atom(ci + 2);
            compound.atoms.push(Atom::Source { text });
            return;
        }

        if indent == ci + 3 {
            // Literal atom (immediately after compound, no blank line)
            if compound.atoms.iter().any(|a| matches!(a, Atom::Source{..} | Atom::Literal{..})) {
                self.errors.push(TelError::new(
                    ErrorCode::E114, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
                ));
                self.idx += 1;
                return;
            }
            if let Some((delim, text)) = self.consume_literal_atom(ci + 3) {
                compound.atoms.push(Atom::Literal { delimiter: delim, text });
            }
            return;
        }

        if indent == ci + 1 {
            // Children at indent+1
            let children = self.parse_blocks(compound_indent);
            compound.children = children;
        }
        // else: indent <= ci or indent > ci+3: don't consume
    }

    fn consume_source_atom(&mut self, source_indent: usize) -> String {
        let indent_chars = self.margin + source_indent * 2;
        let mut lines: Vec<String> = Vec::new();

        while self.idx < self.raw.len() {
            let ri = self.idx;
            let line = &self.raw[ri];

            if line.is_blank() {
                // Blank in source atom = newline
                lines.push(String::new());
                self.idx += 1;
                // Check if source atom continues after blanks
                let mut peek = self.idx;
                while peek < self.raw.len() && self.raw[peek].is_blank() {
                    peek += 1;
                }
                if peek < self.raw.len() {
                    let pi = self.peek_indent(peek);
                    if let Some(pi) = pi {
                        if pi < source_indent {
                            break; // end source atom
                        }
                        // continues
                    } else {
                        break;
                    }
                }
                continue;
            }

            // Non-blank: check indent
            let li = self.peek_indent(ri);
            if let Some(li) = li {
                if li < source_indent {
                    break;
                }
            }

            // Strip indent and trailing spaces
            let chars = &line.chars;
            let stripped: String = if chars.len() > indent_chars {
                chars[indent_chars..].iter().collect::<String>().trim_end().to_string()
            } else {
                String::new()
            };
            lines.push(stripped);
            self.idx += 1;
        }

        let mut result = lines.join("\n");
        result.push('\n');
        result
    }

    fn peek_indent(&self, ri: usize) -> Option<usize> {
        let line = &self.raw[ri];
        if line.is_blank() { return None; }
        let margin = self.margin;
        if line.chars.len() < margin { return Some(0); }
        let spaces = line.chars[margin..].iter().take_while(|&&c| c == ' ').count();
        Some(spaces / 2)
    }

    fn consume_literal_atom(&mut self, literal_indent: usize) -> Option<(String, String)> {
        let ri = self.idx;
        let indent_chars = self.margin + literal_indent * 2;
        let chars = &self.raw[ri].chars;

        if chars.len() <= indent_chars {
            return None; // empty delimiter
        }

        let delimiter: String = chars[indent_chars..].iter().collect::<String>().trim_end().to_string();
        if delimiter.is_empty() {
            return None;
        }

        self.idx += 1; // consume delimiter line

        // Scan raw lines for closing delimiter
        let mut payload_lines: Vec<String> = Vec::new();
        let mut found = false;

        while self.idx < self.raw.len() {
            let line_text = self.raw[self.idx].text();
            self.idx += 1;
            if line_text == delimiter {
                found = true;
                break;
            }
            payload_lines.push(line_text);
        }

        if !found {
            self.errors.push(TelError::new(
                ErrorCode::E115, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
            ));
        }

        let text = payload_lines.join("\n");
        Some((delimiter, text))
    }
}

#[derive(Debug, Clone, Copy)]
enum PrevKind { Start, Blank, Comment, Tabulation, Compound }

/// Find the position of a remark introducer in content, if any.
fn find_remark_pos(content: &[char], sigil: char) -> Option<usize> {
    let mut i = 0;
    let mut hard_space_seen = false;
    while i < content.len() {
        if content[i] == ' ' {
            let start = i;
            while i < content.len() && content[i] == ' ' { i += 1; }
            let spaces = i - start;
            if spaces >= 2 { hard_space_seen = true; }
            if i >= content.len() { break; }
            // Check remark
            if content[i] == sigil {
                let at_boundary = if hard_space_seen { spaces >= 2 } else { true };
                if at_boundary && i + 1 < content.len() && content[i + 1] == ' ' {
                    if i + 2 >= content.len() || content[i + 2] != ' ' {
                        return Some(start); // remark starts at the space before sigil
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn has_tab_markers(content: &[char], sigil: char) -> bool {
    if content.is_empty() || content[0] != sigil { return false; }
    let mut space_count = 0;
    for i in 1..content.len() {
        if content[i] == ' ' {
            space_count += 1;
        } else {
            if content[i] == sigil && space_count >= 2 { return true; }
            space_count = 0;
        }
    }
    false
}

fn parse_tabulation(content: &[char], sigil: char, indent_spaces: usize, errors: &mut Vec<TelError>, line_start: usize) -> Tabulation {
    // Marker offsets are stored relative to after-margin (including indent)
    let mut offsets = vec![indent_spaces]; // M_0 at indent position
    let mut space_count = 0;
    for i in 1..content.len() {
        if content[i] == ' ' {
            space_count += 1;
        } else {
            if content[i] == sigil && space_count >= 2 {
                offsets.push(indent_spaces + i);
            }
            space_count = 0;
        }
    }

    // Parse headings
    let mut headings = Vec::new();
    for (_mi, &off) in offsets.iter().enumerate() {
        let pos = off - indent_spaces; // position in content
        if pos + 1 >= content.len() {
            headings.push(String::new());
            continue;
        }
        let after = &content[pos + 1..];
        if after.is_empty() {
            headings.push(String::new());
            continue;
        }
        if after[0] != ' ' {
            errors.push(TelError::with_detail(
                ErrorCode::E120, line_start + pos, line_start + pos + 2, "non-space after marker",
            ));
            headings.push(String::new());
            continue;
        }
        let spaces = after.iter().take_while(|&&c| c == ' ').count();
        if spaces >= 2 {
            // Hard space: check next non-space is sigil (or end)
            let next_pos = spaces;
            if next_pos < after.len() && after[next_pos] != sigil {
                errors.push(TelError::with_detail(
                    ErrorCode::E120,
                    line_start + pos,
                    line_start + pos + next_pos + 1,
                    "hard space not followed by marker",
                ));
            }
            headings.push(String::new());
        } else {
            // soft space: heading until hard space or end
            let txt = &after[1..];
            let end = txt.iter().enumerate().position(|(j, &c)| {
                c == ' ' && j + 1 < txt.len() && txt[j + 1] == ' '
            }).unwrap_or(txt.len());
            let heading: String = txt[..end].iter().collect();
            if heading.contains(sigil) {
                errors.push(TelError::with_detail(
                    ErrorCode::E120, line_start + pos, line_start + pos + 2 + end, "heading contains sigil",
                ));
            }
            headings.push(heading);
        }
    }

    Tabulation { marker_offsets: offsets, headings }
}

fn parse_atoms(rest: &[char], sigil: char) -> (Vec<Atom>, Option<String>) {
    let mut atoms = Vec::new();
    let mut remark = None;
    let mut i = 0;
    let mut hard_space_seen = false;

    while i < rest.len() {
        // Count spaces
        let mut spaces = 0;
        while i < rest.len() && rest[i] == ' ' { spaces += 1; i += 1; }
        if spaces == 0 || i >= rest.len() { break; }
        if spaces >= 2 { hard_space_seen = true; }

        // Check remark: sigil at word boundary + soft space
        if rest[i] == sigil {
            let at_boundary = if hard_space_seen { spaces >= 2 } else { true };
            if at_boundary && i + 1 < rest.len() && rest[i + 1] == ' ' {
                // Check it's exactly soft space (not hard space after sigil)
                if i + 2 >= rest.len() || rest[i + 2] != ' ' {
                    let payload: String = rest[i + 2..].iter().collect();
                    remark = Some(payload);
                    break;
                }
            }
        }

        // Parse word
        let word_start = i;
        if hard_space_seen {
            // Hard-space mode: word ends at hard space
            while i < rest.len() {
                if rest[i] == ' ' {
                    let mut sc = 0;
                    let mut k = i;
                    while k < rest.len() && rest[k] == ' ' { sc += 1; k += 1; }
                    if sc >= 2 { break; }
                    i = k;
                } else {
                    i += 1;
                }
            }
        } else {
            while i < rest.len() && rest[i] != ' ' { i += 1; }
            // Check if we reached a hard space
            if i < rest.len() {
                let mut sc = 0;
                let mut k = i;
                while k < rest.len() && rest[k] == ' ' { sc += 1; k += 1; }
                if sc >= 2 { hard_space_seen = true; }
            }
        }

        let text: String = rest[word_start..i].iter().collect();
        atoms.push(Atom::Inline { text, preceding_spaces: spaces });
    }

    (atoms, remark)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// Given a URL like `https://example.org/contact-schema`, return
    /// `Some("contact-schema")` if the tail is a kebab-case identifier.
    /// Returns `None` for URLs that don't end in a usable name component.
    fn extract_url_tail(url: &str) -> Option<String> {
        if !url.contains("://") { return None; }
        // Strip any fragment
        let url = url.split('#').next().unwrap_or(url);
        // Strip any query string
        let url = url.split('?').next().unwrap_or(url);
        let tail = url.rsplit('/').next()?;
        if tail.is_empty() { return None; }
        if !tail.chars().next()?.is_ascii_lowercase() { return None; }
        let ok = tail.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if ok && !tail.contains("--") && !tail.ends_with('-') {
            Some(tail.to_string())
        } else {
            None
        }
    }

    fn run_test_with_timeout(path: &str, expect_errors: bool) -> (bool, String) {
        let input = match fs::read(path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => return (false, format!("read error: {}", e)),
        };

        let (tx, rx) = mpsc::channel();
        let input2 = input.clone();
        let _handle = thread::spawn(move || {
            let result = parse(&input2);
            let _ = tx.send(result);
        });

        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => {
                // Schema processing pipeline. Two test conventions:
                //
                // (1) Pragma names tel-schema (via its placeholder URL):
                //     type-assign against the built-in tel-schema, and if
                //     that passes, construct a Schema and validate it.
                //
                // (2) Pragma URL ends with `/<NAME>` where <NAME> matches
                //     a sibling `<NAME>.tel` file in the same directory as
                //     this test: parse that file as a schema document
                //     (type-checked against tel-schema), construct the
                //     user schema, and type-check the current document
                //     against it. This is how user-document tests
                //     reference their schema.
                let mut all_errors: Vec<TelError> = result.errors.clone();
                if let Some(ref pr) = result.document.pragma {
                    if let Some(schema_url) = pr.schema.as_deref() {
                        if schema_url == "https://tel-lang.org/schema/tel-schema" {
                            let builtin = builtin_tel_schema();
                            let ta = type_assign(&result.document, &builtin, None);
                            let had_ta_errors = !ta.errors.is_empty();
                            all_errors.extend(ta.errors);
                            if !had_ta_errors {
                                let constructed = construct_schema(&result.document);
                                for serr in validate_schema(&constructed) {
                                    all_errors.push(TelError::with_detail(
                                        serr.code, 0, 0, serr.detail,
                                    ));
                                }
                            }
                        } else if let Some(name) = extract_url_tail(schema_url) {
                            // Look for a sibling `<name>.tel` first, then
                            // `_<name>.tel` (the underscore convention for
                            // auxiliary fixtures that aren't tests themselves).
                            let test_dir = std::path::Path::new(path).parent()
                                .map(|p| p.to_path_buf())
                                .unwrap_or_else(|| std::path::PathBuf::from("."));
                            let primary = test_dir.join(format!("{}.tel", name));
                            let underscore = test_dir.join(format!("_{}.tel", name));
                            let schema_src_result = fs::read_to_string(&primary)
                                .or_else(|_| fs::read_to_string(&underscore));
                            if let Ok(schema_src) = schema_src_result {
                                // Parse the schema document and build a Schema
                                let schema_parsed = parse(&schema_src);
                                let builtin = builtin_tel_schema();
                                let schema_ta = type_assign(
                                    &schema_parsed.document, &builtin, None,
                                );
                                if schema_ta.errors.is_empty() {
                                    let user_schema = construct_schema(&schema_parsed.document);
                                    // Validate the constructed user schema first;
                                    // surface any of its own E2xx errors so the test
                                    // author sees them.
                                    for serr in validate_schema(&user_schema) {
                                        all_errors.push(TelError::with_detail(
                                            serr.code, 0, 0,
                                            format!("[schema `{}`] {}", name, serr.detail),
                                        ));
                                    }
                                    // Then type-check the test document against it.
                                    let doc_ta = type_assign(
                                        &result.document, &user_schema, None,
                                    );
                                    all_errors.extend(doc_ta.errors);
                                }
                                // If the schema document itself doesn't parse
                                // against tel-schema, we don't surface those
                                // errors here — that's the schema's own bug,
                                // not the test document's.
                            }
                        }
                    }
                }

                let has_errors = !all_errors.is_empty();
                let mut output = format!("{}", result.document);
                if !all_errors.is_empty() {
                    output.push_str("\nerrors:\n");
                    for e in &all_errors {
                        output.push_str(&format!("  {}\n", e));
                    }
                }
                let check_path = path.replace(".tel", ".check");
                let _ = fs::write(&check_path, &output);
                let passed = if expect_errors { has_errors } else { !has_errors };
                (passed, output)
            }
            Err(_) => {
                // Timed out — don't join the thread (it may be stuck)
                (false, "TIMEOUT: parse took > 100ms".into())
            }
        }
    }

    fn run_dir(dir: &str, expect_errors: bool) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tel").unwrap_or(false))
            // Files whose name begins with `_` are auxiliary fixtures
            // (typically schemas referenced by sibling tests), not tests
            // themselves.
            .filter(|e| !e.file_name().to_string_lossy().starts_with('_'))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        let total = entries.len();
        let mut failures = Vec::new();

        for entry in entries {
            let path = entry.path();
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let (passed, output) = run_test_with_timeout(path.to_str().unwrap(), expect_errors);
            if !passed {
                let short = if output.len() > 200 { &output[..200] } else { &output };
                failures.push(format!("  FAIL {}/{}: {}", dir, name, short));
            }
        }

        eprintln!("\n{}: {}/{}", dir, total - failures.len(), total);
        for f in &failures { eprintln!("{}", f); }
        if !failures.is_empty() {
            panic!("{} tests failed out of {}", failures.len(), total);
        }
    }

    #[test]
    fn positive_tests() { run_dir("test/pos", false); }

    #[test]
    fn negative_tests() { run_dir("test/neg", true); }

    // ── Schema unit tests ───────────────────────────────────────────────────

    #[test]
    fn builtin_tel_schema_is_valid() {
        let s = builtin_tel_schema();
        let errors = validate_schema(&s);
        assert!(errors.is_empty(), "built-in tel-schema reports errors: {:?}", errors);
    }

    #[test]
    fn builtin_tel_schema_has_expected_definitions() {
        let s = builtin_tel_schema();
        let names: Vec<&str> = s.types.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec![
            "layer-body", "record-body", "scalar-body", "overlay-body",
            "document-body", "field-body", "select-body", "variant-body",
        ]);
    }

    #[test]
    fn validate_identifier_accepts_kebab_case() {
        assert_eq!(validate_identifier("foo"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("update-value"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("tel-schema"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("a"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("a-b-c-d"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("foo123"), ValidationResponse::Valid);
        assert_eq!(validate_identifier("a1-b2"), ValidationResponse::Valid);
    }

    #[test]
    fn validate_identifier_rejects_malformed() {
        assert!(matches!(validate_identifier(""), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("-foo"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("foo-"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("foo--bar"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("Foo"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("1foo"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("foo_bar"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_identifier("foo bar"), ValidationResponse::Invalid(_)));
    }

    #[test]
    fn validate_sigil_accepts_valid_chars() {
        for s in &["#", "!", "@", "$", "%", "&", "*", "+", ".", "/", ":", ";", "?", "^", "_", "|", "~"] {
            assert_eq!(validate_sigil(s), ValidationResponse::Valid, "sigil `{}` rejected", s);
        }
    }

    #[test]
    fn validate_sigil_rejects_invalid_chars() {
        // letters
        assert!(matches!(validate_sigil("a"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_sigil("A"), ValidationResponse::Invalid(_)));
        // digits
        assert!(matches!(validate_sigil("1"), ValidationResponse::Invalid(_)));
        // whitespace
        assert!(matches!(validate_sigil(" "), ValidationResponse::Invalid(_)));
        // parentheticals
        assert!(matches!(validate_sigil("("), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_sigil("["), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_sigil("<"), ValidationResponse::Invalid(_)));
        assert!(matches!(validate_sigil("{"), ValidationResponse::Invalid(_)));
        // multi-char
        assert!(matches!(validate_sigil("##"), ValidationResponse::Invalid(_)));
        // empty
        assert!(matches!(validate_sigil(""), ValidationResponse::Invalid(_)));
        // non-ASCII
        assert!(matches!(validate_sigil("ñ"), ValidationResponse::Invalid(_)));
    }

    #[test]
    fn validate_string_always_passes() {
        assert_eq!(validate_string(""), ValidationResponse::Valid);
        assert_eq!(validate_string("anything"), ValidationResponse::Valid);
        assert_eq!(validate_string("with spaces"), ValidationResponse::Valid);
        assert_eq!(validate_string("123"), ValidationResponse::Valid);
    }

    #[test]
    fn validate_schema_catches_e201_duplicate_keyword() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E201),
                "expected E201, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e202_empty_select() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Select(Select {
                        required: false, repeatable: false,
                        variants: vec![],
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E202),
                "expected E202, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e204_default_on_optional() {
        // Field.default on a non-required field is invalid (E204).
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, // not required, so default is illegal
                        repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()] }),
                        default: Some("bar".to_string()),
                    }),
                ],
                validators: vec![],
            },
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E204),
                "expected E204, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e208_bad_sigil() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![],
            sigil: Some('A'), // letter
            types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E208),
                "expected E208, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e209_reserved_keyword() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "tel".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E209),
                "expected E209, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e210_unresolved_reference() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Reference("missing".to_string()), default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E210),
                "expected E210, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e211_duplicate_definition() {
        let dup = || Definition {
            name: "dup".to_string(),
            members: vec![], validators: Vec::new(),
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![],
            sigil: None,
            types: vec![dup(), dup()], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E211),
                "expected E211, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e217_exclude_in_document() {
        // `exclude K` may appear only inside a layer's root. An exclude
        // in the base schema's document is E217.
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![Member::Exclude("foo".to_string())],
                validators: vec![],
            },
            layers: vec![],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E217),
                "expected E217, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e217_exclude_in_base_definition() {
        // An exclude inside a base Definition is also E217 (Definitions
        // are part of the base schema namespace).
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![],
            sigil: None,
            types: vec![Definition {
                name: "thing".to_string(),
                members: vec![Member::Exclude("bar".to_string())],
                validators: vec![],
            }], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E217),
                "expected E217, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_accepts_exclude_in_layer_root() {
        // Exclude is legitimate inside a layer's root struct: NOT E217.
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![Member::Select(Select {
                    required: false, repeatable: false,
                    variants: vec![
                        Variant { keyword: "a".to_string(), r#type: Type::Flag },
                        Variant { keyword: "b".to_string(), r#type: Type::Flag },
                    ],
                })],
                validators: vec![],
            },
            layers: vec![Layer {
                name: "drop-b".to_string(),
                overlay: Struct {
                    members: vec![Member::Exclude("b".to_string())],
                    validators: vec![],
                },
                types: vec![], scalars: Vec::new(),
            }],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(!errors.iter().any(|e| e.code == ErrorCode::E217),
                "expected no E217, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e205_duplicate_layer() {
        let l = || Layer {
            name: "dup".to_string(),
            overlay: Struct { members: vec![], validators: vec![] },
            types: vec![], scalars: Vec::new(),
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![l(), l()],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E205),
                "expected E205, got: {:?}", errors);
    }

    // ── Type assignment unit tests ──────────────────────────────────────────

    /// Helper: build a minimal schema for testing.
    fn schema_with_root(members: Vec<Member>) -> Schema {
        Schema {
            name: "test".to_string(),
            document: Struct { members, validators: Vec::new() },
            layers: vec![],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        }
    }

    fn field(req: bool, rep: bool, kw: &str, t: Type) -> Member {
        Member::Field(Field {
            required: req, repeatable: rep, keyword: kw.to_string(), r#type: t, default: None,
        })
    }

    fn select(req: bool, rep: bool, variants: Vec<Variant>) -> Member {
        Member::Select(Select { required: req, repeatable: rep, variants })
    }

    fn variant_(kw: &str, t: Type) -> Variant {
        Variant { keyword: kw.to_string(), r#type: t }
    }

    fn scalar_string() -> Type {
        Type::Scalar(Scalar { validators: vec!["string".to_string()]})
    }

    #[test]
    fn type_assign_minimal_valid_document() {
        // schema: required Scalar field `name`
        let s = schema_with_root(vec![
            field(true, false, "name", scalar_string()),
        ]);
        let doc = parse("name Alice\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.is_empty(), "expected no errors, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e306_unknown_keyword() {
        let s = schema_with_root(vec![
            field(true, false, "name", scalar_string()),
        ]);
        let doc = parse("name Alice\nwhat-is-this 42\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E306),
                "expected E306, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e307_required_absent() {
        let s = schema_with_root(vec![
            field(true, false, "name", scalar_string()),
        ]);
        let doc = parse("").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E307),
                "expected E307, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_required_with_default_satisfies() {
        // Required Scalar Field with a Field.default — the default is
        // substituted when the field is absent from the document.
        let s = schema_with_root(vec![
            Member::Field(Field {
                required: true, repeatable: false,
                keyword: "name".to_string(),
                r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()] }),
                default: Some("Anonymous".to_string()),
            }),
        ]);
        let doc = parse("").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.is_empty(), "expected no errors due to default, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e308_non_repeatable_filled_twice() {
        let s = schema_with_root(vec![
            field(false, false, "name", scalar_string()),
        ]);
        let doc = parse("name Alice\nname Bob\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E308),
                "expected E308, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e310_validator_failure() {
        // schema with identifier validator on `id` field
        let s = schema_with_root(vec![
            Member::Field(Field {
                required: true, repeatable: false,
                keyword: "id".to_string(),
                r#type: Type::Scalar(Scalar { validators: vec!["identifier".to_string()]}), default: None,
            }),
        ]);
        // "FOO" is not a valid identifier (uppercase)
        let doc = parse("id FOO\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E310),
                "expected E310, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e311_flag_with_content() {
        let s = schema_with_root(vec![
            field(false, false, "active", Type::Flag),
        ]);
        // Flag compound with an atom is invalid
        let doc = parse("active extra-atom\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E311),
                "expected E311, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e309_non_contiguous() {
        let s = schema_with_root(vec![
            field(false, true, "a", scalar_string()),
            field(false, true, "b", scalar_string()),
        ]);
        // a, b, a (not contiguous)
        let doc = parse("a 1\nb 2\na 3\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E309),
                "expected E309, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e305_flag_keyword_mismatch() {
        // Flag field with keyword "active", but the user writes "inactive" as atom
        let s = schema_with_root(vec![
            // First a Scalar to position atoms, then a required Flag
            field(true, false, "name", scalar_string()),
        ]);
        // "active" as second atom on the name line — Scalar takes any string,
        // so this should be fine. Let me design a better case.
        // Actually: a required Flag whose keyword is fixed.
        let s2 = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "active".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        // Write the wrong atom name
        let doc = parse("active inactive\n").document;
        let ta = type_assign(&doc, &s2, None);
        // The inline atom "inactive" doesn't match the Flag "active" keyword
        // (it should be a separate atom assignment). Since Flag's keyword is
        // "active" and the compound's keyword is also "active", the inline
        // atom "inactive" tries to bind to... well, there's only one member,
        // and it's been filled by the compound itself, so we'd get a Flag
        // with content (E311).
        assert!(ta.errors.iter().any(|e| matches!(e.code, ErrorCode::E311 | ErrorCode::E305)),
                "expected E311 or E305, got: {:?}", ta.errors);
        let _ = s; // suppress unused warning
    }

    #[test]
    fn type_assign_catches_e301_scalar_compound_with_children() {
        // `note` is a Scalar field. The user writes children under it: that
        // can only make sense for a Struct, so E301.
        let s = schema_with_root(vec![
            field(true, false, "note", scalar_string()),
        ]);
        let doc = parse("note hello\n  child line\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E301),
                "expected E301, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e303_atom_at_non_atom_assignable_position() {
        // `outer` is a required Field whose Struct contains a required Select
        // with mixed variant types (Scalar + Flag). The Select is therefore
        // not atom-assignable, and the atom on `outer`'s line cannot be
        // assigned: E303.
        let mixed_struct = Type::Struct(Struct {
            members: vec![
                Member::Select(Select {
                    required: true, repeatable: false,
                    variants: vec![
                        Variant { keyword: "one".to_string(), r#type: scalar_string() },
                        Variant { keyword: "two".to_string(), r#type: Type::Flag },
                    ],
                }),
            ],
            validators: Vec::new(),
        });
        let s = schema_with_root(vec![
            field(true, false, "outer", mixed_struct),
        ]);
        let doc = parse("outer something\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E303),
                "expected E303, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e304_select_no_matching_variant() {
        // `colour` is a Field with Struct type whose only member is a required
        // all-Flag Select {red, green, blue}. The atom `yellow` on the
        // `colour` compound must match a variant — it doesn't, so E304.
        let colour_struct = Type::Struct(Struct {
            members: vec![
                select(true, false, vec![
                    variant_("red", Type::Flag),
                    variant_("green", Type::Flag),
                    variant_("blue", Type::Flag),
                ]),
            ],
         validators: Vec::new(),});
        let s = schema_with_root(vec![
            field(true, false, "colour", colour_struct),
        ]);
        let doc = parse("colour yellow\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E304),
                "expected E304, got: {:?}", ta.errors);
    }

    // ── compose_schema tests (§20.3) ────────────────────────────────────

    fn layer(name: &str, root_members: Vec<Member>, types: Vec<Definition>) -> Layer {
        Layer {
            name: name.to_string(),
            overlay: Struct { members: root_members, validators: Vec::new() },
            types, scalars: Vec::new(),
        }
    }

    fn flag_field(req: bool, kw: &str) -> Member {
        Member::Field(Field {
            required: req, repeatable: false, keyword: kw.to_string(), r#type: Type::Flag, default: None,
        })
    }

    #[test]
    fn compose_field_add_appends() {
        // Base: { name: string }, Layer: adds { email: string }.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Field(Field {
                    required: true, repeatable: false, keyword: "name".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], validators: Vec::new()},
            layers: vec![layer("with-email", vec![
                Member::Field(Field {
                    required: false, repeatable: false, keyword: "email".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.document.members.len(), 2);
    }

    #[test]
    fn compose_definition_merge_extends_struct() {
        // Base has `define address { street: string }`. Layer adds same
        // definition with `postcode: string`. Composed: 2 fields.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![layer("ext", vec![], vec![Definition {
                name: "address".to_string(),
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false, keyword: "postcode".to_string(),
                        r#type: scalar_string(), default: None,
                    }),
                ], validators: Vec::new(),
            }])],
            sigil: None,
            types: vec![Definition {
                name: "address".to_string(),
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false, keyword: "street".to_string(),
                        r#type: scalar_string(), default: None,
                    }),
                ], validators: Vec::new(),
            }], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.types.len(), 1);
        assert_eq!(composed.types[0].members.len(), 2);
    }

    #[test]
    fn compose_exclude_variant_works() {
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Select(Select {
                    required: false, repeatable: false,
                    variants: vec![
                        Variant { keyword: "active".to_string(), r#type: Type::Flag },
                        Variant { keyword: "archived".to_string(), r#type: Type::Flag },
                    ],
                }),
            ], validators: Vec::new()},
            layers: vec![layer("ro", vec![Member::Exclude("archived".to_string())], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.document.members.len(), 1);
        if let Member::Select(s) = &composed.document.members[0] {
            assert_eq!(s.variants.len(), 1);
            assert_eq!(s.variants[0].keyword, "active");
        } else { panic!("expected Select"); }
    }

    #[test]
    fn compose_exclude_variant_unknown_keyword_is_e212() {
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![flag_field(false, "active")], validators: Vec::new() },
            layers: vec![layer("bad", vec![Member::Exclude("never-existed".to_string())], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E212),
                "expected E212, got: {:?}", errs);
    }

    #[test]
    fn compose_exclude_variant_empties_required_select_is_e213() {
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Select(Select {
                    required: true, repeatable: false,
                    variants: vec![
                        Variant { keyword: "only".to_string(), r#type: Type::Flag },
                    ],
                }),
            ], validators: Vec::new()},
            layers: vec![layer("strip", vec![Member::Exclude("only".to_string())], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E213),
                "expected E213, got: {:?}", errs);
    }

    #[test]
    fn compose_select_variant_keyword_overlap_is_e214() {
        // A layer tries to add a Select whose variant keyword `archived`
        // already exists in the base's Select. This should be E214 —
        // can't widen an existing Select.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Select(Select {
                    required: false, repeatable: false,
                    variants: vec![
                        Variant { keyword: "active".to_string(), r#type: Type::Flag },
                        Variant { keyword: "archived".to_string(), r#type: Type::Flag },
                    ],
                }),
            ], validators: Vec::new()},
            layers: vec![layer("widen", vec![
                Member::Select(Select {
                    required: false, repeatable: false,
                    variants: vec![
                        Variant { keyword: "archived".to_string(), r#type: Type::Flag },
                    ],
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E214),
                "expected E214, got: {:?}", errs);
    }

    #[test]
    fn compose_select_variant_collides_with_field_is_e206() {
        // Layer's Select variant `name` collides with the base Field `name`.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Field(Field {
                    required: true, repeatable: false, keyword: "name".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], validators: Vec::new()},
            layers: vec![layer("collide", vec![
                Member::Select(Select {
                    required: false, repeatable: false,
                    variants: vec![
                        Variant { keyword: "name".to_string(), r#type: Type::Flag },
                    ],
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E206),
                "expected E206, got: {:?}", errs);
    }

    #[test]
    fn compose_appends_root_validators_from_layer() {
        // Base root struct has validator "base-ok". A layer's root carries an
        // additional validator "layer-ok". §20.3 prescribes append-and-dedupe.
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![],
                validators: vec!["base-ok".to_string()],
            },
            layers: vec![Layer {
                name: "ext".to_string(),
                overlay: Struct {
                    members: vec![],
                    validators: vec!["base-ok".to_string(), "layer-ok".to_string()],
                },
                types: vec![], scalars: Vec::new(),
            }],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(
            composed.document.validators,
            vec!["base-ok".to_string(), "layer-ok".to_string()],
            "validators should be append-and-deduplicated",
        );
    }

    #[test]
    fn compose_definition_merge_unions_validators() {
        // Base has `define address` with validator "base-rule". Layer merges
        // `address` with a new validator "layer-rule". §20.3: union.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![layer("ext", vec![], vec![Definition {
                name: "address".to_string(),
                members: vec![],
                validators: vec!["layer-rule".to_string()],
            }])],
            sigil: None,
            types: vec![Definition {
                name: "address".to_string(),
                members: vec![],
                validators: vec!["base-rule".to_string()],
            }], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.types.len(), 1);
        assert_eq!(
            composed.types[0].validators,
            vec!["base-rule".to_string(), "layer-rule".to_string()],
        );
    }

    #[test]
    fn compose_layer_can_tighten_optional_to_required() {
        // Base: `field foo optional scalar string` (required=false).
        // Layer: `field foo required scalar string` (required=true).
        // Expected: merged field has required=true, no errors.
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: false, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("tighten", vec![
                Member::Field(Field {
                    required: true, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        if let Member::Field(f) = &composed.document.members[0] {
            assert!(f.required, "merged field should be required after tightening");
        } else {
            panic!("expected Field at index 0");
        }
    }

    #[test]
    fn compose_layer_can_tighten_repeatable_to_irrepeatable() {
        // Base: `field foo repeatable scalar string` (repeatable=true).
        // Layer: `field foo scalar string` (repeatable=false, i.e. irrepeatable).
        // Expected: merged field has repeatable=false, no errors.
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: true, repeatable: true,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("tighten", vec![
                Member::Field(Field {
                    required: true, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        if let Member::Field(f) = &composed.document.members[0] {
            assert!(!f.repeatable, "merged field should be irrepeatable after tightening");
        } else {
            panic!("expected Field at index 0");
        }
    }

    #[test]
    fn compose_layer_cannot_loosen_required_to_optional_is_e215() {
        // Base required; layer attempts to mark optional → E215.
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: true, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("loosen", vec![
                Member::Field(Field {
                    required: false, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E215),
                "expected E215, got: {:?}", errs);
    }

    #[test]
    fn compose_layer_cannot_loosen_irrepeatable_to_repeatable_is_e216() {
        // Base irrepeatable; layer attempts to mark repeatable → E216.
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: true, repeatable: false,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("loosen", vec![
                Member::Field(Field {
                    required: true, repeatable: true,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            types: vec![], scalars: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E216),
                "expected E216, got: {:?}", errs);
    }

    #[test]
    fn construct_field_with_optional_keyword_yields_required_false() {
        // `field foo optional scalar string` → required=false.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo optional\n    scalar string\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(!f.required, "optional flag should produce required=false");
            assert_eq!(f.keyword, "foo");
        } else {
            panic!("expected Field");
        }
    }

    #[test]
    fn construct_field_without_flags_yields_required_true_irrepeatable_true() {
        // `field foo scalar string` (no flags) → required=true, repeatable=false.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo\n    scalar string\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(f.required, "no flag should default to required=true");
            assert!(!f.repeatable, "no flag should default to repeatable=false");
        } else {
            panic!("expected Field");
        }
    }

    #[test]
    fn construct_field_with_required_and_optional_required_wins() {
        // Both `required` and `optional` flags present: `required` wins
        // (tightening direction), required=true.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo optional required\n    scalar string\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(f.required, "required should override optional in conflict");
        } else {
            panic!("expected Field");
        }
    }

    /// THE bootstrap closure: parsing tel-schema.tel, constructing a Schema
    /// from the result, and confirming it equals the hardcoded built-in.
    #[test]
    fn tel_schema_self_bootstrap_closure() {
        let source = fs::read_to_string("../../tel-schema.tel")
            .expect("tel-schema.tel must exist at the project root");
        let parsed = parse(&source);
        assert!(parsed.errors.is_empty(),
                "parsing tel-schema.tel produced errors: {:?}", parsed.errors);
        // Type-check against the built-in tel-schema.
        let builtin = builtin_tel_schema();
        let ta = type_assign(&parsed.document, &builtin, None);
        assert!(ta.errors.is_empty(),
                "type assignment errors against built-in: {:?}", ta.errors);
        // Construct a Schema from the parsed document.
        let constructed = construct_schema(&parsed.document);
        // The constructed schema MUST equal the built-in. This is the
        // self-describing closure property of §20.5.
        assert_eq!(constructed.name, builtin.name);
        assert_eq!(constructed.document, builtin.document,
                   "constructed.document differs from built-in.document");
        assert_eq!(constructed.types, builtin.types,
                   "constructed.types differs from built-in.types");
        assert_eq!(constructed.layers, builtin.layers);
        assert_eq!(constructed.sigil, builtin.sigil);
        // Lastly: a constructed schema should itself be valid.
        let errs = validate_schema(&constructed);
        assert!(errs.is_empty(),
                "constructed tel-schema reports validity errors: {:?}", errs);
    }

    #[test]
    fn construct_schema_round_trips_minimal_schema() {
        // Hand-built schema → write a TEL source in the v1.0 syntax →
        // re-parse → re-construct. The constructed schema MUST equal the
        // original. In v1.0 every Field type is a `Reference` to a name
        // resolved through the composed namespace (which includes the
        // built-in `string`, `flag`, etc.).
        let original = Schema {
            name: "round-trip".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "name".to_string(),
                        r#type: Type::Reference("string".to_string()),
                        default: None,
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "active".to_string(),
                        r#type: Type::Reference("flag".to_string()),
                        default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: vec![],
            sigil: None,
            types: vec![],
            scalars: Vec::new(),
        };
        let source = "tel 1.0\n\n\
                      name round-trip\n\n\
                      document\n  \
                      field name string\n  \
                      field active optional flag\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let constructed = construct_schema(&parsed.document);
        assert_eq!(constructed, original);
    }

    #[test]
    fn type_assign_with_definitions_resolves_reference() {
        // schema has a Definition `address`, and the root has a Field
        // referencing it.
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    field(true, false, "home", Type::Reference("address".to_string())),
                ],
             validators: Vec::new(),},
            layers: vec![],
            sigil: None,
            types: vec![
                Definition {
                    name: "address".to_string(),
                    members: vec![
                        field(true, false, "city", scalar_string()),
                    ], validators: Vec::new(),
                },
            ], scalars: Vec::new(),
        };
        let doc = parse("home\n  city London\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.is_empty(),
                "expected no errors with Reference resolution, got: {:?}", ta.errors);
    }

    /// The normative BinTEL value hash of `tel-schema.tel` under itself, as
    /// pinned in §20.5 of the TEL Specification and §3 of the BinTEL
    /// Specification. Two conforming implementations MUST agree on this
    /// value.
    pub const TEL_SCHEMA_VALUE_HASH_HEX: &str =
        "55d061b2ced2bcf3d79edfa825aaddf906fd3eca24da7c9b5237ae83782432aa";

    fn hex_decode(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i+2], 16).unwrap())
            .collect()
    }

    #[test]
    fn tel_schema_bintel_value_hash_matches_normative() {
        let source = fs::read_to_string("../../tel-schema.tel")
            .expect("tel-schema.tel must exist at the project root");
        let parsed = parse(&source);
        assert!(parsed.errors.is_empty(), "tel-schema.tel must parse cleanly");
        let schema = builtin_tel_schema();
        let hash = bintel::value_hash(&parsed.document, &schema);
        let expected = hex_decode(TEL_SCHEMA_VALUE_HASH_HEX);
        assert_eq!(hash.to_vec(), expected,
                   "tel-schema.tel value hash does not match the normative \
                   value pinned in spec/tel.md §20.5; computed hex={}",
                   hash.iter().map(|b| format!("{:02x}", b)).collect::<String>());
    }

    #[test]
    fn layered_contact_schema_parses_and_constructs() {
        let source = fs::read_to_string("../../demo/contact-layered-schema.tel")
            .expect("demo/contact-layered-schema.tel must exist");
        let parsed = parse(&source);
        assert!(parsed.errors.is_empty(),
                "layered contact schema parse errors: {:?}", parsed.errors);
        let ta = type_assign(&parsed.document, &builtin_tel_schema(), None);
        assert!(ta.errors.is_empty(),
                "layered contact schema type-assignment errors: {:?}", ta.errors);
        let s = construct_schema(&parsed.document);
        assert_eq!(s.name, "contact");
        assert_eq!(s.layers.len(), 6,
                   "expected 6 layers, got {} ({:?})",
                   s.layers.len(),
                   s.layers.iter().map(|l| &l.name).collect::<Vec<_>>());
        let layer_names: Vec<&str> = s.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(layer_names, vec![
            "with-address", "extended-address", "with-phone",
            "with-status", "with-business", "read-only-status",
        ]);
        let errs = validate_schema(&s);
        assert!(errs.is_empty(),
                "layered contact schema reports validity errors: {:?}", errs);
    }

    /// End-to-end check: parse the layered contact schema, compose its
    /// layers (§20.3), and validate a conforming document against the
    /// composed schema. Tests every operation: Field-add, Definition-
    /// merge, Select-add, and variant-exclude. Also asserts that the
    /// `archived` variant has been excluded from the composed schema
    /// (sum-type subtyping by `read-only-status`).
    #[test]
    fn layered_contact_document_validates_against_composed_schema() {
        let schema_source = fs::read_to_string("../../demo/contact-layered-schema.tel")
            .expect("demo/contact-layered-schema.tel must exist");
        let schema_doc = parse(&schema_source);
        assert!(schema_doc.errors.is_empty());
        let schema = construct_schema(&schema_doc.document);

        // Compose and inspect: the `archived` variant must be gone, and
        // `active` must remain. The composed schema has no layers.
        let (composed, errs) = compose_schema(&schema);
        assert!(errs.is_empty(),
                "compose_schema reported errors: {:?}", errs);
        assert!(composed.layers.is_empty());
        let composed_keywords: Vec<&str> = composed.document.members.iter().flat_map(|m| {
            match m {
                Member::Field(f) => vec![f.keyword.as_str()],
                Member::Select(s) => s.variants.iter().map(|v| v.keyword.as_str()).collect(),
                Member::Exclude(_) => Vec::new(),
            }
        }).collect();
        assert!(composed_keywords.contains(&"active"),
                "composed schema should still contain `active`: {:?}", composed_keywords);
        assert!(!composed_keywords.contains(&"archived"),
                "composed schema should NOT contain `archived`: {:?}", composed_keywords);

        // Validate the document.
        let doc_source = fs::read_to_string("../../demo/contact-layered-document.tel")
            .expect("demo/contact-layered-document.tel must exist");
        let doc = parse(&doc_source);
        assert!(doc.errors.is_empty(),
                "layered contact document parse errors: {:?}", doc.errors);
        let ta = type_assign(&doc.document, &schema, None);
        assert!(ta.errors.is_empty(),
                "type assignment errors against composed schema: {:?}",
                ta.errors);
    }

    /// E107 (odd indentation) is recorded as an error, and the parser
    /// recovers by preferring the shallower interpretation (the line is
    /// treated as if at the floor of (spaces / 2)). Subsequent lines
    /// continue to parse normally.
    #[test]
    fn recovery_from_odd_indentation_continues_parse() {
        // `b` is at 3 spaces (odd) — should be treated as indent 1 (shallower).
        let src = "tel 1.0\n\na\n   b\nc\n";
        let parsed = parse(src);
        // E107 should be raised but parsing produces useful output for
        // the remaining lines.
        assert!(parsed.errors.iter().any(|e| e.code == ErrorCode::E107),
                "expected E107, got: {:?}", parsed.errors);
        // The line `c` at indent 0 should appear at the document root.
        let root_keywords: Vec<&str> = parsed.document.children.iter()
            .flat_map(|b| b.compounds.iter())
            .map(|c| c.keyword.as_str()).collect();
        assert!(root_keywords.contains(&"a"), "got: {:?}", root_keywords);
        assert!(root_keywords.contains(&"c"), "got: {:?}", root_keywords);
    }

    /// E111 (over-indentation) is recorded as an error, and the parser
    /// recovers by skipping the over-indented line. Subsequent lines
    /// continue to parse at the originally-expected indent.
    #[test]
    fn recovery_from_over_indentation_skips_line() {
        // `parent` at indent 0; `too-deep` at indent 8 (4 levels deeper
        // than parent — too deep for a child, beyond source-atom indent,
        // and not a literal atom delimiter). `d` is back at the root
        // and must still parse despite the intervening E111.
        let src = "tel 1.0\n\nparent\n        too-deep\nd\n";
        let parsed = parse(src);
        assert!(parsed.errors.iter().any(|e| e.code == ErrorCode::E111),
                "expected E111, got: {:?}", parsed.errors);
        let root_keywords: Vec<&str> = parsed.document.children.iter()
            .flat_map(|b| b.compounds.iter())
            .map(|c| c.keyword.as_str()).collect();
        assert!(root_keywords.contains(&"d"),
                "parser should recover past E111 and reach `d`, got root keywords: {:?}",
                root_keywords);
    }

    /// Worked example demonstrating all three atom forms. Loads
    /// demo/atom-forms-schema.tel and demo/atom-forms-document.tel,
    /// verifies the document parses and type-checks cleanly, and confirms
    /// each Scalar value's text matches the expected payload.
    #[test]
    fn atom_forms_worked_example() {
        let schema_src = fs::read_to_string("../../demo/atom-forms-schema.tel")
            .expect("demo/atom-forms-schema.tel must exist");
        let schema_parsed = parse(&schema_src);
        assert!(schema_parsed.errors.is_empty(),
                "schema must parse cleanly: {:?}", schema_parsed.errors);
        let schema = construct_schema(&schema_parsed.document);

        let doc_src = fs::read_to_string("../../demo/atom-forms-document.tel")
            .expect("demo/atom-forms-document.tel must exist");
        let doc_parsed = parse(&doc_src);
        assert!(doc_parsed.errors.is_empty(),
                "document must parse cleanly: {:?}", doc_parsed.errors);

        let ta = type_assign(&doc_parsed.document, &schema, None);
        assert!(ta.errors.is_empty(),
                "document must type-check cleanly: {:?}", ta.errors);

        // Flatten compounds across all root blocks (blank lines split blocks).
        let compounds: Vec<&Compound> = doc_parsed.document.children.iter()
            .flat_map(|b| b.compounds.iter()).collect();
        assert_eq!(compounds.len(), 3, "expected three compounds, got {}", compounds.len());

        // inline-value: short text on parent line.
        assert_eq!(compounds[0].keyword, "inline-value");
        assert_eq!(scalar_value_text(compounds[0]), "ipv4-strict");

        // source-value: multi-line JSON.
        assert_eq!(compounds[1].keyword, "source-value");
        let src = scalar_value_text(compounds[1]);
        assert!(src.contains("192.0.2.1"),
                "source-value should contain the JSON payload, got: {:?}", src);

        // literal-value: payload with leading `#` line.
        assert_eq!(compounds[2].keyword, "literal-value");
        let lit = scalar_value_text(compounds[2]);
        assert!(lit.contains("# this would be a comment"),
                "literal-value should contain the would-be-comment line verbatim, got: {:?}", lit);
        assert!(lit.contains("## subheading"),
                "literal-value should contain the ## subheading line, got: {:?}", lit);
    }

    /// End-to-end worked example: load demo/struct-validator-schema.tel
    /// and demo/struct-validator-document.tel, register a real
    /// `start-precedes-end` validator that compares ISO-8601 date strings,
    /// and verify the document's second `event` (where end-date precedes
    /// start-date) raises E310 with a nested per-field diagnostic.
    #[test]
    fn struct_validator_worked_example() {
        use std::collections::HashMap;

        let schema_src = fs::read_to_string("../../demo/struct-validator-schema.tel")
            .expect("demo/struct-validator-schema.tel must exist");
        let schema_parsed = parse(&schema_src);
        assert!(schema_parsed.errors.is_empty(),
                "schema must parse cleanly: {:?}", schema_parsed.errors);
        let schema = construct_schema(&schema_parsed.document);

        let doc_src = fs::read_to_string("../../demo/struct-validator-document.tel")
            .expect("demo/struct-validator-document.tel must exist");
        let doc_parsed = parse(&doc_src);
        assert!(doc_parsed.errors.is_empty(),
                "document must parse cleanly: {:?}", doc_parsed.errors);

        // Validator callback that implements start-precedes-end by
        // reading start-date and end-date from the StructView and
        // comparing them lexicographically (ISO 8601 dates compare
        // correctly this way).
        let cb = |req: &ValidationRequest| -> ValidationResponse {
            match req {
                ValidationRequest::Struct { method, element }
                    if *method == "start-precedes-end" =>
                {
                    let start = element.scalar("start-date");
                    let end = element.scalar("end-date");
                    match (start, end) {
                        (Some(s), Some(e)) if s <= e => ValidationResponse::Valid,
                        (Some(_), Some(e)) => {
                            let mut fields = HashMap::new();
                            fields.insert("end-date".to_string(),
                                Diagnostic::Scalar {
                                    message: format!("end-date `{}` precedes start-date", e),
                                    span: None,
                                });
                            ValidationResponse::Invalid(Diagnostic::Struct {
                                message: "event end-date must not precede start-date".to_string(),
                                fields,
                            })
                        }
                        _ => ValidationResponse::Valid, // missing date — let E307 handle
                    }
                }
                _ => ValidationResponse::Valid,
            }
        };
        let ta = type_assign(&doc_parsed.document, &schema, Some(&cb));
        let e310s: Vec<_> = ta.errors.iter()
            .filter(|e| e.code == ErrorCode::E310).collect();
        // The good event (first) produces no E310; the bad event (second)
        // produces a top-level struct diagnostic plus a nested end-date
        // diagnostic — two E310 entries in total.
        assert_eq!(e310s.len(), 2,
                   "expected exactly two E310 entries (one struct, one nested field), got: {:?}",
                   e310s);
        assert!(e310s.iter().any(|e| e.message.contains("end-date must not precede")),
                "missing top-level struct diagnostic: {:?}", e310s);
        assert!(e310s.iter().any(|e| e.message.contains("`2026-04-30` precedes")),
                "missing field-pointing diagnostic: {:?}", e310s);
    }

    /// Struct validator that pinpoints a specific field using the
    /// recursive `Diagnostic::Struct.fields` mechanism. Tests the
    /// failure path (validator returns Invalid → E310).
    #[test]
    fn struct_validator_invocation_and_diagnostic_shape() {
        use std::collections::HashMap;

        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "address".to_string(),
                        r#type: Type::Struct(Struct {
                            members: vec![
                                Member::Field(Field {
                                    required: true, repeatable: false,
                                    keyword: "street".to_string(),
                                    r#type: Type::Scalar(Scalar {
                                        validators: vec!["string".to_string()]}), default: None,
                                }),
                                Member::Field(Field {
                                    required: true, repeatable: false,
                                    keyword: "country".to_string(),
                                    r#type: Type::Scalar(Scalar {
                                        validators: vec!["string".to_string()]}), default: None,
                                }),
                            ],
                            validators: vec!["postcode-required-when-uk".to_string()],
                        }), default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: vec![], sigil: None, types: vec![], scalars: Vec::new(),
        };
        let doc = parse("tel 1.0\n\naddress\n  street  221B Baker Street\n  country UK\n").document;

        // Validator callback that always rejects struct requests with
        // a Struct diagnostic pointing at the `country` field.
        let cb = |req: &ValidationRequest| -> ValidationResponse {
            match req {
                ValidationRequest::Struct { method, .. } if *method == "postcode-required-when-uk" => {
                    let mut fields = HashMap::new();
                    fields.insert("country".to_string(),
                        Diagnostic::Scalar { message: "UK requires postcode".to_string(), span: None });
                    ValidationResponse::Invalid(Diagnostic::Struct {
                        message: "country/postcode rule violated".to_string(),
                        fields,
                    })
                }
                _ => ValidationResponse::Valid,
            }
        };
        let ta = type_assign(&doc, &schema, Some(&cb));
        let e310s: Vec<_> = ta.errors.iter().filter(|e| e.code == ErrorCode::E310).collect();
        assert!(!e310s.is_empty(),
                "expected E310 from struct validator, got: {:?}", ta.errors);
        assert!(e310s.iter().any(|e| e.message.contains("country/postcode rule violated")),
                "missing top-level struct diagnostic: {:?}", e310s);
        assert!(e310s.iter().any(|e| e.message.contains("UK requires postcode")),
                "missing field-pointing diagnostic: {:?}", e310s);
    }

    /// BinTEL canonical-ordering invariant: two presentation forms of
    /// the same semantic content MUST produce byte-identical BinTEL
    /// (and therefore identical value hashes). Per §7.2 of the BinTEL
    /// Specification, atom-derived and compound-derived children are
    /// emitted in member order, with atom-derived elements preceding
    /// compound-derived elements of the same member.
    #[test]
    fn bintel_presentation_invariance() {
        // A schema with one Scalar field and one Flag field at the root.
        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "name".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "active".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, types: Vec::new(), scalars: Vec::new(),
        };
        // Two equivalent presentation forms — root-level can't use inline
        // atoms (the document root has no atoms), so this test focuses on
        // the property that swapping compound order doesn't affect the
        // hash (canonical = member order).
        let doc_a = parse("tel 1.0\n\nname  Alice Anderson\nactive\n").document;
        let doc_b = parse("tel 1.0\n\nactive\nname  Alice Anderson\n").document;
        let hash_a = bintel::value_hash(&doc_a, &schema);
        let hash_b = bintel::value_hash(&doc_b, &schema);
        assert_eq!(hash_a, hash_b,
                   "value hash differs under member-group reordering: a={} b={}",
                   hash_a.iter().map(|b| format!("{:02x}", b)).collect::<String>(),
                   hash_b.iter().map(|b| format!("{:02x}", b)).collect::<String>());
    }

    /// Canonical-ordering invariant for atom vs compound forms. A Scalar
    /// or Flag value can be filled by an inline atom on the parent's
    /// line OR by a compound child. Both forms MUST encode identically.
    #[test]
    fn bintel_atom_vs_compound_invariance() {
        // Schema: a Struct-typed field `record` containing a Scalar `id`
        // and a Flag `active`. The `record` compound can fill its members
        // with inline atoms on its own line, or with explicit compound
        // children.
        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "record".to_string(),
                        r#type: Type::Struct(Struct {
                            members: vec![
                                Member::Field(Field {
                                    required: true, repeatable: false,
                                    keyword: "id".to_string(),
                                    r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                                }),
                                Member::Field(Field {
                                    required: false, repeatable: false,
                                    keyword: "active".to_string(),
                                    r#type: Type::Flag, default: None,
                                }),
                            ],
                            validators: vec![],
                        }), default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, types: Vec::new(), scalars: Vec::new(),
        };
        // Form A: inline atoms — `record alpha active`. The atoms `alpha`
        // and `active` fill the `id` Scalar and `active` Flag members.
        let doc_a = parse("tel 1.0\n\nrecord alpha active\n").document;
        // Form B: explicit compound children.
        let doc_b = parse("tel 1.0\n\nrecord\n  id alpha\n  active\n").document;
        let hash_a = bintel::value_hash(&doc_a, &schema);
        let hash_b = bintel::value_hash(&doc_b, &schema);
        assert_eq!(hash_a, hash_b,
                   "value hash differs between inline-atom and compound-child forms: a={} b={}",
                   hash_a.iter().map(|b| format!("{:02x}", b)).collect::<String>(),
                   hash_b.iter().map(|b| format!("{:02x}", b)).collect::<String>());
    }

    #[test]
    fn walkthrough_example_encodes_as_expected() {
        // The schema and document described in demo/walkthrough.md.
        let schema = Schema {
            name: "greeting".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "text".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "bold".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, types: Vec::new(), scalars: Vec::new(),
        };
        let source = "tel 1.0\n\ntext  hello, world\nbold\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(),
                "walkthrough doc must parse cleanly: {:?}", parsed.errors);
        let bytes = bintel::encode_root(&parsed.document, &schema);
        let expected = vec![
            0x02,                                           // child count
            0x00, 0x0c,                                     // text @ keyword 0, length 12
            b'h', b'e', b'l', b'l', b'o', b',', b' ',
            b'w', b'o', b'r', b'l', b'd',                   // UTF-8 value
            0x01,                                           // bold @ keyword 1
        ];
        assert_eq!(bytes, expected,
                   "walkthrough BinTEL bytes differ from the values pinned in \
                   demo/walkthrough.md");
    }

    /// Diagnostic helper — prints the bytes/hash for human inspection. Run
    /// with `cargo test print_tel_schema_value_hash -- --nocapture`.
    #[test]
    fn print_tel_schema_value_hash() {
        let source = fs::read_to_string("../../tel-schema.tel")
            .expect("tel-schema.tel must exist at the project root");
        let parsed = parse(&source);
        assert!(parsed.errors.is_empty(), "tel-schema.tel must parse cleanly");
        let schema = builtin_tel_schema();
        let bytes = bintel::encode_root(&parsed.document, &schema);
        let hash = bintel::value_hash(&parsed.document, &schema);
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        let b256 = base256::encode(&hash);
        eprintln!("tel-schema.tel BinTEL root length:    {} bytes", bytes.len());
        eprintln!("tel-schema.tel value hash (hex):      {}", hex);
        eprintln!("tel-schema.tel value hash (base-256): {}", b256);
        let bintel_hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        eprintln!("tel-schema.tel BinTEL root (hex):     {}", bintel_hex);
    }
}

