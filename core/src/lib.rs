//! `rtv_save_core` - parsing, validation and repair for Road to Vostok
//! `Character.tres` save files. Pure-std, no external dependencies.

pub mod catalog;
pub mod character;
pub mod edit;
pub mod repair;
pub mod tres;
pub mod validate;

pub use catalog::{Catalog, ItemInfo};
pub use tres::{Document, Header, Property, Section, Value};
pub use validate::{Diagnostic, Report, Severity};

use std::fs;
use std::io;
use std::path::Path;

/// Parse and validate a save file from disk.
pub fn load(path: &Path) -> io::Result<(Document, Report)> {
    let text = fs::read_to_string(path)?;
    Ok(validate::validate(&text))
}

/// Run repair on a document, returning the action log.
pub fn repair_document(doc: &mut Document, catalog: &Catalog) -> repair::RepairLog {
    repair::repair(doc, catalog)
}

/// Write a document to disk, first copying any existing file to `<path>.bak`.
pub fn save_with_backup(doc: &Document, path: &Path) -> io::Result<()> {
    if path.exists() {
        let bak = path.with_extension("tres.bak");
        fs::copy(path, &bak)?;
    }
    fs::write(path, doc.to_tres())
}
