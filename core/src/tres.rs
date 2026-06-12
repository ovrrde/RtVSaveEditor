//! Parser, model, and writer for Godot `.tres` files. Lossless: each property
//! keeps its original text and only re-serializes when edited, so untouched
//! files round-trip byte-for-byte.

use std::fmt::Write as _;

/// A value on the RHS of a `key = value` line, in an array, or a header attr.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Vector2(f64, f64),
    /// `ExtResource("id")`
    ExtResource(String),
    /// `SubResource("id")`
    SubResource(String),
    /// `Array[<elem>]([<items>])`. `elem` is the type annotation, usually an
    /// `ExtResource(...)` pointing at the element's script.
    Array {
        elem: Option<Box<Value>>,
        items: Vec<Value>,
    },
    /// Anything we couldn't classify (kept verbatim so it round-trips).
    Raw(String),
}

impl Value {
    /// Serialize back to `.tres` syntax.
    pub fn to_tres(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => format_float(*f),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => format!("\"{}\"", s),
            Value::Vector2(x, y) => format!("Vector2({}, {})", format_float(*x), format_float(*y)),
            Value::ExtResource(id) => format!("ExtResource(\"{}\")", id),
            Value::SubResource(id) => format!("SubResource(\"{}\")", id),
            Value::Array { elem, items } => {
                let mut s = String::from("Array[");
                match elem {
                    Some(e) => s.push_str(&e.to_tres()),
                    None => s.push_str("Variant"),
                }
                s.push_str("]([");
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&it.to_tres());
                }
                s.push_str("])");
                s
            }
            Value::Raw(r) => r.clone(),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Format an f64 the way Godot does: always with a decimal point, shortest
/// round-tripping representation otherwise.
pub fn format_float(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "inf_neg".into() };
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// A single `key = value` line inside a section.
#[derive(Debug, Clone)]
pub struct Property {
    pub key: String,
    pub value: Value,
    /// Exact original (or regenerated) RHS text.
    pub raw: String,
    /// 1-based source line, for diagnostics.
    pub line: usize,
    pub edited: bool,
}

impl Property {
    pub fn new(key: impl Into<String>, value: Value) -> Self {
        let raw = value.to_tres();
        Property { key: key.into(), value, raw, line: 0, edited: false }
    }
    /// Replace the value and keep `raw` in sync.
    pub fn set(&mut self, value: Value) {
        self.raw = value.to_tres();
        self.value = value;
        self.edited = true;
    }
}

/// A `[kind attr="x" ...]` header line.
#[derive(Debug, Clone)]
pub struct Header {
    pub kind: String,
    /// Attribute name -> raw value text (including quotes for strings).
    pub attrs: Vec<(String, String)>,
}

impl Header {
    pub fn new(kind: impl Into<String>) -> Self {
        Header { kind: kind.into(), attrs: Vec::new() }
    }
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    /// Attribute value with surrounding quotes stripped.
    pub fn attr_unquoted(&self, key: &str) -> Option<String> {
        self.attr(key).map(|v| v.trim_matches('"').to_string())
    }
    pub fn with_attr(mut self, key: &str, value: &str) -> Self {
        self.attrs.push((key.to_string(), value.to_string()));
        self
    }
    pub fn to_tres(&self) -> String {
        let mut s = format!("[{}", self.kind);
        for (k, v) in &self.attrs {
            let _ = write!(s, " {}={}", k, v);
        }
        s.push(']');
        s
    }
}

/// A header line plus the property lines that follow it.
#[derive(Debug, Clone)]
pub struct Section {
    pub header: Header,
    pub props: Vec<Property>,
    pub line: usize,
}

impl Section {
    pub fn new(header: Header) -> Self {
        Section { header, props: Vec::new(), line: 0 }
    }
    pub fn get(&self, key: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.key == key)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Property> {
        self.props.iter_mut().find(|p| p.key == key)
    }
    pub fn value(&self, key: &str) -> Option<&Value> {
        self.get(key).map(|p| &p.value)
    }
    /// Set a property, replacing it in place or appending if absent.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(p) = self.get_mut(key) {
            p.set(value);
        } else {
            self.props.push(Property::new(key, value));
        }
    }
    pub fn id(&self) -> Option<String> {
        self.header.attr_unquoted("id")
    }
}

/// A whole parsed `.tres` document.
#[derive(Debug, Clone)]
pub struct Document {
    pub sections: Vec<Section>,
    /// Did the file end with a trailing newline? (preserved on write)
    pub trailing_newline: bool,
}

impl Document {
    /// Sections of a given kind (e.g. `"ext_resource"`).
    pub fn by_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Section> {
        self.sections.iter().filter(move |s| s.header.kind == kind)
    }
    pub fn resource(&self) -> Option<&Section> {
        self.sections.iter().find(|s| s.header.kind == "resource")
    }
    pub fn resource_mut(&mut self) -> Option<&mut Section> {
        self.sections.iter_mut().find(|s| s.header.kind == "resource")
    }
    pub fn sub_resource(&self, id: &str) -> Option<&Section> {
        self.by_kind("sub_resource").find(|s| s.id().as_deref() == Some(id))
    }
    pub fn sub_resource_mut(&mut self, id: &str) -> Option<&mut Section> {
        self.sections
            .iter_mut()
            .find(|s| s.header.kind == "sub_resource" && s.id().as_deref() == Some(id))
    }
    pub fn ext_resource(&self, id: &str) -> Option<&Section> {
        self.by_kind("ext_resource").find(|s| s.id().as_deref() == Some(id))
    }

    /// The ext_resource id (as a string) whose `path` matches exactly.
    pub fn ext_id_for_path(&self, path: &str) -> Option<String> {
        self.by_kind("ext_resource")
            .find(|s| s.header.attr_unquoted("path").as_deref() == Some(path))
            .and_then(|s| s.id())
    }

    /// Smallest unused positive integer ext id.
    pub fn next_ext_id(&self) -> i64 {
        self.by_kind("ext_resource")
            .filter_map(|s| s.id())
            .filter_map(|id| id.parse::<i64>().ok())
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Add an ext_resource (creating it after the last existing one) and return
    /// its new id. Reuses an existing entry if the path already exists.
    pub fn add_ext_resource(&mut self, type_: &str, path: &str) -> String {
        if let Some(id) = self.ext_id_for_path(path) {
            return id;
        }
        let id = self.next_ext_id().to_string();
        let header = Header::new("ext_resource")
            .with_attr("type", &format!("\"{}\"", type_))
            .with_attr("path", &format!("\"{}\"", path))
            .with_attr("id", &format!("\"{}\"", id));
        let pos = self
            .sections
            .iter()
            .rposition(|s| s.header.kind == "ext_resource")
            .map(|p| p + 1)
            .unwrap_or(1);
        self.sections.insert(pos, Section::new(header));
        id
    }

    /// Insert a sub_resource section just before the `[resource]` block (or at
    /// the end if there is none yet).
    pub fn insert_sub_resource(&mut self, section: Section) {
        let pos = self
            .sections
            .iter()
            .position(|s| s.header.kind == "resource")
            .unwrap_or(self.sections.len());
        self.sections.insert(pos, section);
    }

    /// Remove a sub_resource section by id.
    pub fn remove_sub_resource(&mut self, id: &str) {
        self.sections
            .retain(|s| !(s.header.kind == "sub_resource" && s.id().as_deref() == Some(id)));
    }

    /// Serialize the whole document back to `.tres` text.
    ///
    /// Godot's layout: a blank line after the `[gd_resource]` header, the
    /// `[ext_resource]` lines packed contiguously, then a blank line before
    /// each `[sub_resource]` / `[resource]` block.
    pub fn to_tres(&self) -> String {
        let mut out = String::new();
        let mut prev_kind: Option<&str> = None;
        for sec in &self.sections {
            let needs_blank = match (prev_kind, sec.header.kind.as_str()) {
                (None, _) => false, // first section
                (Some("ext_resource"), "ext_resource") => false,
                _ => true,
            };
            if needs_blank {
                out.push('\n');
            }
            out.push_str(&sec.header.to_tres());
            out.push('\n');
            for p in &sec.props {
                let _ = writeln!(out, "{} = {}", p.key, p.raw);
            }
            prev_kind = Some(&sec.header.kind);
        }
        if !self.trailing_newline {
            while out.ends_with('\n') {
                out.pop();
            }
        }
        out
    }
}

//parsing

/// A non-fatal issue found while parsing (truncation, malformed value, ...).
#[derive(Debug, Clone)]
pub struct ParseNote {
    pub line: usize,
    pub message: String,
}

/// Result of parsing: a (possibly partial) document plus any parse-time notes.
pub struct ParseOutput {
    pub doc: Document,
    pub notes: Vec<ParseNote>,
}

/// Parse `.tres` source into a [`Document`]. This is intentionally tolerant:
/// malformed or truncated input still yields whatever could be recovered, with
/// the problems recorded as [`ParseNote`]s so the validator can report them.
pub fn parse(src: &str) -> ParseOutput {
    let mut notes = Vec::new();
    let trailing_newline = src.ends_with('\n');
    let lines: Vec<&str> = src.lines().collect();

    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let raw_line = lines[i];
        let line_no = i + 1;
        let trimmed = raw_line.trim_start();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if trimmed.starts_with('[') {
            match parse_header(trimmed) {
                Some(header) => {
                    sections.push(Section { header, props: Vec::new(), line: line_no });
                }
                None => {
                    notes.push(ParseNote {
                        line: line_no,
                        message: format!("malformed section header: {:?}", trimmed),
                    });
                }
            }
            i += 1;
            continue;
        }

        let Some(eq) = raw_line.find('=') else {
            notes.push(ParseNote {
                line: line_no,
                message: format!(
                    "truncated or malformed line (no '='): {:?}",
                    raw_line.trim()
                ),
            });
            i += 1;
            continue;
        };

        let key = raw_line[..eq].trim().to_string();
        let mut rhs = raw_line[eq + 1..].trim().to_string();

        // Handle values that wrap across lines (unbalanced brackets/quotes).
        while !is_balanced(&rhs) && i + 1 < lines.len() {
            let next = lines[i + 1];
            if next.trim_start().starts_with('[') {
                break; // next is a header; current value is genuinely truncated
            }
            rhs.push('\n');
            rhs.push_str(next.trim_end());
            i += 1;
        }

        let (value, note) = parse_value(&rhs);
        if let Some(msg) = note {
            notes.push(ParseNote { line: line_no, message: format!("{} (key `{}`)", msg, key) });
        }

        let prop = Property { key, value, raw: rhs, line: line_no, edited: false };
        if let Some(sec) = sections.last_mut() {
            sec.props.push(prop);
        } else {
            notes.push(ParseNote {
                line: line_no,
                message: "property appears before any section header".into(),
            });
        }
        i += 1;
    }

    ParseOutput { doc: Document { sections, trailing_newline }, notes }
}

/// Parse a header line like `[ext_resource type="Script" path="..." id="1"]`.
fn parse_header(line: &str) -> Option<Header> {
    let inner = line.strip_prefix('[')?;
    let inner = inner.strip_suffix(']').unwrap_or(inner); // tolerate missing ]
    let inner = inner.trim();
    let mut parts = split_top_level_ws(inner);
    if parts.is_empty() {
        return None;
    }
    let kind = parts.remove(0);
    let mut attrs = Vec::new();
    for part in parts {
        if let Some(eq) = part.find('=') {
            let k = part[..eq].trim().to_string();
            let v = part[eq + 1..].trim().to_string();
            attrs.push((k, v));
        }
    }
    Some(Header { kind, attrs })
}

/// Split on whitespace, but keep quoted substrings and bracketed groups intact.
fn split_top_level_ws(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '"' => {
                in_str = !in_str;
                cur.push(c);
            }
            '(' | '[' if !in_str => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' if !in_str => {
                depth -= 1;
                cur.push(c);
            }
            c if c.is_whitespace() && !in_str && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Are quotes, parens and brackets balanced in this fragment? Used to detect
/// values that wrap across lines vs. ones that are truncated at EOF.
fn is_balanced(s: &str) -> bool {
    let mut in_str = false;
    let mut depth = 0i32;
    let mut prev = '\0';
    for c in s.chars() {
        match c {
            '"' if prev != '\\' => in_str = !in_str,
            '(' | '[' if !in_str => depth += 1,
            ')' | ']' if !in_str => depth -= 1,
            _ => {}
        }
        prev = c;
    }
    !in_str && depth == 0
}

/// Parse a single value. Returns the value plus an optional problem note
/// (e.g. a truncated `fals`).
pub fn parse_value(s: &str) -> (Value, Option<String>) {
    let t = s.trim();
    if t.is_empty() {
        return (Value::Raw(String::new()), Some("empty value (truncated?)".into()));
    }

    if let Some(rest) = t.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (Value::Str(rest[..end].to_string()), None);
        }
        return (Value::Raw(t.to_string()), Some("unterminated string (truncated?)".into()));
    }

    if t == "true" {
        return (Value::Bool(true), None);
    }
    if t == "false" {
        return (Value::Bool(false), None);
    }

    if let Some(args) = t.strip_prefix("Vector2(").and_then(|r| r.strip_suffix(')')) {
        let nums: Vec<f64> = args.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if nums.len() == 2 {
            return (Value::Vector2(nums[0], nums[1]), None);
        }
        return (Value::Raw(t.to_string()), Some("malformed Vector2".into()));
    }
    if let Some(arg) = t.strip_prefix("ExtResource(").and_then(|r| r.strip_suffix(')')) {
        return (Value::ExtResource(arg.trim().trim_matches('"').to_string()), None);
    }
    if let Some(arg) = t.strip_prefix("SubResource(").and_then(|r| r.strip_suffix(')')) {
        return (Value::SubResource(arg.trim().trim_matches('"').to_string()), None);
    }
    if t.starts_with("Array[") {
        return parse_array(t);
    }

    // Bare/untyped array, e.g. `["Primary", "Secondary"]` or `[ExtResource("4")]`.
    // Godot writes built-in-typed arrays (Array[String], etc.) and some object
    // arrays in this form, while the save file uses the typed `Array[T]([...])`.
    if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
        let inner = &t[1..t.len() - 1];
        let mut items = Vec::new();
        for part in split_top_level_commas(inner) {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let (v, _) = parse_value(p);
            items.push(v);
        }
        return (Value::Array { elem: None, items }, None);
    }

    if let Ok(i) = t.parse::<i64>() {
        return (Value::Int(i), None);
    }
    if let Ok(f) = t.parse::<f64>() {
        return (Value::Float(f), None);
    }

    // A bare identifier where a value was expected is the classic truncation
    // signature (e.g. `fals` for `false`).
    if t.chars().all(|c| c.is_ascii_alphabetic()) {
        let hint = truncated_keyword_hint(t);
        return (Value::Raw(t.to_string()), Some(hint));
    }

    (Value::Raw(t.to_string()), Some(format!("unrecognized value: {:?}", t)))
}

fn truncated_keyword_hint(t: &str) -> String {
    if "false".starts_with(t) {
        format!("unparseable token `{}` - looks like a truncated `false`", t)
    } else if "true".starts_with(t) {
        format!("unparseable token `{}` - looks like a truncated `true`", t)
    } else if "null".starts_with(t) {
        format!("unparseable token `{}` - looks like a truncated `null`", t)
    } else {
        format!("unparseable bare token `{}` (truncated value?)", t)
    }
}

fn parse_array(t: &str) -> (Value, Option<String>) {
    let rest = &t["Array[".len()..];
    let Some(close_bracket) = find_matching(rest, '[', ']') else {
        return (Value::Raw(t.to_string()), Some("unterminated Array type (truncated?)".into()));
    };
    let elem_text = &rest[..close_bracket];
    let after = rest[close_bracket + 1..].trim_start();
    let inner = after
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .map(|s| s.trim());
    let Some(inner) = inner else {
        return (Value::Raw(t.to_string()), Some("malformed Array body (truncated?)".into()));
    };
    let items_text = inner.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(inner);

    let (elem_val, _) = parse_value(elem_text.trim());
    let elem = Some(Box::new(elem_val));

    let mut items = Vec::new();
    for part in split_top_level_commas(items_text) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let (v, _) = parse_value(p);
        items.push(v);
    }
    (Value::Array { elem, items }, None)
}

fn find_matching(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    for (idx, c) in s.char_indices() {
        match c {
            '"' => in_str = !in_str,
            c if c == open && !in_str => depth += 1,
            c if c == close && !in_str => {
                if depth == 0 {
                    return Some(idx);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    let mut in_str = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_str = !in_str;
                cur.push(c);
            }
            '(' | '[' if !in_str => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' if !in_str => {
                depth -= 1;
                cur.push(c);
            }
            ',' if !in_str && depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}
