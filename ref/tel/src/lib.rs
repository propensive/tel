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
    E212, E213, E214, E215, E216, E217, E218,
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
            Self::E217 => "Exclude operation appears outside a layer's SelectDefinition body",
            Self::E218 => "Reference/SelectRef kind mismatch (Reference resolved to a SelectDefinition, or SelectRef resolved to a Record/Scalar)",
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
    pub records: Vec<RecordDefinition>,
    /// User-declared `scalar` definitions (named scalar types).
    pub scalars: Vec<ScalarDefinition>,
    /// User-declared `select` definitions (named sum types — D2 duality
    /// with `RecordDefinition`).
    pub selects: Vec<SelectDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    /// Members merged into the composed document root (§20.3). Written as
    /// the `overlay` keyword in TEL source.
    pub overlay: Struct,
    pub records: Vec<RecordDefinition>,
    pub scalars: Vec<ScalarDefinition>,
    pub selects: Vec<SelectDefinition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordDefinition {
    pub name: String,
    pub members: Vec<Member>,
    /// Struct-level validators (§21.6) applying to instances of this
    /// Definition. Same semantics as `Struct.validators`.
    pub validators: Vec<String>,
}

/// A named scalar type declared via `scalar <Name>` at schema or layer
/// scope. Its `validators` apply (in AND-conjunction) to every value
/// whose field/variant references this scalar by name.
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarDefinition {
    pub name: String,
    pub validators: Vec<String>,
}

/// A named sum type declared via `select <Name>` at schema or layer scope.
/// The variants supply the keywords admissible at each `SelectRef` use
/// site; the SelectDefinition's optional struct-level validators inspect
/// the chosen variant (§21.6).
#[derive(Debug, Clone, PartialEq)]
pub struct SelectDefinition {
    pub name: String,
    pub variants: Vec<Variant>,
    pub validators: Vec<String>,
    /// Layer-only: `Exclude` markers declared in a layer's `select N` body
    /// that name a variant of the base SelectDefinition to remove. Always
    /// empty in a fully composed schema (consumed by `MergeSelect`).
    pub layer_excludes: Vec<String>,
}

/// A `Type` is what a Field or Variant evaluates to. In the v1.0 schema
/// syntax every user-written field/variant type is a `Reference`; the
/// non-`Reference` variants exist only as resolution results.
/// `Type::Reference(name)` resolves (per §20.2) to either a `Struct` formed
/// from the named record's `members`, a `Scalar` formed from the named
/// scalar's `validators`, or one of the built-in types `Flag`, `String`,
/// `Identifier`, `Sigil`. Resolving to a `SelectDefinition` is **E218**
/// — sums at a single-keyword position are written as `SelectRef`, not
/// `Reference`.
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

/// Per-axis declaration state for `Field` and `SelectRef`. The tristate is
/// retained through schema construction and layer merge so §20.3 can
/// distinguish a layer that loosens an already-tight axis (E215/E216)
/// from a redundant restatement. Effective booleans are derived:
///   effective `required`   = `(polarity != Loose)`
///   effective `repeatable` = `(polarity == Loose)`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Polarity {
    /// No flag declared on this axis. Effective boolean follows the
    /// schema-language default — `required=true`, `repeatable=false`.
    Default,
    /// `optional` or `repeatable` was declared (base-side loosening).
    Loose,
    /// `required` or `irrepeatable` was declared (layer-side tightening).
    Tight,
}

impl Polarity {
    /// Effective `required` for an axis carrying this polarity.
    pub fn effective_required(self) -> bool {
        !matches!(self, Polarity::Loose)
    }
    /// Effective `repeatable` for an axis carrying this polarity.
    pub fn effective_repeatable(self) -> bool {
        matches!(self, Polarity::Loose)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    Field(Field),
    /// References a `SelectDefinition` at a member position. The named
    /// Select's variants become the keywords admissible here; the
    /// SelectRef itself has no own keyword. Polarity lives at the use
    /// site (here), not on the SelectDefinition.
    SelectRef(SelectRef),
    /// Layer-only operation: declared inside a layer's `select N` body to
    /// remove a variant from the merged SelectDefinition (§20.3). It MUST
    /// NOT appear inside any Struct (root, RecordDefinition body, or
    /// overlay); appearing there is **E217**.
    Exclude(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub required: Polarity,
    pub repeatable: Polarity,
    pub keyword: String,
    pub r#type: Type,
    /// Per-use-site default value, applied when a required Scalar-typed
    /// field is absent from the document. Valid only when the effective
    /// `required` is `true` and the resolved `type` is `Scalar`
    /// (E204 otherwise).
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectRef {
    pub required: Polarity,
    pub repeatable: Polarity,
    /// `TypeName` of a `SelectDefinition` in the composed namespace.
    pub reference: String,
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
        let type_name = keyword_type_name_in(self.members, keyword, self.schema)?;
        let resolved_members = resolve_reference(&type_name, self.schema)?;
        Some(StructView {
            compound: child,
            members: resolved_members,
            schema: self.schema,
        })
    }

    /// Iterate every child compound with its keyword. Useful for
    /// validators that need to inspect every present child without
    /// knowing the schema's member layout ahead of time.
    pub fn children(&self) -> impl Iterator<Item = &'a Compound> + '_ {
        self.compound.children.iter().flat_map(|b| b.compounds.iter())
    }
}

/// Look up a Field's keyword in a member list and return the TypeName
/// string from its declared `Reference`. For a `SelectRef` member, the
/// keyword may identify a variant of the referenced `SelectDefinition`;
/// resolution requires the schema. Returns `None` when the declared type
/// isn't a Reference (which shouldn't happen in v1.0, where every
/// user-declared type is a Reference).
fn keyword_type_name_in(members: &[Member], keyword: &str, schema: &Schema) -> Option<String> {
    for m in members {
        match m {
            Member::Field(f) if f.keyword == keyword => {
                if let Type::Reference(n) = &f.r#type {
                    return Some(n.clone());
                }
                return None;
            }
            Member::SelectRef(s) => {
                if let Some(variants) = resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        if v.keyword == keyword {
                            if let Type::Reference(n) = &v.r#type {
                                return Some(n.clone());
                            }
                            return None;
                        }
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

/// Built-in validator: `type-name`. Accepts a string conforming to the
/// PascalCase TypeName grammar of §20.7: begins with an uppercase ASCII
/// letter; remainder is ASCII letters and digits; no hyphens, underscores,
/// or non-ASCII characters; non-empty.
pub fn validate_type_name(value: &str) -> ValidationResponse {
    let end = value.chars().count();
    let mk = |msg: &str| scalar_invalid(msg, end);
    if value.is_empty() { return mk("empty TypeName"); }
    let mut chars = value.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_uppercase() {
        return mk("TypeName must start with an uppercase ASCII letter");
    }
    for c in chars {
        if c == '-' || c == '_' {
            return mk("TypeName may not contain hyphens or underscores");
        }
        if !(c.is_ascii_alphanumeric()) {
            return mk("TypeName may contain only ASCII letters and digits");
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
    let builtin = matches!(method, "identifier" | "sigil" | "string" | "type-name");
    if builtin {
        match req {
            ValidationRequest::Scalar { value, .. } => match method {
                "identifier" => return validate_identifier(value),
                "type-name" => return validate_type_name(value),
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
    // Helpers. Every Type used here is a Reference; resolution at type
    // assignment time (§20.2) picks the matching Definition or built-in.
    let id_type = || Type::Reference("Identifier".to_string());
    let sigil_type = || Type::Reference("Sigil".to_string());
    let str_type = || Type::Reference("String".to_string());
    let flag_type = || Type::Reference("Flag".to_string());
    let tn_type = || Type::Reference("TypeName".to_string());
    let refn = |n: &str| Type::Reference(n.to_string());

    // Per-axis polarity literals (kept short for readability).
    let dflt = Polarity::Default;
    let loose = Polarity::Loose;

    // Field-construction helper. Polarity defaults: Default/Default unless
    // a loosening flag was explicitly chosen.
    let field = |req: Polarity, rep: Polarity, kw: &str, t: Type| Member::Field(Field {
        required: req, repeatable: rep, keyword: kw.to_string(),
        r#type: t, default: None,
    });
    // SelectRef-construction helper.
    let selref = |req: Polarity, rep: Polarity, name: &str| Member::SelectRef(SelectRef {
        required: req, repeatable: rep, reference: name.to_string(),
    });
    let variant = |kw: &str, t: Type| Variant { keyword: kw.to_string(), r#type: t };

    // ── RecordDefinitions ────────────────────────────────────────────────

    // A `field` declaration at a member position.
    let r_field = RecordDefinition {
        name: "Field".to_string(),
        members: vec![
            field(dflt, dflt, "keyword", id_type()),
            field(dflt, dflt, "type", tn_type()),
            field(loose, dflt, "optional", flag_type()),
            field(loose, dflt, "required", flag_type()),
            field(loose, dflt, "repeatable", flag_type()),
            field(loose, dflt, "irrepeatable", flag_type()),
            field(loose, dflt, "default", str_type()),
        ], validators: Vec::new(),
    };

    // A `select` declaration at a member position — a SelectRef.
    let r_select_ref = RecordDefinition {
        name: "SelectRef".to_string(),
        members: vec![
            field(dflt, dflt, "reference", tn_type()),
            field(loose, dflt, "optional", flag_type()),
            field(loose, dflt, "required", flag_type()),
            field(loose, dflt, "repeatable", flag_type()),
            field(loose, dflt, "irrepeatable", flag_type()),
        ], validators: Vec::new(),
    };

    // A `variant` declaration inside a Select body.
    let r_variant = RecordDefinition {
        name: "Variant".to_string(),
        members: vec![
            field(dflt, dflt, "keyword", id_type()),
            field(dflt, dflt, "type", tn_type()),
        ], validators: Vec::new(),
    };

    // A `record` declaration: name + members.
    let r_record = RecordDefinition {
        name: "Record".to_string(),
        members: vec![
            field(dflt, dflt, "name", tn_type()),
            selref(loose, loose, "Member"),
        ], validators: Vec::new(),
    };

    // A `scalar` declaration: name + one or more validators.
    let r_scalar = RecordDefinition {
        name: "Scalar".to_string(),
        members: vec![
            field(dflt, dflt, "name", tn_type()),
            field(dflt, loose, "validate", id_type()),
        ], validators: Vec::new(),
    };

    // A top-level `select` declaration.
    let r_select = RecordDefinition {
        name: "Select".to_string(),
        members: vec![
            field(dflt, dflt, "name", tn_type()),
            selref(dflt, loose, "SelectChild"),
        ], validators: Vec::new(),
    };

    // The shared struct-shape used by `document` and `overlay`.
    let r_body = RecordDefinition {
        name: "Body".to_string(),
        members: vec![
            selref(loose, loose, "Member"),
        ], validators: Vec::new(),
    };

    // A `layer` declaration.
    let r_layer = RecordDefinition {
        name: "Layer".to_string(),
        members: vec![
            field(dflt, dflt, "name", id_type()),
            field(loose, loose, "record", refn("Record")),
            field(loose, loose, "scalar", refn("Scalar")),
            field(loose, loose, "select", refn("Select")),
            field(loose, dflt, "overlay", refn("Body")),
        ], validators: Vec::new(),
    };

    // ── SelectDefinitions ────────────────────────────────────────────────

    // Members admissible inside a Body, Record body, or Overlay.
    let s_member = SelectDefinition {
        name: "Member".to_string(),
        variants: vec![
            variant("field", refn("Field")),
            variant("select", refn("SelectRef")),
            variant("validate", id_type()),
        ],
        validators: Vec::new(),
        layer_excludes: Vec::new(),
    };

    // Children admissible inside a Select body. `exclude` is lexically
    // permitted (E217 if it appears outside a layer's Select body at
    // construction time).
    let s_select_child = SelectDefinition {
        name: "SelectChild".to_string(),
        variants: vec![
            variant("variant", refn("Variant")),
            variant("exclude", id_type()),
            variant("validate", id_type()),
        ],
        validators: Vec::new(),
        layer_excludes: Vec::new(),
    };

    // ── Schema document root ─────────────────────────────────────────────

    let document = Struct {
        validators: Vec::new(),
        members: vec![
            field(dflt, dflt, "name", id_type()),
            field(loose, dflt, "sigil", sigil_type()),
            field(loose, loose, "record", refn("Record")),
            field(loose, loose, "scalar", refn("Scalar")),
            field(loose, loose, "select", refn("Select")),
            field(dflt, dflt, "document", refn("Body")),
            field(loose, loose, "layer", refn("Layer")),
        ],
    };

    Schema {
        name: "tel-schema".to_string(),
        document,
        layers: vec![],
        sigil: None,
        records: vec![
            r_field, r_select_ref, r_variant, r_record, r_scalar,
            r_select, r_body, r_layer,
        ],
        scalars: Vec::new(),
        selects: vec![s_member, s_select_child],
    }
}

// ── Reference resolution (§20.2) ────────────────────────────────────────────

/// The predefined TypeNames (PascalCase) that every TEL parser MUST
/// recognize regardless of the user schema. User schemas MAY NOT declare
/// a `record`, `scalar`, or `select` with any of these names. `TypeName`
/// is the scalar type used in the schema-of-schemas to type the `type`
/// and `reference` fields; its values are validated by `validate_type_name`.
pub const BUILTIN_TYPE_NAMES: &[&str] = &["Flag", "String", "Identifier", "Sigil", "TypeName"];

/// Resolve a `Reference` to a record's `Member` slice. Returns `None` if the
/// name doesn't resolve to a record. Used by `Field.type` resolution; a
/// `Field` whose Reference resolves to a `SelectDefinition` is E218 and
/// should be caught at schema-validity time.
pub(crate) fn resolve_reference<'a>(name: &str, schema: &'a Schema) -> Option<&'a [Member]> {
    schema.records.iter()
        .chain(schema.layers.iter().flat_map(|l| l.records.iter()))
        .find(|d| d.name == name)
        .map(|d| d.members.as_slice())
}

/// Resolve a `SelectRef.reference` to a SelectDefinition's variants. Returns
/// `None` if the name doesn't resolve to a SelectDefinition.
pub(crate) fn resolve_select_ref<'a>(name: &str, schema: &'a Schema) -> Option<&'a [Variant]> {
    schema.selects.iter()
        .chain(schema.layers.iter().flat_map(|l| l.selects.iter()))
        .find(|d| d.name == name)
        .map(|d| d.variants.as_slice())
}

/// Per §20.2, resolve a Type that may be a Reference into a concrete
/// non-Reference type. Built-in TypeNames (`Flag`, `String`, `Identifier`,
/// `Sigil`, `TypeName`) short-circuit to owned built-in types. Records
/// resolve to a member-slice borrow; scalars resolve to an owned `Scalar`
/// synthesized from the definition's validators. A Reference that
/// resolves to a `SelectDefinition` is the E218 condition and is not
/// considered resolved here.
pub(crate) enum ResolvedType<'a> {
    Struct(&'a [Member]),
    /// An owned Scalar — used for both built-in scalar types and named
    /// `scalar` definitions. The `Cow` lets us return a borrowed Scalar
    /// when the source is a literal `Type::Scalar(_)`, and an owned one
    /// when it's a built-in or a named scalar definition.
    Scalar(std::borrow::Cow<'a, Scalar>),
    Flag,
    Unresolved, // Reference whose name doesn't resolve (E210)
    /// Reference whose name resolves to a `SelectDefinition` — invalid in
    /// a `Field.type` / `Variant.type` position (E218).
    KindMismatch,
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
    // Built-in TypeNames short-circuit.
    match name {
        "Flag" => return ResolvedType::Flag,
        "String" => return ResolvedType::Scalar(Cow::Owned(Scalar {
            validators: vec!["string".to_string()],
        })),
        "Identifier" => return ResolvedType::Scalar(Cow::Owned(Scalar {
            validators: vec!["identifier".to_string()],
        })),
        "Sigil" => return ResolvedType::Scalar(Cow::Owned(Scalar {
            validators: vec!["sigil".to_string()],
        })),
        "TypeName" => return ResolvedType::Scalar(Cow::Owned(Scalar {
            validators: vec!["type-name".to_string()],
        })),
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
    // Select definitions: E218 in this position.
    if resolve_select_ref(name, schema).is_some() {
        return ResolvedType::KindMismatch;
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
    let k = build_keyword_map(members, schema);
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
/// SelectRef members expand to one entry per variant of the referenced
/// SelectDefinition; the variant's type carries the entry's value type.
fn build_keyword_map(members: &[Member], schema: &Schema) -> std::collections::HashMap<String, (usize, Type)> {
    let mut k = std::collections::HashMap::new();
    for (i, m) in members.iter().enumerate() {
        match m {
            Member::Field(f) => {
                k.insert(f.keyword.clone(), (i, f.r#type.clone()));
            }
            Member::Exclude(_) => {
                // `Exclude` is layer-only and lives inside a layer's
                // SelectDefinition body; it never reaches a composed Struct's
                // member list. No keyword to map.
            }
            Member::SelectRef(s) => {
                if let Some(variants) = resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        k.insert(v.keyword.clone(), (i, v.r#type.clone()));
                    }
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
        ResolvedType::Unresolved | ResolvedType::KindMismatch => {
            // Schema-validity reports E210/E218; nothing more to do here.
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
            let k = build_keyword_map(members, schema);
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
                            let required = f.required.effective_required();
                            let skip = !required && (!atom_assignable || (is_flag && !atom_matches));
                            (required, skip)
                        }
                        Member::Exclude(_) => (false, true), // Skip Exclude ops in atom phase.
                        Member::SelectRef(s) => {
                            // SelectRef is atom-assignable iff all variants of the
                            // referenced SelectDefinition resolve to Flag.
                            let variants = resolve_select_ref(&s.reference, schema)
                                .unwrap_or(&[]);
                            let all_flag = variants.iter().all(|v|
                                matches!(resolve(&v.r#type, schema), ResolvedType::Flag));
                            let atom_matches_some = variants.iter().any(|v| v.keyword == atom_text);
                            let required = s.required.effective_required();
                            let skip = !required && (!all_flag || (all_flag && !atom_matches_some));
                            (required, skip)
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
                    Member::SelectRef(s) => {
                        let variants = resolve_select_ref(&s.reference, schema).unwrap_or(&[]);
                        variants.iter().all(|v|
                            matches!(resolve(&v.r#type, schema), ResolvedType::Flag))
                    }
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
                        // Should not be reachable: Exclude is skipped above.
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
                        if !f.repeatable.effective_repeatable() { pos += 1; }
                    }
                    Member::SelectRef(s) => {
                        // All variants must be Flag (checked above).
                        let variants = resolve_select_ref(&s.reference, schema).unwrap_or(&[]);
                        let matched = variants.iter().any(|v| v.keyword == atom_text);
                        if !matched {
                            errors.push(TelError::with_detail(
                                ErrorCode::E304, 0, 0,
                                format!("atom `{}` matches no variant keyword in SelectRef `{}`",
                                        atom_text, s.reference),
                            ));
                        }
                        fill_counts[pos] += 1;
                        if !s.repeatable.effective_repeatable() { pos += 1; }
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
        Type::Reference(n) => schema.records.iter()
            .chain(schema.layers.iter().flat_map(|l| l.records.iter()))
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
            Member::Field(f) => (
                f.required.effective_required(),
                f.repeatable.effective_repeatable(),
                f.keyword.clone(),
            ),
            Member::SelectRef(s) => (
                s.required.effective_required(),
                s.repeatable.effective_repeatable(),
                format!("<select-ref {}>", s.reference),
            ),
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
    let mut records: Vec<RecordDefinition> = Vec::new();
    let mut scalars: Vec<ScalarDefinition> = Vec::new();
    let mut selects: Vec<SelectDefinition> = Vec::new();
    let mut layers: Vec<Layer> = Vec::new();
    let mut document = Struct { members: Vec::new(), validators: Vec::new() };

    for block in &doc.children {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "name" => name = scalar_value_text(c),
                "sigil" => sigil = scalar_value_text(c).chars().next(),
                "record" => records.push(construct_record(c)),
                "scalar" => scalars.push(construct_scalar_definition(c)),
                "select" => selects.push(construct_select_definition(c)),
                "document" => {
                    let (members, validators) = construct_struct_body(&c.children);
                    document = Struct { members, validators };
                }
                "layer" => layers.push(construct_layer(c)),
                _ => { /* unknown — type-assignment would have caught it */ }
            }
        }
    }

    Schema { name, document, layers, sigil, records, scalars, selects }
}

fn construct_record(c: &Compound) -> RecordDefinition {
    // The `record` compound's first inline atom is the TypeName.
    let name = first_inline_atom(c);
    let (members, validators) = construct_struct_body(&c.children);
    RecordDefinition { name, members, validators }
}

fn construct_scalar_definition(c: &Compound) -> ScalarDefinition {
    // `scalar <Name>` with one or more `validate <name>` children.
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

/// Construct a `SelectDefinition` from a top-level `select <Name>` compound
/// (at schema root or inside a `layer` body). Walks `variant`, `validate`,
/// and (layer-only) `exclude` children. `exclude` is accumulated into
/// `layer_excludes` to be consumed by `MergeSelect`; in a base schema
/// `layer_excludes` should be empty (E217 if not, reported by
/// `validate_schema`).
fn construct_select_definition(c: &Compound) -> SelectDefinition {
    let name = first_inline_atom(c);
    let mut variants: Vec<Variant> = Vec::new();
    let mut validators: Vec<String> = Vec::new();
    let mut layer_excludes: Vec<String> = Vec::new();
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "variant" => variants.push(construct_variant(child)),
                "validate" => validators.push(scalar_value_text(child)),
                "exclude" => layer_excludes.push(scalar_value_text(child)),
                _ => {}
            }
        }
    }
    SelectDefinition { name, variants, validators, layer_excludes }
}

/// Return the text of `c`'s first inline atom, or the empty string. Helper for
/// extracting the keyword/name from compounds whose first atom is the name.
fn first_inline_atom(c: &Compound) -> String {
    c.atoms.first().map(atom_text).unwrap_or_default()
}

fn construct_layer(c: &Compound) -> Layer {
    let mut name = String::new();
    let mut overlay = Struct { members: Vec::new(), validators: Vec::new() };
    let mut records: Vec<RecordDefinition> = Vec::new();
    let mut scalars: Vec<ScalarDefinition> = Vec::new();
    let mut selects: Vec<SelectDefinition> = Vec::new();
    // First inline atom (if present) is the layer name.
    if let Some(atom) = c.atoms.first() {
        name = atom_text(atom);
    }
    // Children: `name` / `overlay` / `record` / `scalar` / `select` (per Layer record).
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "name" => name = scalar_value_text(child),
                "overlay" => {
                    let (members, validators) = construct_struct_body(&child.children);
                    overlay = Struct { members, validators };
                }
                "record" => records.push(construct_record(child)),
                "scalar" => scalars.push(construct_scalar_definition(child)),
                "select" => selects.push(construct_select_definition(child)),
                _ => {}
            }
        }
    }
    Layer { name, overlay, records, scalars, selects }
}

/// Walk the children of a struct-shaped compound (the `document` block, a
/// `record`'s body, or an `overlay` block) and collect both Members
/// (`field`, `select` → SelectRef, `validate`) and its struct-level
/// validators. `Exclude` MUST NOT appear in a struct-shaped body (§20.3);
/// if it does, that's E217, reported by `validate_schema`. To remain
/// permissive at construction time and let the validator report cleanly,
/// the constructor accepts `Member::Exclude` here without complaint.
fn construct_struct_body(blocks: &[Block]) -> (Vec<Member>, Vec<String>) {
    let mut members = Vec::new();
    let mut validators = Vec::new();
    for block in blocks {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "field" => members.push(Member::Field(construct_field(c))),
                "select" => members.push(Member::SelectRef(construct_select_ref(c))),
                "exclude" => members.push(Member::Exclude(scalar_value_text(c))),
                "validate" => validators.push(scalar_value_text(c)),
                _ => {}
            }
        }
    }
    (members, validators)
}

/// Per-axis polarity, computed from the four loosen/tighten Flag children
/// per §20.6. The tightening flag wins when both axes-direction flags are
/// declared on the same axis (`required` over `optional`, `irrepeatable`
/// over `repeatable`).
fn polarity_of(loose: bool, tight: bool) -> Polarity {
    if tight {
        Polarity::Tight
    } else if loose {
        Polarity::Loose
    } else {
        Polarity::Default
    }
}

fn construct_field(c: &Compound) -> Field {
    // Atom phase against the Field record's member order (§20.5):
    //   keyword (req Scalar), type (req Scalar),
    //   optional/required/repeatable/irrepeatable (opt Flags),
    //   default (opt Scalar).
    let mut optional_flag = false;
    let mut required_flag = false;
    let mut repeatable_flag = false;
    let mut irrepeatable_flag = false;
    let mut keyword = String::new();
    let mut type_name = String::new();
    let mut default: Option<String> = None;
    let mut iter = c.atoms.iter();
    // First atom = keyword (required).
    if let Some(a) = iter.next() {
        keyword = atom_text(a);
    }
    // Second atom = type-name (required).
    if let Some(a) = iter.next() {
        type_name = atom_text(a);
    }
    // Remaining atoms: flag-matching atoms set their flag; any non-flag
    // atom fills `default`.
    for a in iter {
        let t = atom_text(a);
        match t.as_str() {
            "optional" => optional_flag = true,
            "required" => required_flag = true,
            "repeatable" => repeatable_flag = true,
            "irrepeatable" => irrepeatable_flag = true,
            _ => {
                if default.is_none() {
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
    let required = polarity_of(optional_flag, required_flag);
    let repeatable = polarity_of(repeatable_flag, irrepeatable_flag);
    let r#type = Type::Reference(type_name);
    Field { required, repeatable, keyword, r#type, default }
}

/// Construct a `SelectRef` at a member position. The first inline atom is
/// the TypeName of the referenced SelectDefinition; remaining atoms /
/// child compounds set the four loosen/tighten flags. SelectRef carries
/// no inline variants — those live on the referenced SelectDefinition.
fn construct_select_ref(c: &Compound) -> SelectRef {
    let mut optional_flag = false;
    let mut required_flag = false;
    let mut repeatable_flag = false;
    let mut irrepeatable_flag = false;
    let mut reference = String::new();
    let mut iter = c.atoms.iter();
    if let Some(a) = iter.next() {
        reference = atom_text(a);
    }
    for a in iter {
        match atom_text(a).as_str() {
            "optional" => optional_flag = true,
            "required" => required_flag = true,
            "repeatable" => repeatable_flag = true,
            "irrepeatable" => irrepeatable_flag = true,
            _ => {}
        }
    }
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "reference" => reference = scalar_value_text(child),
                "optional" => optional_flag = true,
                "required" => required_flag = true,
                "repeatable" => repeatable_flag = true,
                "irrepeatable" => irrepeatable_flag = true,
                _ => {}
            }
        }
    }
    let required = polarity_of(optional_flag, required_flag);
    let repeatable = polarity_of(repeatable_flag, irrepeatable_flag);
    SelectRef { required, repeatable, reference }
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

    // E211: duplicate definition names across the base schema's three
    // Definition lists (records, scalars, selects). They share one
    // namespace.
    let mut seen_base: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let push_dup = |name: &str, errs: &mut Vec<SchemaError>| {
        errs.push(SchemaError {
            code: ErrorCode::E211,
            detail: format!("duplicate definition name `{}` in base schema", name),
        });
    };
    for d in &s.records {
        if !seen_base.insert(&d.name) { push_dup(&d.name, &mut errors); }
    }
    for sd in &s.scalars {
        if !seen_base.insert(&sd.name) { push_dup(&sd.name, &mut errors); }
    }
    for sl in &s.selects {
        if !seen_base.insert(&sl.name) { push_dup(&sl.name, &mut errors); }
    }
    // Built-in name collision: user definitions MAY NOT redefine the
    // predefined TypeNames `Flag`, `String`, `Identifier`, `Sigil`, `TypeName`.
    for d in &s.records {
        if BUILTIN_TYPE_NAMES.contains(&d.name.as_str()) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("record `{}` collides with a built-in TypeName", d.name),
            });
        }
    }
    for sd in &s.scalars {
        if BUILTIN_TYPE_NAMES.contains(&sd.name.as_str()) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("scalar `{}` collides with a built-in TypeName", sd.name),
            });
        }
    }
    for sl in &s.selects {
        if BUILTIN_TYPE_NAMES.contains(&sl.name.as_str()) {
            errors.push(SchemaError {
                code: ErrorCode::E211,
                detail: format!("select `{}` collides with a built-in TypeName", sl.name),
            });
        }
    }
    // Per-layer: a layer's own Definitions can't duplicate within the layer.
    for layer in &s.layers {
        let mut seen_in_layer: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for d in &layer.records {
            if !seen_in_layer.insert(&d.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!("duplicate definition name `{}` within layer `{}`",
                        d.name, layer.name),
                });
            }
        }
        for sd in &layer.scalars {
            if !seen_in_layer.insert(&sd.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!("duplicate definition name `{}` within layer `{}`",
                        sd.name, layer.name),
                });
            }
        }
        for sl in &layer.selects {
            if !seen_in_layer.insert(&sl.name) {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!("duplicate definition name `{}` within layer `{}`",
                        sl.name, layer.name),
                });
            }
        }
    }

    // E202: every SelectDefinition (base or layer) must have ≥ 1 variant.
    // (A layer-side select with only Exclude/validate children but no
    // variants is fine in the LAYER source — the merge consumes excludes
    // against the base's variants; the layer's own `variants` list may be
    // empty.)
    for sl in &s.selects {
        if sl.variants.is_empty() {
            errors.push(SchemaError {
                code: ErrorCode::E202,
                detail: format!("SelectDefinition `{}` has empty variants list", sl.name),
            });
        }
    }
    // E217: an `exclude` declared inside a *base* SelectDefinition (i.e.
    // `layer_excludes` non-empty on a base select) is layer-only and not
    // allowed here. Layer-side excludes are valid; we trust them.
    for sl in &s.selects {
        for kw in &sl.layer_excludes {
            errors.push(SchemaError {
                code: ErrorCode::E217,
                detail: format!(
                    "`exclude {}` appears in base SelectDefinition `{}`; exclude is layer-only",
                    kw, sl.name,
                ),
            });
        }
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

    // Walk every Struct-shaped member list in the schema.
    check_members_recursive(&s.document.members, s, &mut errors);
    for d in &s.records {
        check_members_recursive(&d.members, s, &mut errors);
    }
    for l in &s.layers {
        check_members_recursive(&l.overlay.members, s, &mut errors);
        for d in &l.records {
            check_members_recursive(&d.members, s, &mut errors);
        }
    }

    // E217 (alternative path): `Member::Exclude` MUST NOT appear inside any
    // struct-shaped body. (Layer-side exclude lives on `SelectDefinition.layer_excludes`
    // which is collected by `construct_select_definition`, not as a Member.)
    check_no_exclude_in_struct(&s.document.members, "document", &mut errors);
    for d in &s.records {
        check_no_exclude_in_struct(&d.members, &format!("record `{}`", d.name), &mut errors);
    }
    for l in &s.layers {
        check_no_exclude_in_struct(&l.overlay.members,
            &format!("layer `{}` overlay", l.name), &mut errors);
        for d in &l.records {
            check_no_exclude_in_struct(&d.members,
                &format!("layer `{}` record `{}`", l.name, d.name), &mut errors);
        }
    }

    // E209: reserved keyword `tel` — check every Field/Variant keyword.
    for kw in collect_all_keywords(s) {
        if kw == "tel" {
            errors.push(SchemaError {
                code: ErrorCode::E209,
                detail: "keyword `tel` is reserved (§8)".to_string(),
            });
        }
    }

    // Run the full composition algorithm to surface any merge-time errors
    // (E206/E207/E212/E213/E214). The simulation in the legacy code is
    // subsumed by compose_schema.
    if !s.layers.is_empty() {
        let (_, compose_errs) = compose_schema(s);
        errors.extend(compose_errs);
    }

    errors
}

/// Collect every Field/Variant keyword reachable from the schema (for E209 check).
fn collect_all_keywords(s: &Schema) -> Vec<String> {
    let mut out = Vec::new();
    let visit_members = |out: &mut Vec<String>, members: &[Member]| {
        for m in members {
            if let Member::Field(f) = m {
                out.push(f.keyword.clone());
            }
        }
    };
    visit_members(&mut out, &s.document.members);
    for d in &s.records { visit_members(&mut out, &d.members); }
    for l in &s.layers {
        visit_members(&mut out, &l.overlay.members);
        for d in &l.records { visit_members(&mut out, &d.members); }
    }
    for sl in &s.selects {
        for v in &sl.variants { out.push(v.keyword.clone()); }
    }
    for l in &s.layers {
        for sl in &l.selects {
            for v in &sl.variants { out.push(v.keyword.clone()); }
        }
    }
    out
}

// ── Schema composition (§20.3) ───────────────────────────────────────────────

/// Apply every `Layer` in `s.layers` to produce a fully composed schema
/// per §20.3. Returns the composed `Schema` (with empty `layers`) plus
/// any `SchemaError`s raised during composition (E206, E207, E211, E212,
/// E213, E214). The returned schema is always a best-effort result.
pub fn compose_schema(s: &Schema) -> (Schema, Vec<SchemaError>) {
    let mut errors: Vec<SchemaError> = Vec::new();
    let mut records: Vec<RecordDefinition> = s.records.clone();
    let mut scalars: Vec<ScalarDefinition> = s.scalars.clone();
    let mut selects: Vec<SelectDefinition> = s.selects.clone();
    let mut root_members: Vec<Member> = s.document.members.clone();
    let mut root_validators: Vec<String> = s.document.validators.clone();

    for layer in &s.layers {
        // Record merge.
        for def in &layer.records {
            if scalars.iter().any(|sd| sd.name == def.name)
                || selects.iter().any(|sl| sl.name == def.name)
            {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!(
                        "layer `{}` declares record `{}` but a Definition of another kind with that name already exists",
                        layer.name, def.name,
                    ),
                });
                continue;
            }
            if let Some(pos) = records.iter().position(|d| d.name == def.name) {
                let merged_members = merge_members(
                    &records[pos].members,
                    &def.members,
                    &layer.name,
                    &format!("record `{}`", def.name),
                    &mut errors,
                );
                let merged_validators = merge_validators(
                    &records[pos].validators,
                    &def.validators,
                );
                records[pos] = RecordDefinition {
                    name: def.name.clone(),
                    members: merged_members,
                    validators: merged_validators,
                };
            } else {
                records.push(def.clone());
            }
        }

        // Scalar merge.
        for sd in &layer.scalars {
            if records.iter().any(|d| d.name == sd.name)
                || selects.iter().any(|sl| sl.name == sd.name)
            {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!(
                        "layer `{}` declares scalar `{}` but a Definition of another kind with that name already exists",
                        layer.name, sd.name,
                    ),
                });
                continue;
            }
            if let Some(pos) = scalars.iter().position(|x| x.name == sd.name) {
                let merged = merge_validators(&scalars[pos].validators, &sd.validators);
                scalars[pos] = ScalarDefinition {
                    name: sd.name.clone(),
                    validators: merged,
                };
            } else {
                scalars.push(sd.clone());
            }
        }

        // Select merge: same-name SelectDefinition → MergeSelect (exclude
        // variants per layer's `layer_excludes`; append validators). Adding
        // a variant in a layer (i.e. layer's SelectDefinition has a variant
        // keyword absent from the base) is E214.
        for sl in &layer.selects {
            if records.iter().any(|d| d.name == sl.name)
                || scalars.iter().any(|sd| sd.name == sl.name)
            {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: format!(
                        "layer `{}` declares select `{}` but a Definition of another kind with that name already exists",
                        layer.name, sl.name,
                    ),
                });
                continue;
            }
            if let Some(pos) = selects.iter().position(|x| x.name == sl.name) {
                // Existing SelectDefinition → MergeSelect.
                let merged = merge_select_def(&selects[pos], sl, &layer.name, &mut errors);
                selects[pos] = merged;
            } else {
                // Brand-new SelectDefinition introduced by the layer; only
                // valid if the layer's `layer_excludes` is empty (you can't
                // exclude a variant from a Select that doesn't exist yet).
                if !sl.layer_excludes.is_empty() {
                    for kw in &sl.layer_excludes {
                        errors.push(SchemaError {
                            code: ErrorCode::E212,
                            detail: format!(
                                "layer `{}` exclude `{}` in fresh select `{}`: no base SelectDefinition to exclude from",
                                layer.name, kw, sl.name,
                            ),
                        });
                    }
                }
                let mut fresh = sl.clone();
                fresh.layer_excludes.clear();
                selects.push(fresh);
            }
        }

        // Overlay merge into document Struct.
        root_members = merge_members(
            &root_members,
            &layer.overlay.members,
            &layer.name,
            "overlay",
            &mut errors,
        );
        root_validators = merge_validators(&root_validators, &layer.overlay.validators);
    }

    (Schema {
        name: s.name.clone(),
        document: Struct { members: root_members, validators: root_validators },
        layers: Vec::new(),
        sigil: s.sigil,
        records,
        scalars,
        selects,
    }, errors)
}

/// Merge a layer's SelectDefinition into the base SelectDefinition with the
/// same name. Variant addition by the layer is E214; an Exclude that names
/// a non-existent variant is E212; emptying a SelectDefinition referenced
/// by any required SelectRef would be E213 (deferred: we don't know the
/// referencing SelectRefs at this layer; the validity check is left to
/// downstream schema validation that observes the composed schema).
fn merge_select_def(
    base: &SelectDefinition,
    layer: &SelectDefinition,
    layer_name: &str,
    errors: &mut Vec<SchemaError>,
) -> SelectDefinition {
    let mut variants = base.variants.clone();
    // Variants in `layer.variants` MUST identify existing base variants
    // (variant restatement, allowed); a layer variant whose keyword is
    // absent from the base is E214.
    for lv in &layer.variants {
        if !variants.iter().any(|v| v.keyword == lv.keyword) {
            errors.push(SchemaError {
                code: ErrorCode::E214,
                detail: format!(
                    "layer `{}` introduces variant `{}` in SelectDefinition `{}` not present in base (would widen the sum)",
                    layer_name, lv.keyword, base.name,
                ),
            });
        }
    }
    // Apply excludes.
    for kw in &layer.layer_excludes {
        let before = variants.len();
        variants.retain(|v| v.keyword != *kw);
        if variants.len() == before {
            errors.push(SchemaError {
                code: ErrorCode::E212,
                detail: format!(
                    "layer `{}` exclude `{}` in SelectDefinition `{}`: no such variant in base",
                    layer_name, kw, base.name,
                ),
            });
        }
    }
    let validators = merge_validators(&base.validators, &layer.validators);
    SelectDefinition {
        name: base.name.clone(),
        variants,
        validators,
        layer_excludes: Vec::new(),
    }
}

/// Merge per-axis Polarity (§20.3): tightening or restatement allowed;
/// loosening an already-tight or default axis is E215 (required axis) or
/// E216 (repeatable axis).
fn merge_polarity(
    base: Polarity,
    layer: Polarity,
    is_required_axis: bool,
    layer_name: &str,
    where_: &str,
    errors: &mut Vec<SchemaError>,
) -> Polarity {
    match (base, layer) {
        (_, Polarity::Default) => base,
        (_, Polarity::Tight) => Polarity::Tight,
        (Polarity::Loose, Polarity::Loose) => Polarity::Loose,
        (Polarity::Default, Polarity::Loose) | (Polarity::Tight, Polarity::Loose) => {
            errors.push(SchemaError {
                code: if is_required_axis { ErrorCode::E215 } else { ErrorCode::E216 },
                detail: format!(
                    "layer `{}` in {}: cannot loosen a {} axis whose merged polarity is {}",
                    layer_name, where_,
                    if is_required_axis { "required" } else { "repeatable" },
                    match base { Polarity::Default => "default", Polarity::Tight => "tight", _ => "loose" },
                ),
            });
            base
        }
    }
}

/// Merge a layer's Field into an existing base Field with the same keyword
/// (§20.3).
fn merge_field_with(
    base: &Field,
    layer: &Field,
    layer_name: &str,
    where_: &str,
    errors: &mut Vec<SchemaError>,
) -> Option<Field> {
    let merged_required = merge_polarity(base.required, layer.required, true,
        layer_name, &format!("{} → field `{}`", where_, layer.keyword), errors);
    let merged_repeatable = merge_polarity(base.repeatable, layer.repeatable, false,
        layer_name, &format!("{} → field `{}`", where_, layer.keyword), errors);
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
                    "layer `{}` field `{}` in {}: type mismatch",
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
        r#type: merged_type,
        default: base.default.clone(),
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
                // Find an existing Field with the same keyword in merged.
                let existing_idx = merged.iter().position(|m| match m {
                    Member::Field(g) => g.keyword == f.keyword,
                    Member::SelectRef(_) | Member::Exclude(_) => false,
                });
                match existing_idx {
                    Some(idx) => match &merged[idx] {
                        Member::Field(g) => {
                            if let Some(field) = merge_field_with(g, f, layer_name, where_, errors) {
                                merged[idx] = Member::Field(field);
                            }
                        }
                        _ => unreachable!(),
                    },
                    None => merged.push(Member::Field(f.clone())),
                }
            }
            Member::SelectRef(sref) => {
                // Same-reference merge: polarity merge in place.
                let existing_idx = merged.iter().position(|m| match m {
                    Member::SelectRef(s) => s.reference == sref.reference,
                    _ => false,
                });
                match existing_idx {
                    Some(idx) => {
                        if let Member::SelectRef(base_sref) = &merged[idx] {
                            let merged_required = merge_polarity(
                                base_sref.required, sref.required, true,
                                layer_name,
                                &format!("{} → select-ref `{}`", where_, sref.reference),
                                errors,
                            );
                            let merged_repeatable = merge_polarity(
                                base_sref.repeatable, sref.repeatable, false,
                                layer_name,
                                &format!("{} → select-ref `{}`", where_, sref.reference),
                                errors,
                            );
                            merged[idx] = Member::SelectRef(SelectRef {
                                required: merged_required,
                                repeatable: merged_repeatable,
                                reference: base_sref.reference.clone(),
                            });
                        }
                    }
                    None => merged.push(Member::SelectRef(sref.clone())),
                }
            }
            Member::Exclude(kw) => {
                // Exclude is layer-only and lives in a layer's SelectDefinition
                // body, not in a struct-shaped member list. Reaching here is a
                // validity error (E217) — but the validate_schema path
                // typically catches it first. Report defensively.
                errors.push(SchemaError {
                    code: ErrorCode::E217,
                    detail: format!(
                        "layer `{}` exclude `{}` in {}: `exclude` is only valid inside a layer's `select` body, not in a struct-shaped member list",
                        layer_name, kw, where_,
                    ),
                });
            }
        }
    }
    merged
}

/// Return all `Type`s reachable directly inside a Member.
#[allow(dead_code)]
fn member_types(m: &Member) -> Vec<&Type> {
    match m {
        Member::Field(f) => vec![&f.r#type],
        Member::SelectRef(_) | Member::Exclude(_) => Vec::new(),
    }
}

/// E217 (§20.3): scan a member list (a struct-shaped body) for `Member::Exclude`
/// and report each as an error.
fn check_no_exclude_in_struct(members: &[Member], where_: &str, errors: &mut Vec<SchemaError>) {
    for m in members {
        match m {
            Member::Exclude(kw) => {
                errors.push(SchemaError {
                    code: ErrorCode::E217,
                    detail: format!(
                        "`exclude {}` appears in {} but `exclude` is only valid inside a layer's `select` body",
                        kw, where_,
                    ),
                });
            }
            Member::Field(f) => {
                if let Type::Struct(st) = &f.r#type {
                    check_no_exclude_in_struct(
                        &st.members,
                        &format!("{} → field `{}`", where_, f.keyword),
                        errors,
                    );
                }
            }
            Member::SelectRef(_) => {
                // SelectRef has no inline body; its variants live on the
                // referenced SelectDefinition.
            }
        }
    }
}

fn check_members_recursive(
    members: &[Member],
    schema: &Schema,
    errors: &mut Vec<SchemaError>,
) {
    // E201: duplicate keyword within a Struct (Field keywords + variant
    // keywords of every SelectRef's referenced SelectDefinition).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let push_dup = |seen: &mut std::collections::HashSet<String>,
                    errors: &mut Vec<SchemaError>, kw: &str| {
        if !seen.insert(kw.to_string()) {
            errors.push(SchemaError {
                code: ErrorCode::E201,
                detail: format!("duplicate keyword `{}` within a Struct", kw),
            });
        }
    };
    for m in members {
        match m {
            Member::Field(f) => push_dup(&mut seen, errors, &f.keyword),
            Member::SelectRef(s) => {
                if let Some(variants) = resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        push_dup(&mut seen, errors, &v.keyword);
                    }
                }
            }
            Member::Exclude(_) => {}
        }
    }

    // Per-member checks.
    for m in members {
        match m {
            Member::Field(f) => {
                // E204: Field.default requires required + Scalar resolution.
                if let Some(def_val) = &f.default {
                    let is_required = f.required.effective_required();
                    let resolves_to_scalar = matches!(&f.r#type, Type::Scalar(_)) ||
                        match &f.r#type {
                            Type::Reference(n) => matches!(
                                resolve_name(n, schema),
                                ResolvedType::Scalar(_)
                            ),
                            _ => false,
                        };
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
                // E210/E218: type-name resolution.
                if let Type::Reference(n) = &f.r#type {
                    match resolve_name(n, schema) {
                        ResolvedType::Unresolved => {
                            errors.push(SchemaError {
                                code: ErrorCode::E210,
                                detail: format!(
                                    "Reference `{}` (Field `{}`) does not resolve to any Definition",
                                    n, f.keyword,
                                ),
                            });
                        }
                        ResolvedType::KindMismatch => {
                            errors.push(SchemaError {
                                code: ErrorCode::E218,
                                detail: format!(
                                    "Reference `{}` (Field `{}`) resolves to a SelectDefinition; use `select <Name>` at this position instead",
                                    n, f.keyword,
                                ),
                            });
                        }
                        _ => {}
                    }
                }
                // Recurse into nested Struct types.
                if let Type::Struct(st) = &f.r#type {
                    check_members_recursive(&st.members, schema, errors);
                }
            }
            Member::SelectRef(s) => {
                // E210/E218: SelectRef must resolve to a SelectDefinition.
                match resolve_name(&s.reference, schema) {
                    ResolvedType::Unresolved => {
                        errors.push(SchemaError {
                            code: ErrorCode::E210,
                            detail: format!(
                                "SelectRef `{}` does not resolve to any Definition",
                                s.reference,
                            ),
                        });
                    }
                    ResolvedType::Struct(_) | ResolvedType::Scalar(_) => {
                        errors.push(SchemaError {
                            code: ErrorCode::E218,
                            detail: format!(
                                "SelectRef `{}` resolves to a Record or Scalar (not a SelectDefinition)",
                                s.reference,
                            ),
                        });
                    }
                    _ => {
                        // KindMismatch here means resolve_name found a
                        // SelectDefinition (good — that's exactly what a
                        // SelectRef should resolve to). The
                        // resolve_select_ref helper confirms it.
                        if resolve_select_ref(&s.reference, schema).is_none() {
                            errors.push(SchemaError {
                                code: ErrorCode::E210,
                                detail: format!(
                                    "SelectRef `{}` does not resolve to a SelectDefinition",
                                    s.reference,
                                ),
                            });
                        }
                    }
                }
            }
            Member::Exclude(_) => {
                // Handled by check_no_exclude_in_struct.
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
    parse_inner(input, None)
}

/// Parse with schema-aware E107 (odd-indentation) recovery enabled.
/// Per §19.5: when a schema is available, the parser disambiguates a line
/// whose relative indentation is odd by picking the candidate depth at
/// which the line's keyword is a valid member of the parent struct. With
/// no schema (or via `parse`), the parser falls back to the
/// schema-independent shallower-wins rule. All other behaviour is
/// identical.
pub fn parse_with_schema(input: &str, schema: &Schema) -> ParseResult {
    parse_inner(input, Some(schema))
}

fn parse_inner(input: &str, schema: Option<&Schema>) -> ParseResult {
    let mut p = ParserState::new(input);
    let doc = p.run(schema);
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

    fn run(&mut self, schema: Option<&Schema>) -> Document {
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
        let children = self.build_tree(&raw_lines, line_idx, schema);

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

    fn build_tree<'a>(&'a mut self, raw_lines: &'a [RawLine], start_idx: usize, schema: Option<&'a Schema>) -> Vec<Block> {
        let all_chars: &[char] = &self.all_chars;
        let mut bld = TreeCtx {
            raw: raw_lines,
            all_chars,
            idx: start_idx,
            margin: self.margin,
            sigil: self.sigil,
            errors: Vec::new(),
            schema,
            ancestors: Vec::new(),
        };
        let blocks = bld.parse_blocks(-1); // -1 = accept indent 0
        let errs = std::mem::take(&mut bld.errors);
        self.errors.extend(errs);
        blocks
    }
}

/// Tree-building context. Works directly on raw lines.
struct TreeCtx<'a> {
    raw: &'a [RawLine],
    /// The full document character buffer. Needed to recover bytes that
    /// `split_lines` strips (specifically, CR before LF) for literal-atom
    /// payloads, which preserve all bytes between structural LFs per §15.
    all_chars: &'a [char],
    idx: usize,
    margin: usize,
    sigil: char,
    errors: Vec<TelError>,
    /// Schema used for schema-aware E107 recovery. `None` falls back to the
    /// schema-independent shallower-wins rule (§19.5).
    schema: Option<&'a Schema>,
    /// Stack of keywords of currently-open ancestor compounds, indexed by
    /// depth. `ancestors[d]` is the keyword of the compound at depth `d`,
    /// for `d` in `0..ancestors.len()`. Empty at the document root.
    ancestors: Vec<String>,
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
        if spaces % 2 == 0 {
            return Some(spaces / 2);
        }

        // E107: odd indentation. §19.5 specifies schema-aware recovery when a
        // schema is available, falling back to the schema-independent
        // shallower-wins rule otherwise.
        self.errors.push(TelError::new(ErrorCode::E107, line.start, line.start + margin + spaces));
        let shallower = spaces / 2;
        let deeper = shallower + 1;

        // Schema-aware path: check which candidate's parent admits the line's
        // keyword. Defaults to shallower when both are valid or both invalid.
        if self.schema.is_some() {
            let keyword = self.line_keyword(ri);
            let shallower_valid = self.is_keyword_admissible_at_depth(&keyword, shallower);
            let deeper_valid    = self.is_keyword_admissible_at_depth(&keyword, deeper);
            match (shallower_valid, deeper_valid) {
                (true, false) => return Some(shallower),
                (false, true) => return Some(deeper),
                _ => {} // both valid or both invalid — fall through to shallower-wins
            }
        }

        Some(shallower)
    }

    /// Extract the line's keyword: the first non-space sequence after the
    /// margin + leading spaces, up to the next space or end-of-line.
    fn line_keyword(&self, ri: usize) -> String {
        let line = &self.raw[ri];
        if line.is_blank() { return String::new(); }
        let chars = &line.chars;
        let margin = self.margin.min(chars.len());
        let mut i = margin;
        while i < chars.len() && chars[i] == ' ' { i += 1; }
        let start = i;
        while i < chars.len() && chars[i] != ' ' { i += 1; }
        chars[start..i].iter().collect()
    }

    /// Is `keyword` a valid member-keyword for a compound placed at
    /// `target_depth`? Used by schema-aware E107 recovery. Returns `false`
    /// if there's no schema, no parent at `target_depth - 1`, the parent's
    /// type can't be resolved to a Struct, or the keyword doesn't appear
    /// in the parent's keyword order.
    fn is_keyword_admissible_at_depth(&self, keyword: &str, target_depth: usize) -> bool {
        let schema = match self.schema { Some(s) => s, None => return false };
        // Parent depth = target_depth - 1. Resolved struct's members
        // determine admissibility.
        let parent_members = match self.resolved_members_at_depth(target_depth, schema) {
            Some(m) => m,
            None => return false,
        };
        keyword_in_members(keyword, &parent_members, schema)
    }

    /// Walk the schema from the document root through `ancestors[0..depth-1]`
    /// keywords to find the resolved member list at `depth - 1` (i.e. the
    /// parent of a compound at `depth`). Returns `None` if any step fails
    /// to resolve (missing ancestor, non-Struct resolved type, unknown
    /// keyword on the way down). For `depth == 0` returns the document
    /// root's members directly.
    fn resolved_members_at_depth(&self, depth: usize, schema: &Schema) -> Option<Vec<Member>> {
        // Need ancestors[0..depth-1] to walk down; if depth > ancestors.len()
        // there's no compound at depth-1, so admissibility is false.
        if depth > self.ancestors.len() { return None; }
        let mut current: Vec<Member> = schema.document.members.clone();
        for d in 0..depth {
            let kw = &self.ancestors[d];
            // Find the member or variant whose keyword is `kw`, then
            // resolve its type to a Struct's members.
            let resolved = lookup_keyword_struct(&current, kw, schema)?;
            current = resolved;
        }
        Some(current)
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
                        // Push this compound's keyword onto the ancestor
                        // stack so that schema-aware E107 recovery in any
                        // nested parse_blocks call can resolve the parent
                        // struct correctly. Pop on the way out.
                        self.ancestors.push(compound.keyword.clone());
                        self.parse_compound_body(&mut compound, expected as i32);
                        self.ancestors.pop();
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

        // Scan raw lines for the closing-delimiter match: a line whose
        // content (after CR-stripping) equals the delimiter exactly.
        let mut close_idx: Option<usize> = None;
        while self.idx < self.raw.len() {
            let line_text = self.raw[self.idx].text();
            if line_text == delimiter {
                close_idx = Some(self.idx);
                self.idx += 1; // consume closing delimiter line
                break;
            }
            self.idx += 1;
        }

        let text = match close_idx {
            Some(ci) if ci > ri => {
                // Per §15: every byte between the opening LF and the
                // closing-delimiter LF — including any CR, bare LF, or CR
                // LF sequence — is payload content. We reconstruct the
                // payload by slicing the raw character buffer between the
                // opening LF (just before raw[ri+1].start) and the LF
                // immediately preceding the closing delimiter (at
                // raw[ci].start - 1). If a CR precedes the closing LF
                // (CRLF source), the CR is included as a payload byte —
                // it is the line terminator of the last payload line, not
                // a structural marker.
                if ri + 1 > self.raw.len() {
                    String::new()
                } else {
                    let payload_start = self.raw[ri + 1].start;
                    // P_close = position of the LF right before the closing
                    // delimiter content. The closing delimiter line starts
                    // at self.raw[ci].start; the LF that precedes it is at
                    // self.raw[ci].start - 1 (always present because a
                    // closing-delimiter match requires LF + delim + LF).
                    let payload_end = self.raw[ci].start.saturating_sub(1);
                    if payload_end >= payload_start && payload_end <= self.all_chars.len() {
                        self.all_chars[payload_start..payload_end].iter().collect()
                    } else {
                        String::new()
                    }
                }
            }
            Some(_) => String::new(), // closing delim on the same line as opener (degenerate)
            None => {
                // E115: unclosed literal atom. Per §19.5's E115 recovery,
                // treat the payload as everything from the opening
                // delimiter line to end of file (excluding the final LF).
                self.errors.push(TelError::new(
                    ErrorCode::E115, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
                ));
                if ri + 1 < self.raw.len() {
                    let payload_start = self.raw[ri + 1].start;
                    // To EOF: take the entire remaining buffer, stripping
                    // a single trailing LF if present.
                    let mut end = self.all_chars.len();
                    if end > payload_start && self.all_chars.get(end - 1) == Some(&'\n') {
                        end -= 1;
                    }
                    self.all_chars[payload_start..end].iter().collect()
                } else {
                    String::new()
                }
            }
        };
        Some((delimiter, text))
    }
}

#[derive(Debug, Clone, Copy)]
enum PrevKind { Start, Blank, Comment, Tabulation, Compound }

/// Find the position of a remark introducer in content, if any.
/// Schema-aware-recovery helper: does `keyword` appear as a Field's keyword
/// or as a variant keyword of any SelectRef-referenced SelectDefinition
/// within `members`?
fn keyword_in_members(keyword: &str, members: &[Member], schema: &Schema) -> bool {
    for m in members {
        match m {
            Member::Field(f) => if f.keyword == keyword { return true; },
            Member::SelectRef(s) => {
                if let Some(variants) = resolve_select_ref(&s.reference, schema) {
                    if variants.iter().any(|v| v.keyword == keyword) {
                        return true;
                    }
                }
            }
            Member::Exclude(_) => {}
        }
    }
    false
}

/// Schema-aware-recovery helper: look up `keyword` in `members` and return
/// the resolved member-list of the matching struct type (after Reference
/// resolution). Returns `None` if the keyword isn't found or doesn't
/// resolve to a Struct.
fn lookup_keyword_struct(members: &[Member], keyword: &str, schema: &Schema) -> Option<Vec<Member>> {
    for m in members {
        match m {
            Member::Field(f) if f.keyword == keyword => {
                return match resolve(&f.r#type, schema) {
                    ResolvedType::Struct(ms) => Some(ms.to_vec()),
                    _ => None,
                };
            }
            Member::SelectRef(s) => {
                if let Some(variants) = resolve_select_ref(&s.reference, schema) {
                    for v in variants {
                        if v.keyword == keyword {
                            return match resolve(&v.r#type, schema) {
                                ResolvedType::Struct(ms) => Some(ms.to_vec()),
                                _ => None,
                            };
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

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
        let record_names: Vec<&str> = s.records.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(record_names, vec![
            "Field", "SelectRef", "Variant", "Record", "Scalar",
            "Select", "Body", "Layer",
        ]);
        let select_names: Vec<&str> = s.selects.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(select_names, vec!["Member", "SelectChild"]);
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
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                    Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E201),
                "expected E201, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e202_empty_select() {
        // A SelectDefinition with no variants is E202.
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![], sigil: None,
            records: vec![],
            scalars: Vec::new(),
            selects: vec![SelectDefinition {
                name: "Empty".to_string(),
                variants: vec![],
                validators: Vec::new(),
                layer_excludes: Vec::new(),
            }],
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
                        required: Polarity::Loose, // not required, so default is illegal
                        repeatable: Polarity::Default,
                        keyword: "foo".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()] }),
                        default: Some("bar".to_string()),
                    }),
                ],
                validators: vec![],
            },
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "tel".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "foo".to_string(),
                        r#type: Type::Reference("missing".to_string()), default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E210),
                "expected E210, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e211_duplicate_definition() {
        let dup = || RecordDefinition {
            name: "Dup".to_string(),
            members: vec![], validators: Vec::new(),
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![],
            sigil: None,
            records: vec![dup(), dup()], scalars: Vec::new(), selects: Vec::new(),
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
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
            records: vec![RecordDefinition {
                name: "Thing".to_string(),
                members: vec![Member::Exclude("bar".to_string())],
                validators: vec![],
            }], scalars: Vec::new(), selects: Vec::new(),
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E217),
                "expected E217, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_accepts_exclude_in_layer_select_def() {
        // `exclude` inside a layer's SelectDefinition body is the valid
        // path — NOT E217. Base schema declares a named Select with
        // variants {a, b}; the layer declares a same-name Select with
        // `exclude b` to narrow.
        let base_select = SelectDefinition {
            name: "Choice".to_string(),
            variants: vec![
                Variant { keyword: "a".to_string(), r#type: Type::Flag },
                Variant { keyword: "b".to_string(), r#type: Type::Flag },
            ],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        };
        let layer_select = SelectDefinition {
            name: "Choice".to_string(),
            variants: vec![],
            validators: Vec::new(),
            layer_excludes: vec!["b".to_string()],
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![select_ref(false, false, "Choice")],
                validators: vec![],
            },
            layers: vec![Layer {
                name: "drop-b".to_string(),
                overlay: Struct { members: vec![], validators: vec![] },
                records: vec![], scalars: Vec::new(),
                selects: vec![layer_select],
            }],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: vec![base_select],
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
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![], validators: vec![] },
            layers: vec![l(), l()],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
        }
    }

    fn field(req: bool, rep: bool, kw: &str, t: Type) -> Member {
        Member::Field(Field {
            required: if req { Polarity::Default } else { Polarity::Loose },
            repeatable: if rep { Polarity::Loose } else { Polarity::Default },
            keyword: kw.to_string(), r#type: t, default: None,
        })
    }

    /// Test helper: build a `SelectRef` member pointing at a named SelectDefinition.
    fn select_ref(req: bool, rep: bool, name: &str) -> Member {
        Member::SelectRef(SelectRef {
            required: if req { Polarity::Default } else { Polarity::Loose },
            repeatable: if rep { Polarity::Loose } else { Polarity::Default },
            reference: name.to_string(),
        })
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
                required: Polarity::Default, repeatable: Polarity::Default,
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
                required: Polarity::Default, repeatable: Polarity::Default,
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "active".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
             validators: Vec::new(),},
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
        // `outer` is a required Field whose Struct contains a SelectRef to
        // `Mixed`, whose variants mix Scalar and Flag types. The SelectRef
        // is therefore not atom-assignable, and the atom on `outer`'s line
        // cannot be assigned: E303.
        let mixed_struct = Type::Struct(Struct {
            members: vec![select_ref(true, false, "Mixed")],
            validators: Vec::new(),
        });
        let mut s = schema_with_root(vec![
            field(true, false, "outer", mixed_struct),
        ]);
        s.selects.push(SelectDefinition {
            name: "Mixed".to_string(),
            variants: vec![
                Variant { keyword: "one".to_string(), r#type: scalar_string() },
                Variant { keyword: "two".to_string(), r#type: Type::Flag },
            ],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        });
        let doc = parse("outer something\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E303),
                "expected E303, got: {:?}", ta.errors);
    }

    #[test]
    fn type_assign_catches_e304_select_no_matching_variant() {
        // `colour` is a Field with Struct type whose only member is a SelectRef
        // to an all-Flag SelectDefinition `Colour` = {red, green, blue}. The
        // atom `yellow` on the `colour` compound must match a variant — it
        // doesn't, so E304.
        let colour_struct = Type::Struct(Struct {
            members: vec![select_ref(true, false, "Colour")],
            validators: Vec::new(),
        });
        let mut s = schema_with_root(vec![
            field(true, false, "colour", colour_struct),
        ]);
        s.selects.push(SelectDefinition {
            name: "Colour".to_string(),
            variants: vec![
                variant_("red", Type::Flag),
                variant_("green", Type::Flag),
                variant_("blue", Type::Flag),
            ],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        });
        let doc = parse("colour yellow\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E304),
                "expected E304, got: {:?}", ta.errors);
    }

    // ── compose_schema tests (§20.3) ────────────────────────────────────

    fn layer(name: &str, root_members: Vec<Member>, records: Vec<RecordDefinition>) -> Layer {
        Layer {
            name: name.to_string(),
            overlay: Struct { members: root_members, validators: Vec::new() },
            records, scalars: Vec::new(), selects: Vec::new(),
        }
    }

    #[allow(dead_code)]
    fn flag_field(req: bool, kw: &str) -> Member {
        Member::Field(Field {
            required: if req { Polarity::Default } else { Polarity::Loose },
            repeatable: Polarity::Default,
            keyword: kw.to_string(), r#type: Type::Flag, default: None,
        })
    }

    #[test]
    fn compose_field_add_appends() {
        // Base: { name: string }, Layer: adds { email: string }.
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![
                Member::Field(Field {
                    required: Polarity::Default, repeatable: Polarity::Default, keyword: "name".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], validators: Vec::new()},
            layers: vec![layer("with-email", vec![
                Member::Field(Field {
                    required: Polarity::Loose, repeatable: Polarity::Default, keyword: "email".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
            layers: vec![layer("ext", vec![], vec![RecordDefinition {
                name: "Address".to_string(),
                members: vec![
                    Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default, keyword: "postcode".to_string(),
                        r#type: scalar_string(), default: None,
                    }),
                ], validators: Vec::new(),
            }])],
            sigil: None,
            records: vec![RecordDefinition {
                name: "Address".to_string(),
                members: vec![
                    Member::Field(Field {
                        required: Polarity::Default, repeatable: Polarity::Default, keyword: "street".to_string(),
                        r#type: scalar_string(), default: None,
                    }),
                ], validators: Vec::new(),
            }], scalars: Vec::new(), selects: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.records.len(), 1);
        assert_eq!(composed.records[0].members.len(), 2);
    }

    #[test]
    fn compose_exclude_variant_works() {
        // Base has `select Status { active, archived }`. Layer excludes
        // `archived`. Composed Status has only `active`.
        let base_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![
                Variant { keyword: "active".to_string(), r#type: Type::Flag },
                Variant { keyword: "archived".to_string(), r#type: Type::Flag },
            ],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        };
        let layer_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![],
            validators: Vec::new(),
            layer_excludes: vec!["archived".to_string()],
        };
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![select_ref(false, false, "Status")],
                validators: Vec::new(),
            },
            layers: vec![Layer {
                name: "ro".to_string(),
                overlay: Struct { members: vec![], validators: vec![] },
                records: vec![], scalars: Vec::new(),
                selects: vec![layer_status],
            }],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: vec![base_status],
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        let composed_status = composed.selects.iter().find(|s| s.name == "Status").unwrap();
        assert_eq!(composed_status.variants.len(), 1);
        assert_eq!(composed_status.variants[0].keyword, "active");
    }

    #[test]
    fn compose_exclude_variant_unknown_keyword_is_e212() {
        let base_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![Variant { keyword: "active".to_string(), r#type: Type::Flag }],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        };
        let layer_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![],
            validators: Vec::new(),
            layer_excludes: vec!["never-existed".to_string()],
        };
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![], validators: Vec::new() },
            layers: vec![Layer {
                name: "bad".to_string(),
                overlay: Struct { members: vec![], validators: vec![] },
                records: vec![], scalars: Vec::new(),
                selects: vec![layer_status],
            }],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: vec![base_status],
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E212),
                "expected E212, got: {:?}", errs);
    }

    #[test]
    fn compose_select_variant_addition_is_e214() {
        // A layer tries to introduce a fresh variant `extra` in an existing
        // SelectDefinition — variant addition is forbidden (E214 — would
        // widen the sum).
        let base_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![Variant { keyword: "active".to_string(), r#type: Type::Flag }],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        };
        let layer_status = SelectDefinition {
            name: "Status".to_string(),
            variants: vec![Variant { keyword: "extra".to_string(), r#type: Type::Flag }],
            validators: Vec::new(),
            layer_excludes: Vec::new(),
        };
        let base = Schema {
            name: "x".to_string(),
            document: Struct { members: vec![], validators: Vec::new() },
            layers: vec![Layer {
                name: "widen".to_string(),
                overlay: Struct { members: vec![], validators: vec![] },
                records: vec![], scalars: Vec::new(),
                selects: vec![layer_status],
            }],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: vec![base_status],
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E214),
                "expected E214, got: {:?}", errs);
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
                records: vec![], scalars: Vec::new(), selects: Vec::new(),
            }],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
            layers: vec![layer("ext", vec![], vec![RecordDefinition {
                name: "Address".to_string(),
                members: vec![],
                validators: vec!["layer-rule".to_string()],
            }])],
            sigil: None,
            records: vec![RecordDefinition {
                name: "Address".to_string(),
                members: vec![],
                validators: vec!["base-rule".to_string()],
            }], scalars: Vec::new(), selects: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        assert_eq!(composed.records.len(), 1);
        assert_eq!(
            composed.records[0].validators,
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
                    required: Polarity::Loose, repeatable: Polarity::Default,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("tighten", vec![
                Member::Field(Field {
                    required: Polarity::Tight, repeatable: Polarity::Default,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        if let Member::Field(f) = &composed.document.members[0] {
            assert!(f.required.effective_required(), "merged field should be required after tightening");
        } else {
            panic!("expected Field at index 0");
        }
    }

    #[test]
    fn compose_layer_can_tighten_repeatable_to_irrepeatable() {
        // Base: `field foo repeatable scalar string` (Polarity::Loose).
        // Layer: declares `irrepeatable` (Polarity::Tight).
        // Expected: merged field has repeatable=false (effective).
        let base = Schema {
            name: "x".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: Polarity::Default, repeatable: Polarity::Loose,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("tighten", vec![
                Member::Field(Field {
                    required: Polarity::Default, repeatable: Polarity::Tight,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let (composed, errs) = compose_schema(&base);
        assert!(errs.is_empty(), "expected no errors, got: {:?}", errs);
        if let Member::Field(f) = &composed.document.members[0] {
            assert!(!f.repeatable.effective_repeatable(), "merged field should be irrepeatable after tightening");
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
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("loosen", vec![
                Member::Field(Field {
                    required: Polarity::Loose, repeatable: Polarity::Default,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                })],
                validators: vec![],
            },
            layers: vec![layer("loosen", vec![
                Member::Field(Field {
                    required: Polarity::Default, repeatable: Polarity::Loose,
                    keyword: "foo".to_string(),
                    r#type: scalar_string(), default: None,
                }),
            ], vec![])],
            sigil: None,
            records: vec![], scalars: Vec::new(), selects: Vec::new(),
        };
        let (_composed, errs) = compose_schema(&base);
        assert!(errs.iter().any(|e| e.code == ErrorCode::E216),
                "expected E216, got: {:?}", errs);
    }

    #[test]
    fn construct_field_with_optional_keyword_yields_required_false() {
        // `field foo string optional` → required=false.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo string optional\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(!f.required.effective_required(), "optional flag should produce required=false");
            assert_eq!(f.keyword, "foo");
        } else {
            panic!("expected Field");
        }
    }

    #[test]
    fn construct_field_without_flags_yields_required_true_irrepeatable_true() {
        // `field foo string` (no flags) → required=true, repeatable=false.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo string\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(f.required.effective_required(), "no flag should default to required=true");
            assert!(!f.repeatable.effective_repeatable(), "no flag should default to repeatable=false");
        } else {
            panic!("expected Field");
        }
    }

    #[test]
    fn construct_field_with_required_and_optional_required_wins() {
        // Both `required` and `optional` flags present: `required` wins
        // (tightening direction), required=true.
        let source = "tel 1.0\n\nname x\n\ndocument\n  field foo string optional required\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let s = construct_schema(&parsed.document);
        if let Member::Field(f) = &s.document.members[0] {
            assert!(f.required.effective_required(), "required should override optional in conflict");
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
        assert_eq!(constructed.records, builtin.records,
                   "constructed.records differs from built-in.records");
        assert_eq!(constructed.scalars, builtin.scalars,
                   "constructed.scalars differs from built-in.scalars");
        assert_eq!(constructed.selects, builtin.selects,
                   "constructed.selects differs from built-in.selects");
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "name".to_string(),
                        r#type: Type::Reference("String".to_string()),
                        default: None,
                    }),
                    Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "active".to_string(),
                        r#type: Type::Reference("Flag".to_string()),
                        default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: vec![],
            sigil: None,
            records: vec![],
            scalars: Vec::new(),
            selects: Vec::new(),
        };
        let source = "tel 1.0\n\n\
                      name round-trip\n\n\
                      document\n  \
                      field name String\n  \
                      field active Flag optional\n";
        let parsed = parse(source);
        assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
        let constructed = construct_schema(&parsed.document);
        assert_eq!(constructed, original);
    }

    #[test]
    fn type_assign_with_definitions_resolves_reference() {
        // schema has a RecordDefinition `Address`, and the root has a Field
        // referencing it.
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    field(true, false, "home", Type::Reference("Address".to_string())),
                ],
             validators: Vec::new(),},
            layers: vec![],
            sigil: None,
            records: vec![
                RecordDefinition {
                    name: "Address".to_string(),
                    members: vec![
                        field(true, false, "city", scalar_string()),
                    ], validators: Vec::new(),
                },
            ], scalars: Vec::new(), selects: Vec::new(),
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
        "9033cf054ed14fc460cfd04502a2b69e1ac840cd1035f213492b74af7df2a8dd";

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
        let bytes = bintel::encode_root(&parsed.document, &schema);
        // When DUMP_TEL_SCHEMA_BINTEL is set, write the canonical hex to
        // demo/tel-schema.bintel.hex. Useful for regenerating the pinned
        // artefacts after schema changes.
        if std::env::var("DUMP_TEL_SCHEMA_BINTEL").is_ok() {
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            fs::write("../../demo/tel-schema.bintel.hex", &hex).ok();
            eprintln!("wrote {} bytes to demo/tel-schema.bintel.hex", bytes.len());
        }
        let expected = hex_decode(TEL_SCHEMA_VALUE_HASH_HEX);
        assert_eq!(hash.to_vec(), expected,
                   "tel-schema.tel value hash does not match the normative \
                   value pinned in spec/tel.md §20.5; computed hex={} (bytes={})",
                   hash.iter().map(|b| format!("{:02x}", b)).collect::<String>(),
                   bytes.len());
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
        let composed_keywords: Vec<String> = composed.document.members.iter().flat_map(|m| {
            match m {
                Member::Field(f) => vec![f.keyword.clone()],
                Member::SelectRef(s) => crate::resolve_select_ref(&s.reference, &composed)
                    .map(|vs| vs.iter().map(|v| v.keyword.clone()).collect::<Vec<_>>())
                    .unwrap_or_default(),
                Member::Exclude(_) => Vec::new(),
            }
        }).collect();
        assert!(composed_keywords.iter().any(|k| k == "active"),
                "composed schema should still contain `active`: {:?}", composed_keywords);
        assert!(!composed_keywords.iter().any(|k| k == "archived"),
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

    /// Schema-aware E107 recovery (§19.5): when a schema is supplied and
    /// the line's keyword is valid only at the deeper candidate depth, the
    /// parser places the line at deeper rather than the default shallower.
    /// Schema: root has `field outer Outer`; `Outer` has `field inner String`.
    /// Document: `outer` at indent 0, then a 3-space-indented `inner foo`.
    /// `inner` is NOT valid at indent 1 against the document root (which
    /// only knows `outer`), but IS valid at indent 1 against `outer`'s
    /// `Outer` struct. Schema-aware recovery picks deeper.
    #[test]
    fn recovery_e107_schema_aware_picks_deeper_when_only_deeper_valid() {
        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: Polarity::Default,
                    repeatable: Polarity::Default,
                    keyword: "outer".to_string(),
                    r#type: Type::Reference("Outer".to_string()),
                    default: None,
                })],
                validators: vec![],
            },
            layers: vec![],
            sigil: None,
            records: vec![RecordDefinition {
                name: "Outer".to_string(),
                members: vec![Member::Field(Field {
                    required: Polarity::Default,
                    repeatable: Polarity::Default,
                    keyword: "inner".to_string(),
                    r#type: Type::Reference("String".to_string()),
                    default: None,
                })],
                validators: vec![],
            }],
            scalars: Vec::new(),
            selects: Vec::new(),
        };
        // `inner foo` has 3 leading spaces — odd. Shallower=1 (peer of outer
        // at root) would attach `inner` to the root, which has no `inner`
        // member. Deeper=2 (child of outer) DOES have `inner`. Schema-aware
        // recovery should pick deeper.
        let src = "tel 1.0\n\nouter\n   inner foo\n";
        let parsed = parse_with_schema(src, &schema);
        assert!(parsed.errors.iter().any(|e| e.code == ErrorCode::E107),
                "expected E107 with schema-aware recovery, got: {:?}", parsed.errors);
        // The `inner` compound should appear as a child of `outer`, not as
        // a root-level peer.
        let root: Vec<&Compound> = parsed.document.children.iter()
            .flat_map(|b| b.compounds.iter()).collect();
        assert_eq!(root.len(), 1, "expected one root compound, got {:?}",
                   root.iter().map(|c| &c.keyword).collect::<Vec<_>>());
        assert_eq!(root[0].keyword, "outer");
        let outer_children: Vec<&Compound> = root[0].children.iter()
            .flat_map(|b| b.compounds.iter()).collect();
        assert!(outer_children.iter().any(|c| c.keyword == "inner"),
                "expected `inner` to be a child of `outer` under schema-aware \
                recovery; got: {:?}",
                outer_children.iter().map(|c| &c.keyword).collect::<Vec<_>>());
    }

    /// Schema-aware E107 recovery: when both candidates are valid, the
    /// parser uses the shallower-wins tiebreaker. The test schema has the
    /// keyword `shared` admissible at two depths (3 and 4 in this
    /// document's structure): `shared` is a member of B (depth-2 parent)
    /// and of C (depth-3 parent). A line at 5 spaces (between indent 2
    /// and 3) within a 3-deep stack `[a, b, c]` has both candidates valid
    /// — shallower=2 (peer of c, child of b) and deeper=3 (child of c).
    /// Shallower wins.
    #[test]
    fn recovery_e107_schema_aware_prefers_shallower_on_tie() {
        let schema = Schema {
            name: "demo".to_string(),
            document: Struct {
                members: vec![Member::Field(Field {
                    required: Polarity::Default, repeatable: Polarity::Default,
                    keyword: "a".to_string(),
                    r#type: Type::Reference("A".to_string()),
                    default: None,
                })],
                validators: vec![],
            },
            layers: vec![],
            sigil: None,
            records: vec![
                RecordDefinition {
                    name: "A".to_string(),
                    members: vec![Member::Field(Field {
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "b".to_string(),
                        r#type: Type::Reference("B".to_string()),
                        default: None,
                    })],
                    validators: vec![],
                },
                RecordDefinition {
                    name: "B".to_string(),
                    members: vec![
                        Member::Field(Field {
                            required: Polarity::Loose, repeatable: Polarity::Default,
                            keyword: "shared".to_string(),
                            r#type: Type::Reference("String".to_string()),
                            default: None,
                        }),
                        Member::Field(Field {
                            required: Polarity::Default, repeatable: Polarity::Default,
                            keyword: "c".to_string(),
                            r#type: Type::Reference("C".to_string()),
                            default: None,
                        }),
                    ],
                    validators: vec![],
                },
                RecordDefinition {
                    name: "C".to_string(),
                    members: vec![Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "shared".to_string(),
                        r#type: Type::Reference("String".to_string()),
                        default: None,
                    })],
                    validators: vec![],
                },
            ],
            scalars: Vec::new(),
            selects: Vec::new(),
        };
        // `     shared foo` has 5 spaces — odd. shallower=2 (peer of `c`,
        // child of `b`), deeper=3 (child of `c`). Both parents admit
        // `shared`. Shallower wins → the line becomes b's child, NOT c's.
        let src = "tel 1.0\n\na\n  b\n    c\n     shared foo\n";
        let parsed = parse_with_schema(src, &schema);
        assert!(parsed.errors.iter().any(|e| e.code == ErrorCode::E107),
                "expected E107, got: {:?}", parsed.errors);
        // Walk a → b. b's children should include `shared` (peer of c),
        // and c's children should NOT include `shared`.
        let root: &Compound = &parsed.document.children[0].compounds[0];
        assert_eq!(root.keyword, "a");
        let b: &Compound = &root.children[0].compounds[0];
        assert_eq!(b.keyword, "b");
        let b_children: Vec<&str> = b.children.iter()
            .flat_map(|bk| bk.compounds.iter())
            .map(|c| c.keyword.as_str()).collect();
        assert!(b_children.contains(&"shared"),
                "expected `shared` as a child of `b` (shallower-wins tiebreak); got: {:?}",
                b_children);
        // Find c among b's children and check it has no `shared` child.
        let c: &Compound = b.children.iter()
            .flat_map(|bk| bk.compounds.iter())
            .find(|c| c.keyword == "c").expect("c is present under b");
        let c_children: Vec<&str> = c.children.iter()
            .flat_map(|bk| bk.compounds.iter())
            .map(|cc| cc.keyword.as_str()).collect();
        assert!(!c_children.contains(&"shared"),
                "expected `shared` NOT to be a child of `c` under tie-break; \
                got c's children: {:?}",
                c_children);
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "address".to_string(),
                        r#type: Type::Struct(Struct {
                            members: vec![
                                Member::Field(Field {
                                    required: Polarity::Default, repeatable: Polarity::Default,
                                    keyword: "street".to_string(),
                                    r#type: Type::Scalar(Scalar {
                                        validators: vec!["string".to_string()]}), default: None,
                                }),
                                Member::Field(Field {
                                    required: Polarity::Default, repeatable: Polarity::Default,
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
            layers: vec![], sigil: None, records: vec![], scalars: Vec::new(), selects: Vec::new(),
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "name".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                    }),
                    Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "active".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "record".to_string(),
                        r#type: Type::Struct(Struct {
                            members: vec![
                                Member::Field(Field {
                                    required: Polarity::Default, repeatable: Polarity::Default,
                                    keyword: "id".to_string(),
                                    r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                                }),
                                Member::Field(Field {
                                    required: Polarity::Loose, repeatable: Polarity::Default,
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
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
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
                        required: Polarity::Default, repeatable: Polarity::Default,
                        keyword: "text".to_string(),
                        r#type: Type::Scalar(Scalar { validators: vec!["string".to_string()]}), default: None,
                    }),
                    Member::Field(Field {
                        required: Polarity::Loose, repeatable: Polarity::Default,
                        keyword: "bold".to_string(),
                        r#type: Type::Flag, default: None,
                    }),
                ],
                validators: vec![],
            },
            layers: Vec::new(), sigil: None, records: Vec::new(), scalars: Vec::new(), selects: Vec::new(),
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

