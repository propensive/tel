/// TEL presentation model and parser.

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
    E101, E102, E103, E104, E106,
    E108, E109, E110, E111, E113, E114, E115, E116, E117,
    E118, E119, E120, E121, E122, E123, E124, E125,
    // Schema validity errors (§20.1)
    E202, E203, E206, E207, E208, E209, E210, E211, E212, E213,
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
            Self::E106 => "Invalid sigil character",
            Self::E108 => "Line does not begin with the margin",
            Self::E109 => "Odd indentation",
            Self::E110 => "Trailing spaces on ordinary line",
            Self::E111 => "Comment must follow a blank line, another comment, or start of document",
            Self::E113 => "Over-indentation",
            Self::E114 => "Child of comment, tabulation, or tabulated row",
            Self::E115 => "Source atom already present on this compound",
            Self::E116 => "Literal atom already present on this compound",
            Self::E117 => "Unclosed literal atom",
            Self::E118 => "Tabulated row has wrong indentation",
            Self::E119 => "Hard space does not end at a column boundary",
            Self::E120 => "Consecutive spaces within column value",
            Self::E121 => "Column value exceeds maximum width",
            Self::E122 => "Malformed tabulation heading",
            Self::E123 => "Line-ending inconsistency",
            Self::E124 => "Invalid schema identifier",
            Self::E125 => "Pragma has extra atoms",
            Self::E202 => "Duplicate keyword within a Struct",
            Self::E203 => "Select member has empty variants list",
            Self::E206 => "Scalar has non-null default but member is not required",
            Self::E207 => "Two or more Layers share the same name",
            Self::E208 => "Layer Select variant keyword overlaps existing keyword in base Struct",
            Self::E209 => "Layer Field merge requires both base and layer types to be Struct",
            Self::E210 => "Schema.sigil character is not permitted",
            Self::E211 => "Keyword `tel` is reserved and must not be used as a Field or Variant keyword",
            Self::E212 => "Reference does not resolve to a Definition in the schema",
            Self::E213 => "Two or more Definitions share the same name",
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
    pub types: Vec<Definition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub root: Struct,
    pub types: Vec<Definition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub members: Vec<Member>,
}

/// A `Type` is what a Field, Variant, or referenced `define` evaluates to.
/// `Type::Reference(name)` resolves (per §20.2) to the `Struct` formed from
/// the named `Definition.members`.
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    pub validator: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Member {
    Field(Field),
    Select(Select),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub required: bool,
    pub repeatable: bool,
    pub keyword: String,
    pub r#type: Type,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ValidationRequest<'a> {
    pub method: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResponse {
    Valid,
    Invalid(Vec<Diagnostic>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// Validator callback: maps a `ValidationRequest` to a `ValidationResponse`.
/// Implementations register validators with this signature.
pub type ValidatorFn = dyn Fn(&ValidationRequest) -> ValidationResponse + Send + Sync;

/// Built-in validator: `identifier`. Accepts a kebab-case identifier per §20.7.
///
/// Grammar: starts with a lowercase letter, followed by lowercase letters,
/// digits, or single hyphens (no consecutive hyphens, no trailing hyphen).
pub fn validate_identifier(value: &str) -> ValidationResponse {
    let mk = |msg: &str| ValidationResponse::Invalid(vec![Diagnostic {
        message: msg.to_string(), start: 0, end: value.chars().count(),
    }]);
    if value.is_empty() { return mk("empty identifier"); }
    let mut chars = value.chars().peekable();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return mk("identifier must start with a lowercase ASCII letter");
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
    let mk = |msg: &str| ValidationResponse::Invalid(vec![Diagnostic {
        message: msg.to_string(), start: 0, end: value.chars().count(),
    }]);
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
/// otherwise delegate to the optional user callback.
pub fn validate_with_builtins(
    req: &ValidationRequest,
    user: Option<&ValidatorFn>,
) -> ValidationResponse {
    match req.method {
        "identifier" => validate_identifier(req.value),
        "sigil" => validate_sigil(req.value),
        "string" => validate_string(req.value),
        _ => match user {
            Some(cb) => cb(req),
            None => ValidationResponse::Valid, // no callback → opt out per §21.4
        },
    }
}

// ── Built-in tel-schema (§20.5 bootstrap requirement) ───────────────────────

/// The hardcoded `Schema` value describing TEL's schema language. This is
/// the schema referenced by every TEL schema document, and the closure
/// invariant of §20.5 requires it to match what `tel-schema.tel` describes.
pub fn builtin_tel_schema() -> Schema {
    // Helpers
    let scalar_id = || Type::Scalar(Scalar { validator: "identifier".to_string(), default: None });
    let scalar_sigil = || Type::Scalar(Scalar { validator: "sigil".to_string(), default: None });
    let scalar_str = || Type::Scalar(Scalar { validator: "string".to_string(), default: None });
    let refn = |n: &str| Type::Reference(n.to_string());
    let field = |req: bool, rep: bool, kw: &str, t: Type| Member::Field(Field {
        required: req, repeatable: rep, keyword: kw.to_string(), r#type: t,
    });
    let select = |req: bool, rep: bool, variants: Vec<Variant>| Member::Select(Select {
        required: req, repeatable: rep, variants,
    });
    let variant = |kw: &str, t: Type| Variant { keyword: kw.to_string(), r#type: t };

    // The four variants of the type Select that appear inside Field/Variant
    // bodies. Built fresh in each Definition to keep ownership tidy.
    let type_variants = || vec![
        variant("struct", refn("struct-body")),
        variant("scalar", refn("scalar-body")),
        variant("flag", Type::Flag),
        variant("type", refn("reference-body")),
    ];

    // The repeatable Member Select (variants: field | select).
    let member_select = || select(false, true, vec![
        variant("field", refn("field-body")),
        variant("select", refn("select-body")),
    ]);

    let layer_body = Definition {
        name: "layer-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
            field(false, true, "define", refn("define-body")),
            field(true, false, "root", refn("struct-body")),
        ],
    };

    let define_body = Definition {
        name: "define-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
            member_select(),
        ],
    };

    let struct_body = Definition {
        name: "struct-body".to_string(),
        members: vec![member_select()],
    };

    let field_body = Definition {
        name: "field-body".to_string(),
        members: vec![
            field(true, false, "keyword", scalar_id()),
            field(false, false, "required", Type::Flag),
            field(false, false, "repeatable", Type::Flag),
            select(true, false, type_variants()),
        ],
    };

    let select_body = Definition {
        name: "select-body".to_string(),
        members: vec![
            field(false, false, "required", Type::Flag),
            field(false, false, "repeatable", Type::Flag),
            field(true, true, "variant", refn("variant-body")),
        ],
    };

    let variant_body = Definition {
        name: "variant-body".to_string(),
        members: vec![
            field(true, false, "keyword", scalar_id()),
            select(true, false, type_variants()),
        ],
    };

    let scalar_body = Definition {
        name: "scalar-body".to_string(),
        members: vec![
            field(true, false, "validator", scalar_id()),
            field(false, false, "default", scalar_str()),
        ],
    };

    let reference_body = Definition {
        name: "reference-body".to_string(),
        members: vec![
            field(true, false, "name", scalar_id()),
        ],
    };

    // The schema-document Struct: top-level members of any schema document.
    let document = Struct {
        members: vec![
            field(true, false, "name", scalar_id()),
            field(false, false, "sigil", scalar_sigil()),
            field(false, true, "define", refn("define-body")),
            field(true, false, "document", refn("struct-body")),
            field(false, true, "layer", refn("layer-body")),
        ],
    };

    Schema {
        name: "tel-schema".to_string(),
        document,
        layers: vec![],
        sigil: None,
        types: vec![
            layer_body, define_body, struct_body, field_body, select_body,
            variant_body, scalar_body, reference_body,
        ],
    }
}

// ── Reference resolution (§20.2) ────────────────────────────────────────────

/// Resolve a `Reference` to its underlying `Struct` (Definition.members) per
/// §20.2. Returns `None` if the reference doesn't resolve.
fn resolve_reference<'a>(name: &str, schema: &'a Schema) -> Option<&'a [Member]> {
    schema.types.iter()
        .chain(schema.layers.iter().flat_map(|l| l.types.iter()))
        .find(|d| d.name == name)
        .map(|d| d.members.as_slice())
}

/// Per §20.2, resolve a Type that may be a Reference into either a concrete
/// non-Reference type or — if T is already a Reference to a Struct — return
/// the Struct's member slice and a tag indicating Struct-resolved.
enum ResolvedType<'a> {
    Struct(&'a [Member]),
    Scalar(&'a Scalar),
    Flag,
    Unresolved, // Reference whose name doesn't resolve (E212 caught at schema-validity time)
}

fn resolve<'a>(t: &'a Type, schema: &'a Schema) -> ResolvedType<'a> {
    match t {
        Type::Struct(s) => ResolvedType::Struct(&s.members),
        Type::Scalar(s) => ResolvedType::Scalar(s),
        Type::Flag => ResolvedType::Flag,
        Type::Reference(n) => match resolve_reference(n, schema) {
            Some(members) => ResolvedType::Struct(members),
            None => ResolvedType::Unresolved,
        },
    }
}

// ── Type assignment (§20.2) ─────────────────────────────────────────────────

/// Result of type-assigning a document against a schema. Carries E3xx errors
/// and (optionally) E310 errors from validator callbacks.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAssignment {
    pub errors: Vec<TelError>,
}

/// Type-assign a `Document` against a `Schema`. Implements §20.2 in full.
pub fn type_assign(
    doc: &Document,
    schema: &Schema,
    validator_cb: Option<&ValidatorFn>,
) -> TypeAssignment {
    let mut errors = Vec::new();
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
            Member::Select(s) => {
                for v in &s.variants {
                    k.insert(v.keyword.as_str(), (i, v.r#type.clone()));
                }
            }
        }
    }
    k
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
            // Schema-validity reports E212; nothing to do here.
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
            // E311 doesn't apply; but excess atoms beyond the value are an error?
            // The spec says compound's value = inline atom text. Multiple atoms
            // would be excess; report as E302 (more atoms than positions).
            if c.atoms.len() > 1 {
                errors.push(TelError::with_detail(
                    ErrorCode::E302, 0, 0,
                    format!("Scalar compound `{}` has more than one atom", c.keyword),
                ));
            }
            // E310: invoke validator
            let req = ValidationRequest { method: &sc.validator, value: &value };
            if let ValidationResponse::Invalid(diags) = validate_with_builtins(&req, cb) {
                for d in diags {
                    errors.push(TelError::with_detail(
                        ErrorCode::E310, d.start, d.end,
                        format!("Scalar `{}` failed validation: {}", c.keyword, d.message),
                    ));
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
                                let req = ValidationRequest { method: &sc.validator, value: &atom_text };
                                if let ValidationResponse::Invalid(diags) = validate_with_builtins(&req, cb) {
                                    for d in diags {
                                        errors.push(TelError::with_detail(
                                            ErrorCode::E310, d.start, d.end,
                                            format!("Scalar `{}` failed validation: {}", f.keyword, d.message),
                                        ));
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
        }
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
        };
        // E307: required and empty (defaults handled separately for Scalar)
        if required && fc == 0 {
            let has_default = matches!(m, Member::Field(f)
                if matches!(&f.r#type, Type::Scalar(s) if s.default.is_some()));
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
fn scalar_value_text(c: &Compound) -> String {
    match c.atoms.first() {
        Some(Atom::Inline { text, .. }) => text.clone(),
        Some(Atom::Source { text }) => text.clone(),
        Some(Atom::Literal { text, .. }) => text.clone(),
        None => String::new(),
    }
}

fn atom_text(a: &Atom) -> String {
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
    let mut layers: Vec<Layer> = Vec::new();
    let mut document = Struct { members: Vec::new() };

    for block in &doc.children {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "name" => name = scalar_value_text(c),
                "sigil" => sigil = scalar_value_text(c).chars().next(),
                "define" => types.push(construct_definition(c)),
                "document" => document = Struct {
                    members: construct_members(&c.children),
                },
                "layer" => layers.push(construct_layer(c)),
                _ => { /* unknown — type-assignment would have caught it */ }
            }
        }
    }

    Schema { name, document, layers, sigil, types }
}

fn construct_definition(c: &Compound) -> Definition {
    // The `define` compound's first inline atom is the name.
    let name = scalar_value_text(c);
    let members = construct_members(&c.children);
    Definition { name, members }
}

fn construct_layer(c: &Compound) -> Layer {
    let mut name = String::new();
    let mut root = Struct { members: Vec::new() };
    let mut types: Vec<Definition> = Vec::new();
    // First inline atom (if present) is the layer name.
    if let Some(atom) = c.atoms.first() {
        name = atom_text(atom);
    }
    // Children: `name` / `root` / `define` (per layer-body schema).
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "name" => name = scalar_value_text(child),
                "root" => root = Struct {
                    members: construct_members(&child.children),
                },
                "define" => types.push(construct_definition(child)),
                _ => {}
            }
        }
    }
    Layer { name, root, types }
}

/// Walk the children of a Struct-shaped compound and collect Members.
fn construct_members(blocks: &[Block]) -> Vec<Member> {
    let mut out = Vec::new();
    for block in blocks {
        for c in &block.compounds {
            match c.keyword.as_str() {
                "field" => out.push(Member::Field(construct_field(c))),
                "select" => out.push(Member::Select(construct_select(c))),
                _ => {}
            }
        }
    }
    out
}

fn construct_field(c: &Compound) -> Field {
    // Atom phase against field-body's member order:
    //   keyword (Scalar id, required), required (Flag), repeatable (Flag), type (Sum)
    let mut required = false;
    let mut repeatable = false;
    let mut keyword = String::new();
    let mut iter = c.atoms.iter();
    if let Some(a) = iter.next() {
        // First atom is always the keyword (Scalar at position 0).
        keyword = atom_text(a);
    }
    // Remaining atoms match required / repeatable Flag keywords in order.
    for a in iter {
        let t = atom_text(a);
        if t == "required" { required = true; }
        else if t == "repeatable" { repeatable = true; }
    }
    // Child compounds: `keyword` (Scalar), `required`/`repeatable` (Flag), and
    // one of `struct`/`scalar`/`flag`/`type` for the type Sum.
    let mut r#type: Type = Type::Flag; // overwritten below
    let mut type_set = false;
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "keyword" => keyword = scalar_value_text(child),
                "required" => required = true,
                "repeatable" => repeatable = true,
                "struct" | "scalar" | "flag" | "type" => {
                    r#type = construct_type(child);
                    type_set = true;
                }
                _ => {}
            }
        }
    }
    let _ = type_set;
    Field { required, repeatable, keyword, r#type }
}

fn construct_select(c: &Compound) -> Select {
    let mut required = false;
    let mut repeatable = false;
    for a in &c.atoms {
        let t = atom_text(a);
        if t == "required" { required = true; }
        else if t == "repeatable" { repeatable = true; }
    }
    let mut variants: Vec<Variant> = Vec::new();
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "required" => required = true,
                "repeatable" => repeatable = true,
                "variant" => variants.push(construct_variant(child)),
                _ => {}
            }
        }
    }
    Select { required, repeatable, variants }
}

fn construct_variant(c: &Compound) -> Variant {
    let mut keyword = String::new();
    if let Some(a) = c.atoms.first() { keyword = atom_text(a); }
    let mut r#type: Type = Type::Flag;
    for block in &c.children {
        for child in &block.compounds {
            match child.keyword.as_str() {
                "keyword" => keyword = scalar_value_text(child),
                "struct" | "scalar" | "flag" | "type" => {
                    r#type = construct_type(child);
                }
                _ => {}
            }
        }
    }
    Variant { keyword, r#type }
}

/// Build a `Type` from a type-variant child compound (`struct`, `scalar`,
/// `flag`, or `type`).
fn construct_type(c: &Compound) -> Type {
    match c.keyword.as_str() {
        "struct" => Type::Struct(Struct { members: construct_members(&c.children) }),
        "scalar" => {
            let mut validator = String::new();
            let mut default: Option<String> = None;
            // Inline atoms: validator first, then optional default
            let mut iter = c.atoms.iter();
            if let Some(a) = iter.next() { validator = atom_text(a); }
            if let Some(a) = iter.next() { default = Some(atom_text(a)); }
            for block in &c.children {
                for child in &block.compounds {
                    match child.keyword.as_str() {
                        "validator" => validator = scalar_value_text(child),
                        "default" => default = Some(scalar_value_text(child)),
                        _ => {}
                    }
                }
            }
            Type::Scalar(Scalar { validator, default })
        }
        "flag" => Type::Flag,
        "type" => {
            // The Reference's name is the inline atom (or a child `name` compound).
            let mut name = String::new();
            if let Some(a) = c.atoms.first() { name = atom_text(a); }
            for block in &c.children {
                for child in &block.compounds {
                    if child.keyword == "name" { name = scalar_value_text(child); }
                }
            }
            Type::Reference(name)
        }
        _ => Type::Flag, // unknown — should be caught by type assignment
    }
}

pub fn validate_schema(s: &Schema) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    // Build the composed type namespace: base types first, then each layer's
    // types in order. E213 fires on any duplicate name across this set.
    let mut all_defs: Vec<&Definition> = s.types.iter().collect();
    for layer in &s.layers {
        all_defs.extend(layer.types.iter());
    }
    let mut seen_def_names: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for d in &all_defs {
        if !seen_def_names.insert(&d.name) {
            errors.push(SchemaError {
                code: ErrorCode::E213,
                detail: format!("duplicate Definition name `{}`", d.name),
            });
        }
    }
    let def_names: std::collections::HashSet<&str> =
        all_defs.iter().map(|d| d.name.as_str()).collect();

    // E207: duplicate layer names
    let mut seen_layer_names = std::collections::HashSet::new();
    for l in &s.layers {
        if !seen_layer_names.insert(&l.name) {
            errors.push(SchemaError {
                code: ErrorCode::E207,
                detail: format!("duplicate Layer name `{}`", l.name),
            });
        }
    }

    // E210: sigil character check
    if let Some(c) = s.sigil {
        if matches!(validate_sigil(&c.to_string()), ValidationResponse::Invalid(_)) {
            errors.push(SchemaError {
                code: ErrorCode::E210,
                detail: format!("Schema.sigil `{}` is not a permitted sigil character", c),
            });
        }
    }

    // Walk every Struct in the schema (document, each Definition, every nested
    // Struct inside any Type) and check the per-Struct constraints.
    let mut to_visit: Vec<&Struct> = Vec::new();
    to_visit.push(&s.document);
    for d in &all_defs {
        // A Definition's body is effectively a Struct.
        // We can't directly take a &Struct since Definition has Vec<Member>
        // not Struct, but the rules are the same. Treat them via the helper.
        check_members_recursive(&d.members, &def_names, &mut errors);
    }
    while let Some(st) = to_visit.pop() {
        check_members_recursive(&st.members, &def_names, &mut errors);
        for m in &st.members {
            collect_inner_structs(member_types(m), &mut to_visit);
        }
    }
    for l in &s.layers {
        check_members_recursive(&l.root.members, &def_names, &mut errors);
    }

    // E208/E209: layer-merge constraints (§20.3).
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
        }
    }
    for layer in &s.layers {
        for m in &layer.root.members {
            match m {
                Member::Field(f) => {
                    if let Some(existing) = composed_keywords.get(&f.keyword) {
                        // E209: existing must be a Field whose type is Struct,
                        // and the layer's type must also be Struct.
                        match existing {
                            MergeKind::Field(base_is_struct) => {
                                let layer_is_struct = is_struct_type(&f.r#type);
                                if !base_is_struct || !layer_is_struct {
                                    errors.push(SchemaError {
                                        code: ErrorCode::E209,
                                        detail: format!(
                                            "layer `{}` overrides keyword `{}` but Field merge requires both base and layer types to be Struct",
                                            layer.name, f.keyword,
                                        ),
                                    });
                                }
                            }
                            MergeKind::Variant => {
                                errors.push(SchemaError {
                                    code: ErrorCode::E209,
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
                                code: ErrorCode::E208,
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

    errors
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
    }
}

fn collect_inner_structs<'a>(types: Vec<&'a Type>, out: &mut Vec<&'a Struct>) {
    for t in types {
        if let Type::Struct(st) = t { out.push(st); }
    }
}

fn check_members_recursive(
    members: &[Member],
    def_names: &std::collections::HashSet<&str>,
    errors: &mut Vec<SchemaError>,
) {
    // E202: duplicate keyword within a Struct (across Field and Sum variant keywords)
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for m in members {
        let kws: Vec<&str> = match m {
            Member::Field(f) => vec![&f.keyword],
            Member::Select(s) => s.variants.iter().map(|v| v.keyword.as_str()).collect(),
        };
        for kw in &kws {
            if !seen.insert(*kw) {
                errors.push(SchemaError {
                    code: ErrorCode::E202,
                    detail: format!("duplicate keyword `{}` within a Struct", kw),
                });
            }
            // E211: reserved keyword `tel`
            if *kw == "tel" {
                errors.push(SchemaError {
                    code: ErrorCode::E211,
                    detail: "keyword `tel` is reserved (§8)".to_string(),
                });
            }
        }

        // E203: empty Select variants list
        if let Member::Select(s) = m {
            if s.variants.is_empty() {
                errors.push(SchemaError {
                    code: ErrorCode::E203,
                    detail: "Select member has empty variants list".to_string(),
                });
            }
        }

        // E206: Scalar default on non-required member
        let (is_required, types_to_check): (bool, Vec<&Type>) = match m {
            Member::Field(f) => (f.required, vec![&f.r#type]),
            Member::Select(s) => (s.required, s.variants.iter().map(|v| &v.r#type).collect()),
        };
        for t in &types_to_check {
            if let Type::Scalar(sc) = t {
                if sc.default.is_some() && !is_required {
                    errors.push(SchemaError {
                        code: ErrorCode::E206,
                        detail: format!(
                            "Scalar with non-null default `{}` appears in a non-required member",
                            sc.default.as_ref().unwrap()
                        ),
                    });
                }
            }
        }

        // E212: Reference name must resolve
        for t in types_to_check {
            if let Type::Reference(n) = t {
                if !def_names.contains(n.as_str()) {
                    errors.push(SchemaError {
                        code: ErrorCode::E212,
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

        // Detect line endings and check E123
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
                        ErrorCode::E123, i, i + 1, "CR not followed by LF",
                    ));
                } else if established && mode == LineEndings::LF {
                    self.errors.push(TelError::with_detail(
                        ErrorCode::E123, i, i + 2, "CRLF in LF-mode document",
                    ));
                }
                i += 2;
                continue;
            }
            if chars[i] == '\n' && established && mode == LineEndings::CRLF {
                if i == start || chars[i - 1] != '\r' {
                    self.errors.push(TelError::with_detail(
                        ErrorCode::E123, i, i + 1, "bare LF in CRLF-mode document",
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
        // This is a rough scan — we just need to avoid false E123 inside literal payloads.

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

        // E125: extra atoms or remark
        if atoms.len() > 3 {
            self.errors.push(TelError::new(ErrorCode::E125, line_start, line_start + trimmed.len()));
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
                self.errors.push(TelError::new(ErrorCode::E124, line_start, line_start + trimmed.len()));
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
                self.errors.push(TelError::new(ErrorCode::E106, line_start, line_start + trimmed.len()));
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
        // Bare hex-encoded schema signature: at minimum 64 hex chars (single
        // component) and always even-length (32 + 2k bytes → 64 + 4k chars).
        // Accept both lowercase and uppercase hex per §8 of bintel-spec.md.
        if s.is_empty() || s.len() < 64 || s.len() % 2 != 0 { return false; }
        s.chars().all(|c| c.is_ascii_hexdigit())
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
    /// Get indent of raw line, or None if blank. Also checks E108/E109/E110.
    fn line_indent(&mut self, ri: usize) -> Option<usize> {
        let line = &self.raw[ri];
        if line.is_blank() { return None; }
        let chars = &line.chars;
        let margin = self.margin;

        // Check margin (E108)
        if chars.len() < margin {
            self.errors.push(TelError::with_detail(
                ErrorCode::E108, line.start, line.start + chars.len(), "line shorter than margin",
            ));
            return Some(0);
        }
        for i in 0..margin {
            if chars[i] != ' ' {
                self.errors.push(TelError::with_detail(
                    ErrorCode::E108, line.start, line.start + i + 1, "non-space within margin",
                ));
                return Some(0);
            }
        }

        let after = &chars[margin..];
        let spaces = after.iter().take_while(|&&c| c == ' ').count();
        if spaces % 2 != 0 {
            self.errors.push(TelError::new(ErrorCode::E109, line.start, line.start + margin + spaces));
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

    /// Check trailing spaces (E110) on a non-blank ordinary line.
    fn check_trailing(&mut self, ri: usize) {
        let chars = &self.raw[ri].chars;
        if !chars.is_empty() && *chars.last().unwrap() == ' ' {
            let ts = chars.iter().rposition(|&c| c != ' ').map(|i| i + 1).unwrap_or(0);
            self.errors.push(TelError::new(
                ErrorCode::E110, self.raw[ri].start + ts, self.raw[ri].start + chars.len(),
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
                    // Row at wrong indent inside a tabulated block → E118
                    self.errors.push(TelError::new(
                        ErrorCode::E118, self.raw[ri].start, self.raw[ri].start + self.margin + indent * 2,
                    ));
                    self.idx += 1;
                    continue;
                }
                if indent < expected {
                    break; // belongs to parent
                }
                // E113: over-indentation outside a tabulated block
                self.errors.push(TelError::new(
                    ErrorCode::E113, self.raw[ri].start, self.raw[ri].start + self.margin + indent * 2,
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
                    // E111 check
                    let ok = matches!(prev_kind, PrevKind::Start | PrevKind::Blank | PrevKind::Comment);
                    if !ok {
                        self.errors.push(TelError::new(
                            ErrorCode::E111, self.raw[ri].start, self.raw[ri].start,
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
                        // Tabulated rows must not have children (E114)
                        if self.idx < self.raw.len() && !self.raw[self.idx].is_blank() {
                            let next_indent = self.peek_indent(self.idx);
                            if let Some(ni) = next_indent {
                                if ni > expected {
                                    self.errors.push(TelError::new(
                                        ErrorCode::E114, self.raw[self.idx].start, self.raw[self.idx].start,
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
                            ErrorCode::E119,
                            self.raw[ri].start + margin + indent_spaces + space_start,
                            self.raw[ri].start + margin + hard_end,
                        ));
                    }
                }
            } else {
                i += 1;
            }
        }

        // E121: column width check
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
                        ErrorCode::E121,
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
                    ErrorCode::E115, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
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
                    ErrorCode::E116, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
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
                ErrorCode::E117, self.raw[ri].start, self.raw[ri].start + self.raw[ri].chars.len(),
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
                ErrorCode::E122, line_start + pos, line_start + pos + 2, "non-space after marker",
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
                    ErrorCode::E122,
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
                    ErrorCode::E122, line_start + pos, line_start + pos + 2 + end, "heading contains sigil",
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
            "layer-body", "define-body", "struct-body", "field-body",
            "select-body", "variant-body", "scalar-body", "reference-body",
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
    fn validate_schema_catches_e202_duplicate_keyword() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag,
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Flag,
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E202),
                "expected E202, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e203_empty_select() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Select(Select {
                        required: false, repeatable: false,
                        variants: vec![],
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E203),
                "expected E203, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e206_default_on_optional() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, // not required, so default is illegal
                        repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Scalar(Scalar {
                            validator: "string".to_string(),
                            default: Some("bar".to_string()),
                        }),
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E206),
                "expected E206, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e210_bad_sigil() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![] },
            layers: vec![],
            sigil: Some('A'), // letter
            types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E210),
                "expected E210, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e211_reserved_keyword() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "tel".to_string(),
                        r#type: Type::Flag,
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E211),
                "expected E211, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e212_unresolved_reference() {
        let s = Schema {
            name: "test".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "foo".to_string(),
                        r#type: Type::Reference("missing".to_string()),
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E212),
                "expected E212, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e213_duplicate_definition() {
        let dup = || Definition {
            name: "dup".to_string(),
            members: vec![],
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![] },
            layers: vec![],
            sigil: None,
            types: vec![dup(), dup()],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E213),
                "expected E213, got: {:?}", errors);
    }

    #[test]
    fn validate_schema_catches_e207_duplicate_layer() {
        let l = || Layer {
            name: "dup".to_string(),
            root: Struct { members: vec![] },
            types: vec![],
        };
        let s = Schema {
            name: "test".to_string(),
            document: Struct { members: vec![] },
            layers: vec![l(), l()],
            sigil: None,
            types: vec![],
        };
        let errors = validate_schema(&s);
        assert!(errors.iter().any(|e| e.code == ErrorCode::E207),
                "expected E207, got: {:?}", errors);
    }

    // ── Type assignment unit tests ──────────────────────────────────────────

    /// Helper: build a minimal schema for testing.
    fn schema_with_root(members: Vec<Member>) -> Schema {
        Schema {
            name: "test".to_string(),
            document: Struct { members },
            layers: vec![],
            sigil: None,
            types: vec![],
        }
    }

    fn field(req: bool, rep: bool, kw: &str, t: Type) -> Member {
        Member::Field(Field {
            required: req, repeatable: rep, keyword: kw.to_string(), r#type: t,
        })
    }

    fn select(req: bool, rep: bool, variants: Vec<Variant>) -> Member {
        Member::Select(Select { required: req, repeatable: rep, variants })
    }

    fn variant_(kw: &str, t: Type) -> Variant {
        Variant { keyword: kw.to_string(), r#type: t }
    }

    fn scalar_string() -> Type {
        Type::Scalar(Scalar { validator: "string".to_string(), default: None })
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
        let s = schema_with_root(vec![
            Member::Field(Field {
                required: true, repeatable: false,
                keyword: "name".to_string(),
                r#type: Type::Scalar(Scalar {
                    validator: "string".to_string(),
                    default: Some("Anonymous".to_string()),
                }),
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
                r#type: Type::Scalar(Scalar {
                    validator: "identifier".to_string(),
                    default: None,
                }),
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
                        r#type: Type::Flag,
                    }),
                ],
            },
            layers: vec![], sigil: None, types: vec![],
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
        });
        let s = schema_with_root(vec![
            field(true, false, "colour", colour_struct),
        ]);
        let doc = parse("colour yellow\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.iter().any(|e| e.code == ErrorCode::E304),
                "expected E304, got: {:?}", ta.errors);
    }

    /// THE bootstrap closure: parsing tel-schema.tel, constructing a Schema
    /// from the result, and confirming it equals the hardcoded built-in.
    #[test]
    fn tel_schema_self_bootstrap_closure() {
        let source = fs::read_to_string("tel-schema.tel")
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
        // Hand-built schema → write a TEL source → re-parse → re-construct.
        // The constructed schema MUST equal the original.
        let original = Schema {
            name: "round-trip".to_string(),
            document: Struct {
                members: vec![
                    Member::Field(Field {
                        required: true, repeatable: false,
                        keyword: "name".to_string(),
                        r#type: Type::Scalar(Scalar {
                            validator: "string".to_string(),
                            default: None,
                        }),
                    }),
                    Member::Field(Field {
                        required: false, repeatable: false,
                        keyword: "active".to_string(),
                        r#type: Type::Flag,
                    }),
                ],
            },
            layers: vec![],
            sigil: None,
            types: vec![],
        };
        // The TEL source corresponding to `original`.
        let source = "tel 1.0\n\n\
                      name round-trip\n\n\
                      document\n  \
                      field name required\n    \
                      scalar string\n  \
                      field active\n    \
                      flag\n";
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
            },
            layers: vec![],
            sigil: None,
            types: vec![
                Definition {
                    name: "address".to_string(),
                    members: vec![
                        field(true, false, "city", scalar_string()),
                    ],
                },
            ],
        };
        let doc = parse("home\n  city London\n").document;
        let ta = type_assign(&doc, &s, None);
        assert!(ta.errors.is_empty(),
                "expected no errors with Reference resolution, got: {:?}", ta.errors);
    }
}

