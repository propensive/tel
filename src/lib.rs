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
        // Bare base64url hash
        !s.is_empty()
            && s.len() >= 20
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
                let has_errors = !result.errors.is_empty();
                let mut output = format!("{}", result.document);
                if !result.errors.is_empty() {
                    output.push_str("\nerrors:\n");
                    for e in &result.errors {
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
}

