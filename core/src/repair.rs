//! Auto-repair engine. Given a parsed (possibly broken) document, it:
//!   1. fixes truncated literals (`fals` -> `false`),
//!   2. completes `SlotData` sub-resources that were cut off mid-write,
//!   3. drops dangling array references, and
//!   4. reconstructs a missing `[resource]` block from the declared items and
//!      whatever slots survived - the exact scenario that prompted this tool.

use std::collections::HashSet;

use crate::catalog::Catalog;
use crate::character::{is_slotdata, slotdata_fields, FLAG_FIELDS, STAT_FIELDS};
use crate::tres::{Document, Header, Property, Section, Value};
use crate::validate::slotdata_ext_id;

/// Width of the inventory grid in cells (from Interface.tscn: 512px / 64).
const GRID_W: u32 = 8;
const GRID_H: u32 = 12;
const CELL: f64 = 64.0;

#[derive(Debug, Default)]
pub struct RepairLog {
    pub actions: Vec<String>,
}

impl RepairLog {
    fn add(&mut self, msg: impl Into<String>) {
        self.actions.push(msg.into());
    }
}

/// Repair `doc` in place using `catalog` (which may be empty). Returns a log of
/// everything that was changed.
pub fn repair(doc: &mut Document, catalog: &Catalog) -> RepairLog {
    let mut log = RepairLog::default();

    fix_truncated_literals(doc, &mut log);

    let sd_id = slotdata_ext_id(doc);
    complete_slotdata(doc, sd_id.as_deref(), &mut log);

    if doc.resource().is_none() {
        reconstruct_resource(doc, catalog, &mut log);
    } else {
        drop_dangling_refs(doc, &mut log);
    }

    if log.actions.is_empty() {
        log.add("Nothing to repair - file is already structurally sound.");
    }
    log
}

/// Replace `Raw` values that look like truncated keywords with the real thing.
fn fix_truncated_literals(doc: &mut Document, log: &mut RepairLog) {
    for sec in &mut doc.sections {
        for prop in &mut sec.props {
            if let Value::Raw(text) = &prop.value {
                let t = text.trim();
                if t.is_empty() {
                    continue;
                }
                let fixed = if !t.is_empty() && "false".starts_with(t) {
                    Some(Value::Bool(false))
                } else if !t.is_empty() && "true".starts_with(t) {
                    Some(Value::Bool(true))
                } else {
                    None
                };
                if let Some(v) = fixed {
                    log.add(format!(
                        "line {}: fixed truncated `{}` -> `{}` (key `{}`)",
                        prop.line, t, v.to_tres(), prop.key
                    ));
                    prop.set(v);
                }
            }
        }
    }
}

/// Add any missing tail fields to `SlotData` sub-resources, using defaults.
fn complete_slotdata(doc: &mut Document, sd_id: Option<&str>, log: &mut RepairLog) {
    let fields = slotdata_fields();
    for sec in &mut doc.sections {
        if !is_slotdata(sec, sd_id) {
            continue;
        }
        let id = sec.id().unwrap_or_default();
        for f in &fields {
            if sec.get(f.name).is_none() {
                let v = (f.default)();
                log.add(format!(
                    "SlotData \"{}\": added missing field `{} = {}` (default)",
                    id, f.name, v.to_tres()
                ));
                sec.props.push(Property::new(f.name, v));
            }
        }
    }
}

/// Remove SubResource references in arrays that point at undefined ids.
fn drop_dangling_refs(doc: &mut Document, log: &mut RepairLog) {
    let defined: HashSet<String> =
        doc.by_kind("sub_resource").filter_map(|s| s.id()).collect();
    for sec in &mut doc.sections {
        for prop in &mut sec.props {
            if let Value::Array { items, .. } = &mut prop.value {
                let before = items.len();
                items.retain(|it| match it {
                    Value::SubResource(id) => defined.contains(id),
                    _ => true,
                });
                if items.len() != before {
                    let removed = before - items.len();
                    let v = prop.value.clone();
                    prop.set(v);
                    log.add(format!(
                        "line {}: dropped {} dangling reference(s) from `{}`",
                        prop.line, removed, prop.key
                    ));
                }
            }
        }
    }
}

//reconstruct the [resource] block if not found

fn reconstruct_resource(doc: &mut Document, catalog: &Catalog, log: &mut RepairLog) {
    log.add("Reconstructing missing [resource] block...");

    let mut next_ext = max_ext_id(doc) + 1;
    let sd_id = ensure_script_ext(doc, "SlotData.gd", "res://Scripts/SlotData.gd", &mut next_ext, log);
    let it_id = ensure_script_ext(doc, "ItemData.gd", "res://Scripts/ItemData.gd", &mut next_ext, log);
    let cs_id =
        ensure_script_ext(doc, "CharacterSave.gd", "res://Scripts/CharacterSave.gd", &mut next_ext, log);

    // Which item ext_resources are already referenced (as a slot's itemData or
    // nested inside one)? Those must not be duplicated as loose items.
    let used = referenced_item_ext_ids(doc, sd_id.as_deref());

    let mut equipment_ids: Vec<String> = Vec::new();
    let mut inventory_ids: Vec<String> = Vec::new();
    for sec in doc.by_kind("sub_resource") {
        if !is_slotdata(sec, sd_id.as_deref()) {
            continue;
        }
        let slot = sec.value("slot").and_then(|v| v.as_str()).unwrap_or("");
        match sec.id() {
            Some(id) if !slot.is_empty() => equipment_ids.push(id),
            Some(id) => inventory_ids.push(id),
            None => {}
        }
    }
    log.add(format!(
        "Recovered {} equipment slot(s) and {} loose inventory slot(s) from surviving sub-resources.",
        equipment_ids.len(),
        inventory_ids.len()
    ));

    let mut new_sections: Vec<Section> = Vec::new();
    let mut rebuilt = 0usize;
    let item_exts: Vec<(String, String)> = doc
        .by_kind("ext_resource")
        .filter(|s| s.header.attr_unquoted("type").as_deref() == Some("Resource"))
        .filter_map(|s| Some((s.id()?, s.header.attr_unquoted("path")?)))
        .filter(|(_, path)| path.contains("/Items/"))
        .collect();

    for (item_id, path) in &item_exts {
        if used.contains(item_id) {
            continue;
        }
        let info = catalog.get(path);
        let amount = info
            .map(|i| if i.stackable || i.default_amount > 0 { i.default_amount } else { 0 })
            .unwrap_or(0);
        let sub_id = format!("Resource_rebuilt_{}", rebuilt);
        rebuilt += 1;
        let sec = make_inventory_slot(
            &sub_id,
            sd_id.as_deref(),
            it_id.as_deref(),
            item_id,
            amount,
        );
        inventory_ids.push(sub_id);
        new_sections.push(sec);
    }
    if rebuilt > 0 {
        log.add(format!(
            "Recreated {} item(s) that were declared but lost in the truncation, as inventory slots.",
            rebuilt
        ));
    }

    for sec in new_sections {
        doc.sections.push(sec);
    }

    // Pack grid positions for all inventory slots so nothing overlaps.
    pack_inventory(doc, &inventory_ids, catalog, log);

    let equipped_slots: HashSet<String> = equipment_ids
        .iter()
        .filter_map(|id| doc.sub_resource(id))
        .filter_map(|s| s.value("slot").and_then(|v| v.as_str()).map(|x| x.to_string()))
        .collect();

    let mut res = Section::new(Header::new("resource"));
    if let Some(cs) = &cs_id {
        res.props.push(Property::new("script", Value::ExtResource(cs.clone())));
    }
    for (name, def) in STAT_FIELDS {
        res.props.push(Property::new(*name, Value::Float(*def)));
    }
    for (name, def) in FLAG_FIELDS {
        let val = match *name {
            "primary" => equipped_slots.contains("Primary"),
            "secondary" => equipped_slots.contains("Secondary"),
            "knife" => equipped_slots.contains("Knife"),
            "grenade1" => equipped_slots.contains("Grenade_1"),
            "grenade2" => equipped_slots.contains("Grenade_2"),
            "flashlight" => equipped_slots.contains("Light"),
            "NVG" => equipped_slots.contains("NVG"),
            _ => *def,
        };
        res.props.push(Property::new(*name, Value::Bool(val)));
    }
    let typed_array = |ids: &[String]| Value::Array {
        elem: sd_id.as_ref().map(|id| Box::new(Value::ExtResource(id.clone()))),
        items: ids.iter().map(|i| Value::SubResource(i.clone())).collect(),
    };
    res.props.push(Property::new("inventory", typed_array(&inventory_ids)));
    res.props.push(Property::new("equipment", typed_array(&equipment_ids)));
    res.props.push(Property::new("catalog", typed_array(&[])));
    res.props.push(Property::new("weaponPosition", Value::Int(1)));

    doc.sections.push(res);
    log.add(format!(
        "Built [resource]: {} inventory, {} equipment items. Stats set to defaults (100); \
         original stat values could not be recovered.",
        inventory_ids.len(),
        equipment_ids.len()
    ));
}

fn make_inventory_slot(
    sub_id: &str,
    sd_id: Option<&str>,
    it_id: Option<&str>,
    item_id: &str,
    amount: i64,
) -> Section {
    let mut sec = Section::new(Header::new("sub_resource").with_attr("type", "\"Resource\"").with_attr("id", &format!("\"{}\"", sub_id)));
    if let Some(sd) = sd_id {
        sec.props.push(Property::new("script", Value::ExtResource(sd.to_string())));
    }
    sec.props.push(Property::new("itemData", Value::ExtResource(item_id.to_string())));
    let nested_elem = it_id.map(|id| Box::new(Value::ExtResource(id.to_string())));
    sec.props.push(Property::new("nested", Value::Array { elem: nested_elem, items: vec![] }));
    let storage_elem = sd_id.map(|id| Box::new(Value::ExtResource(id.to_string())));
    sec.props.push(Property::new("storage", Value::Array { elem: storage_elem, items: vec![] }));
    sec.props.push(Property::new("condition", Value::Int(100)));
    sec.props.push(Property::new("amount", Value::Int(amount)));
    sec.props.push(Property::new("position", Value::Int(0)));
    sec.props.push(Property::new("mode", Value::Int(1)));
    sec.props.push(Property::new("zoom", Value::Int(1)));
    sec.props.push(Property::new("chamber", Value::Bool(false)));
    sec.props.push(Property::new("casing", Value::Bool(false)));
    sec.props.push(Property::new("state", Value::Str(String::new())));
    sec.props.push(Property::new("gridPosition", Value::Vector2(0.0, 0.0)));
    sec.props.push(Property::new("gridRotated", Value::Bool(false)));
    sec.props.push(Property::new("slot", Value::Str(String::new())));
    sec
}

/// First-fit grid packing. Writes a non-overlapping `gridPosition` into every
/// inventory slot so the loader's `Place()` accepts them all.
fn pack_inventory(doc: &mut Document, inventory_ids: &[String], catalog: &Catalog, log: &mut RepairLog) {
    // Gather (id, w, h) sorted by descending height then width for tighter packing.
    let mut items: Vec<(String, u32, u32)> = inventory_ids
        .iter()
        .map(|id| {
            let path = doc
                .sub_resource(id)
                .and_then(|s| s.value("itemData"))
                .and_then(|v| match v {
                    Value::ExtResource(eid) => doc.ext_resource(eid).and_then(|e| e.header.attr_unquoted("path")),
                    _ => None,
                })
                .unwrap_or_default();
            let (w, h) = catalog.size_of(&path);
            (id.clone(), w, h)
        })
        .collect();
    items.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));

    let mut occ = vec![vec![false; GRID_H as usize]; GRID_W as usize];
    let mut overflow = 0;
    for (id, w, h) in &items {
        if let Some((gx, gy)) = first_fit(&mut occ, *w, *h) {
            if let Some(sec) = doc.sub_resource_mut(id) {
                sec.set("gridPosition", Value::Vector2(gx as f64 * CELL, gy as f64 * CELL));
            }
        } else {
            overflow += 1;
        }
    }
    if overflow > 0 {
        log.add(format!(
            "Note: {} item(s) did not fit the {}x{} inventory grid and may need manual placement.",
            overflow, GRID_W, GRID_H
        ));
    }
}

fn first_fit(occ: &mut [Vec<bool>], w: u32, h: u32) -> Option<(u32, u32)> {
    if w > GRID_W || h > GRID_H {
        return None;
    }
    for y in 0..=(GRID_H - h) {
        'x: for x in 0..=(GRID_W - w) {
            for i in x..x + w {
                for j in y..y + h {
                    if occ[i as usize][j as usize] {
                        continue 'x;
                    }
                }
            }
            for i in x..x + w {
                for j in y..y + h {
                    occ[i as usize][j as usize] = true;
                }
            }
            return Some((x, y));
        }
    }
    None
}

//helpers

fn max_ext_id(doc: &Document) -> i64 {
    doc.by_kind("ext_resource")
        .filter_map(|s| s.id())
        .filter_map(|id| id.parse::<i64>().ok())
        .max()
        .unwrap_or(0)
}

/// Find an ext_resource by path suffix; create one if missing. Returns its id.
fn ensure_script_ext(
    doc: &mut Document,
    suffix: &str,
    path: &str,
    next_id: &mut i64,
    log: &mut RepairLog,
) -> Option<String> {
    if let Some(s) = doc
        .by_kind("ext_resource")
        .find(|s| s.header.attr_unquoted("path").as_deref().map_or(false, |p| p.ends_with(suffix)))
    {
        return s.id();
    }
    let id = next_id.to_string();
    *next_id += 1;
    let header = Header::new("ext_resource")
        .with_attr("type", "\"Script\"")
        .with_attr("path", &format!("\"{}\"", path))
        .with_attr("id", &format!("\"{}\"", id));
    let pos = doc
        .sections
        .iter()
        .rposition(|s| s.header.kind == "ext_resource")
        .map(|p| p + 1)
        .unwrap_or(1);
    doc.sections.insert(pos, Section::new(header));
    log.add(format!("Added missing ext_resource for {} as id \"{}\".", suffix, id));
    Some(id)
}

/// Collect ext ids referenced as a slot's `itemData` or inside its `nested`.
fn referenced_item_ext_ids(doc: &Document, sd_id: Option<&str>) -> HashSet<String> {
    let mut used = HashSet::new();
    for sec in doc.by_kind("sub_resource") {
        if !is_slotdata(sec, sd_id) {
            continue;
        }
        if let Some(Value::ExtResource(id)) = sec.value("itemData") {
            used.insert(id.clone());
        }
        if let Some(Value::Array { items, .. }) = sec.value("nested") {
            for it in items {
                if let Value::ExtResource(id) = it {
                    used.insert(id.clone());
                }
            }
        }
    }
    used
}
