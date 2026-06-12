//! Corruption detection. Produces structured [`Diagnostic`]s describing what is
//! wrong with a save and (where possible) whether the repair engine can fix it.

use std::collections::HashSet;

use crate::character::{is_slotdata, slotdata_fields, SLOT_ARRAYS};
use crate::tres::{parse, Document, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
            Severity::Info => "INFO",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    /// 1-based source line if known.
    pub line: Option<usize>,
    pub message: String,
    /// True if the repair engine knows how to fix this class of problem.
    pub repairable: bool,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn is_ok(&self) -> bool {
        !self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
    pub fn errors(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }
    pub fn warnings(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }
    pub fn has_repairable(&self) -> bool {
        self.diagnostics.iter().any(|d| d.repairable)
    }
}

/// Parse `src` and run all corruption checks.
pub fn validate(src: &str) -> (Document, Report) {
    let parsed = parse(src);
    let doc = parsed.doc;
    let mut diags = Vec::new();

    for note in &parsed.notes {
        let repairable = note.message.contains("truncated `false`")
            || note.message.contains("truncated `true`")
            || note.message.contains("truncated `null`")
            || note.message.contains("empty value");
        diags.push(Diagnostic {
            severity: Severity::Error,
            line: Some(note.line),
            message: note.message.clone(),
            repairable,
        });
    }

    if doc.sections.first().map(|s| s.header.kind.as_str()) != Some("gd_resource") {
        diags.push(Diagnostic {
            severity: Severity::Warning,
            line: Some(1),
            message: "file does not start with a [gd_resource] header".into(),
            repairable: false,
        });
    }

    let mut ext_ids: HashSet<String> = HashSet::new();
    for sec in doc.by_kind("ext_resource") {
        if let Some(id) = sec.id() {
            if !ext_ids.insert(id.clone()) {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    line: Some(sec.line),
                    message: format!("duplicate ext_resource id \"{}\"", id),
                    repairable: false,
                });
            }
        }
    }

    let mut sub_ids: HashSet<String> = HashSet::new();
    for sec in doc.by_kind("sub_resource") {
        if let Some(id) = sec.id() {
            if !sub_ids.insert(id.clone()) {
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    line: Some(sec.line),
                    message: format!("duplicate sub_resource id \"{}\"", id),
                    repairable: false,
                });
            }
        }
    }

    check_references(&doc, &ext_ids, &sub_ids, &mut diags);

    if doc.resource().is_none() {
        diags.push(Diagnostic {
            severity: Severity::Error,
            line: None,
            message: "missing main [resource] block - the file was truncated before the \
                      character body was written. Repair can reconstruct it from the declared \
                      items and recovered slots."
                .into(),
            repairable: true,
        });
    } else {
        check_slotdata_completeness(&doc, &mut diags);
    }

    (doc, Report { diagnostics: diags })
}

fn check_references(
    doc: &Document,
    ext_ids: &HashSet<String>,
    sub_ids: &HashSet<String>,
    diags: &mut Vec<Diagnostic>,
) {
    for sec in &doc.sections {
        for prop in &sec.props {
            visit_refs(&prop.value, &mut |v| match v {
                Value::ExtResource(id) if !ext_ids.contains(id) => {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        line: Some(prop.line),
                        message: format!(
                            "reference to undefined ExtResource(\"{}\") in `{}`",
                            id, prop.key
                        ),
                        repairable: false,
                    });
                }
                Value::SubResource(id) if !sub_ids.contains(id) => {
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        line: Some(prop.line),
                        message: format!(
                            "reference to undefined SubResource(\"{}\") in `{}`",
                            id, prop.key
                        ),
                        repairable: true, // we can drop dangling array entries
                    });
                }
                _ => {}
            });
        }
    }
}

fn visit_refs(v: &Value, f: &mut impl FnMut(&Value)) {
    match v {
        Value::ExtResource(_) | Value::SubResource(_) => f(v),
        Value::Array { elem, items } => {
            if let Some(e) = elem {
                visit_refs(e, f);
            }
            for it in items {
                visit_refs(it, f);
            }
        }
        _ => {}
    }
}

/// Find the ext_resource id whose path points at SlotData.gd.
pub fn slotdata_ext_id(doc: &Document) -> Option<String> {
    doc.by_kind("ext_resource")
        .find(|s| s.header.attr_unquoted("path").as_deref().map_or(false, |p| p.ends_with("SlotData.gd")))
        .and_then(|s| s.id())
}

fn check_slotdata_completeness(doc: &Document, diags: &mut Vec<Diagnostic>) {
    let sd_id = slotdata_ext_id(doc);
    let fields = slotdata_fields();
    for sec in doc.by_kind("sub_resource") {
        if !is_slotdata(sec, sd_id.as_deref()) {
            continue;
        }
        let missing: Vec<&str> = fields
            .iter()
            .filter(|f| sec.get(f.name).is_none())
            .map(|f| f.name)
            .collect();
        if !missing.is_empty() {
            diags.push(Diagnostic {
                severity: Severity::Error,
                line: Some(sec.line),
                message: format!(
                    "SlotData \"{}\" is missing field(s): {} (truncated mid-write)",
                    sec.id().unwrap_or_default(),
                    missing.join(", ")
                ),
                repairable: true,
            });
        }
    }

    // Sanity: the slot arrays should reference only SlotData subresources.
    if let Some(res) = doc.resource() {
        for arr in SLOT_ARRAYS {
            if res.get(arr).is_none() {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    line: Some(res.line),
                    message: format!("CharacterSave is missing its `{}` array", arr),
                    repairable: true,
                });
            }
        }
    }
}
