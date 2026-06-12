//! Item catalog: scans the game project's `Items/` tree and parses each item
//! `.tres` (themselves `ItemData` resources) to learn names, grid sizes, valid
//! equipment slots and stack info. Used by the editor's "add item" feature and
//! by the repair engine's grid packing. Entirely optional - if the project
//! path isn't available the editor still works with raw res:// paths.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::character::item_name_from_path;
use crate::tres::{parse, Value};

#[derive(Debug, Clone)]
pub struct ItemInfo {
    /// `res://Items/.../Foo.tres`
    pub res_path: String,
    pub display_name: String,
    /// Grid footprint in cells (width, height). Defaults to (1, 1).
    pub size: (u32, u32),
    pub slots: Vec<String>,
    pub stackable: bool,
    pub default_amount: i64,
    pub max_amount: i64,
}

impl ItemInfo {
    pub fn is_equippable(&self) -> bool {
        !self.slots.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub items: Vec<ItemInfo>,
    by_path: HashMap<String, usize>,
}

impl Catalog {
    pub fn get(&self, res_path: &str) -> Option<&ItemInfo> {
        self.by_path.get(res_path).map(|&i| &self.items[i])
    }

    /// Look up a size, falling back to (1, 1) for unknown items.
    pub fn size_of(&self, res_path: &str) -> (u32, u32) {
        self.get(res_path).map(|i| i.size).unwrap_or((1, 1))
    }

    pub fn name_of(&self, res_path: &str) -> String {
        self.get(res_path)
            .map(|i| i.display_name.clone())
            .unwrap_or_else(|| item_name_from_path(res_path))
    }

    fn index(&mut self) {
        self.by_path.clear();
        for (i, it) in self.items.iter().enumerate() {
            self.by_path.insert(it.res_path.clone(), i);
        }
    }
}

/// Scan `<project_root>/Items` for item resources. Returns an empty catalog if
/// the directory does not exist.
pub fn scan(project_root: &Path) -> Catalog {
    let mut items = Vec::new();
    let items_dir = project_root.join("Items");
    if items_dir.is_dir() {
        let mut files = Vec::new();
        collect_tres(&items_dir, &mut files);
        for path in files {
            if let Some(info) = parse_item(project_root, &path) {
                items.push(info);
            }
        }
    }
    items.sort_by(|a, b| a.res_path.cmp(&b.res_path));
    let mut cat = Catalog { items, by_path: HashMap::new() };
    cat.index();
    cat
}

fn collect_tres(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_tres(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("tres") {
            out.push(path);
        }
    }
}

/// Resource script classes that are items (all extend `ItemData` in-game, so
/// they share its `size` / `slots` / `name` fields). Files in `Items/` with any
/// other class (AudioEvent, TrackData, ...) are not items.
const ITEM_SCRIPT_CLASSES: &[&str] = &[
    "ItemData",
    "WeaponData",
    "AttachmentData",
    "GrenadeData",
    "KnifeData",
    "FishingData",
    "InstrumentData",
    "CasetteData",
    "CatData",
];

fn parse_item(project_root: &Path, path: &Path) -> Option<ItemInfo> {
    let text = fs::read_to_string(path).ok()?;
    let doc = parse(&text).doc;
    let header = doc.sections.first()?;
    let is_item = header
        .header
        .attr_unquoted("script_class")
        .map_or(false, |c| ITEM_SCRIPT_CLASSES.contains(&c.as_str()));
    if !is_item {
        return None;
    }
    let res = doc.resource()?;

    let size = match res.value("size") {
        Some(Value::Vector2(x, y)) => (*x as u32, *y as u32),
        _ => (1, 1),
    };
    let slots = match res.value("slots") {
        Some(Value::Array { items, .. }) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };
    let stackable = res.value("stackable").and_then(|v| v.as_bool()).unwrap_or(false);
    let default_amount = res.value("defaultAmount").and_then(|v| v.as_i64()).unwrap_or(0);
    let max_amount = res.value("maxAmount").and_then(|v| v.as_i64()).unwrap_or(0);

    let res_path = to_res_path(project_root, path)?;
    let display_name = res
        .value("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| item_name_from_path(&res_path));

    Some(ItemInfo {
        res_path,
        display_name,
        size: (size.0.max(1), size.1.max(1)),
        slots,
        stackable,
        default_amount,
        max_amount,
    })
}

fn to_res_path(project_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(project_root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    Some(format!("res://{}", s))
}
