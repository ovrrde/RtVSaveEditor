//! High-level editing operations over a [`Document`]: listing the slots in a
//! container, adding/removing/equipping items, and packing the grid. Lives in
//! `core` so the logic is testable and the GUI stays thin.

use crate::catalog::Catalog;
use crate::tres::{Document, Header, Property, Section, Value};
use crate::validate::slotdata_ext_id;

/// Inventory grid geometry (from Interface.tscn: 512px wide / 64px cells).
pub const GRID_W: u32 = 8;
pub const GRID_H: u32 = 12;
pub const CELL: f64 = 64.0;

/// A flattened, display-friendly view of one `SlotData` slot.
#[derive(Debug, Clone)]
pub struct SlotView {
    pub sub_id: String,
    pub item_path: String,
    pub item_name: String,
    pub condition: i64,
    pub amount: i64,
    pub grid_x: i64,
    pub grid_y: i64,
    pub rotated: bool,
    pub slot: String,
    pub nested: Vec<String>,
}

/// List the slots referenced by one of the `CharacterSave` arrays.
pub fn list_slots(doc: &Document, array: &str, catalog: &Catalog) -> Vec<SlotView> {
    let Some(res) = doc.resource() else { return vec![] };
    let ids: Vec<String> = match res.value(array) {
        Some(Value::Array { items, .. }) => items
            .iter()
            .filter_map(|v| match v {
                Value::SubResource(id) => Some(id.clone()),
                _ => None,
            })
            .collect(),
        _ => vec![],
    };

    let mut out = Vec::new();
    for id in ids {
        let Some(sub) = doc.sub_resource(&id) else { continue };
        let item_path = sub
            .value("itemData")
            .and_then(|v| match v {
                Value::ExtResource(eid) => {
                    doc.ext_resource(eid).and_then(|e| e.header.attr_unquoted("path"))
                }
                _ => None,
            })
            .unwrap_or_default();
        let (gx, gy) = match sub.value("gridPosition") {
            Some(Value::Vector2(x, y)) => ((*x / CELL) as i64, (*y / CELL) as i64),
            _ => (0, 0),
        };
        let nested = match sub.value("nested") {
            Some(Value::Array { items, .. }) => items
                .iter()
                .filter_map(|v| match v {
                    Value::ExtResource(eid) => doc
                        .ext_resource(eid)
                        .and_then(|e| e.header.attr_unquoted("path"))
                        .map(|p| catalog.name_of(&p)),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };
        out.push(SlotView {
            sub_id: id,
            item_name: if item_path.is_empty() {
                "(empty)".into()
            } else {
                catalog.name_of(&item_path)
            },
            item_path,
            condition: sub.value("condition").and_then(|v| v.as_i64()).unwrap_or(100),
            amount: sub.value("amount").and_then(|v| v.as_i64()).unwrap_or(0),
            grid_x: gx,
            grid_y: gy,
            rotated: sub.value("gridRotated").and_then(|v| v.as_bool()).unwrap_or(false),
            slot: sub.value("slot").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            nested,
        });
    }
    out
}

/// Set a scalar field on a slot's sub-resource.
pub fn set_slot_field(doc: &mut Document, sub_id: &str, field: &str, value: Value) {
    if let Some(sub) = doc.sub_resource_mut(sub_id) {
        sub.set(field, value);
    }
}

/// Add an item to a container array. For equipment pass `slot = Some("Torso")`,
/// otherwise the item lands in the grid and positions are repacked.
pub fn add_item(
    doc: &mut Document,
    catalog: &Catalog,
    array: &str,
    item_path: &str,
    slot: Option<&str>,
) -> Option<String> {
    if doc.resource().is_none() {
        return None;
    }
    let item_id = doc.add_ext_resource("Resource", item_path);
    let sd_id = match slotdata_ext_id(doc) {
        Some(id) => Some(id),
        None => Some(doc.add_ext_resource("Script", "res://Scripts/SlotData.gd")),
    };
    let existing_it = doc
        .by_kind("ext_resource")
        .find(|s| s.header.attr_unquoted("path").as_deref().map_or(false, |p| p.ends_with("ItemData.gd")))
        .and_then(|s| s.id());
    let it_id = match existing_it {
        Some(id) => Some(id),
        None => Some(doc.add_ext_resource("Script", "res://Scripts/ItemData.gd")),
    };

    let sub_id = unique_sub_id(doc, "Resource_add");
    let info = catalog.get(item_path);
    let amount = info
        .map(|i| if i.stackable || i.default_amount > 0 { i.default_amount } else { 0 })
        .unwrap_or(0);

    let mut sec = Section::new(
        Header::new("sub_resource")
            .with_attr("type", "\"Resource\"")
            .with_attr("id", &format!("\"{}\"", sub_id)),
    );
    if let Some(sd) = &sd_id {
        sec.props.push(Property::new("script", Value::ExtResource(sd.clone())));
    }
    sec.props.push(Property::new("itemData", Value::ExtResource(item_id)));
    sec.props.push(Property::new(
        "nested",
        Value::Array { elem: it_id.clone().map(|id| Box::new(Value::ExtResource(id))), items: vec![] },
    ));
    sec.props.push(Property::new(
        "storage",
        Value::Array { elem: sd_id.clone().map(|id| Box::new(Value::ExtResource(id))), items: vec![] },
    ));
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
    sec.props.push(Property::new("slot", Value::Str(slot.unwrap_or("").to_string())));
    doc.insert_sub_resource(sec);

    if let Some(res) = doc.resource_mut() {
        let mut arr = match res.value(array) {
            Some(v @ Value::Array { .. }) => v.clone(),
            _ => Value::Array { elem: None, items: vec![] },
        };
        if let Value::Array { items, .. } = &mut arr {
            items.push(Value::SubResource(sub_id.clone()));
        }
        res.set(array, arr);
    }

    if slot.is_none() {
        // Place the new item in the first free cell without disturbing the
        // existing layout (full repack is a separate, explicit action).
        place_item(doc, array, catalog, &sub_id);
    }
    Some(sub_id)
}

/// An item's footprint in cells, accounting for its rotation flag.
pub fn effective_size(catalog: &Catalog, path: &str, rotated: bool) -> (u32, u32) {
    let (w, h) = catalog.size_of(path);
    if rotated {
        (h, w)
    } else {
        (w, h)
    }
}

/// Build the occupancy grid for a container, optionally ignoring one slot
/// (used when testing where a dragged/rotated item could go).
pub fn occupancy(
    doc: &Document,
    array: &str,
    catalog: &Catalog,
    exclude: Option<&str>,
) -> Vec<Vec<bool>> {
    let mut occ = vec![vec![false; GRID_H as usize]; GRID_W as usize];
    for s in list_slots(doc, array, catalog) {
        if Some(s.sub_id.as_str()) == exclude {
            continue;
        }
        let (w, h) = effective_size(catalog, &s.item_path, s.rotated);
        for i in s.grid_x..s.grid_x + w as i64 {
            for j in s.grid_y..s.grid_y + h as i64 {
                if i >= 0 && j >= 0 && (i as u32) < GRID_W && (j as u32) < GRID_H {
                    occ[i as usize][j as usize] = true;
                }
            }
        }
    }
    occ
}

/// Does a `w`x`h` footprint fit at `(gx, gy)` against `occ`?
pub fn fits(occ: &[Vec<bool>], gx: i64, gy: i64, w: u32, h: u32) -> bool {
    if gx < 0 || gy < 0 {
        return false;
    }
    if gx as u32 + w > GRID_W || gy as u32 + h > GRID_H {
        return false;
    }
    for i in gx..gx + w as i64 {
        for j in gy..gy + h as i64 {
            if occ[i as usize][j as usize] {
                return false;
            }
        }
    }
    true
}

/// Can the given slot legally sit at grid cell `(gx, gy)`?
pub fn can_place_at(doc: &Document, array: &str, catalog: &Catalog, sub_id: &str, gx: i64, gy: i64) -> bool {
    let slots = list_slots(doc, array, catalog);
    let Some(s) = slots.iter().find(|s| s.sub_id == sub_id) else { return false };
    let (w, h) = effective_size(catalog, &s.item_path, s.rotated);
    let occ = occupancy(doc, array, catalog, Some(sub_id));
    fits(&occ, gx, gy, w, h)
}

/// Move a slot to `(gx, gy)` if it fits; returns whether it moved.
pub fn move_item(doc: &mut Document, array: &str, catalog: &Catalog, sub_id: &str, gx: i64, gy: i64) -> bool {
    if can_place_at(doc, array, catalog, sub_id, gx, gy) {
        set_slot_field(doc, sub_id, "gridPosition", Value::Vector2(gx as f64 * CELL, gy as f64 * CELL));
        true
    } else {
        false
    }
}

fn find_first_fit(occ: &[Vec<bool>], w: u32, h: u32) -> Option<(i64, i64)> {
    if w == 0 || h == 0 || w > GRID_W || h > GRID_H {
        return None;
    }
    for y in 0..=(GRID_H - h) {
        for x in 0..=(GRID_W - w) {
            if fits(occ, x as i64, y as i64, w, h) {
                return Some((x as i64, y as i64));
            }
        }
    }
    None
}

/// Place a slot in the first free cell that fits it. Returns false if full.
pub fn place_item(doc: &mut Document, array: &str, catalog: &Catalog, sub_id: &str) -> bool {
    let slots = list_slots(doc, array, catalog);
    let Some(s) = slots.iter().find(|s| s.sub_id == sub_id) else { return false };
    let (w, h) = effective_size(catalog, &s.item_path, s.rotated);
    let occ = occupancy(doc, array, catalog, Some(sub_id));
    if let Some((gx, gy)) = find_first_fit(&occ, w, h) {
        set_slot_field(doc, sub_id, "gridPosition", Value::Vector2(gx as f64 * CELL, gy as f64 * CELL));
        true
    } else {
        false
    }
}

/// Toggle a slot's rotation, keeping its position if the rotated footprint
/// still fits there, otherwise relocating it. Returns false if it can't fit
/// anywhere rotated (rotation is then refused).
pub fn rotate_item(doc: &mut Document, array: &str, catalog: &Catalog, sub_id: &str) -> bool {
    let slots = list_slots(doc, array, catalog);
    let Some(s) = slots.iter().find(|s| s.sub_id == sub_id) else { return false };
    let new_rot = !s.rotated;
    let (w, h) = effective_size(catalog, &s.item_path, new_rot);
    let occ = occupancy(doc, array, catalog, Some(sub_id));
    if fits(&occ, s.grid_x, s.grid_y, w, h) {
        set_slot_field(doc, sub_id, "gridRotated", Value::Bool(new_rot));
        return true;
    }
    if let Some((gx, gy)) = find_first_fit(&occ, w, h) {
        set_slot_field(doc, sub_id, "gridRotated", Value::Bool(new_rot));
        set_slot_field(doc, sub_id, "gridPosition", Value::Vector2(gx as f64 * CELL, gy as f64 * CELL));
        return true;
    }
    false
}

/// Remove a slot from a container array and delete its sub-resource.
pub fn remove_slot(doc: &mut Document, array: &str, sub_id: &str) {
    if let Some(res) = doc.resource_mut() {
        if let Some(Value::Array { items, .. }) = res.value(array).cloned().as_ref() {
            let kept: Vec<Value> = items
                .iter()
                .filter(|v| !matches!(v, Value::SubResource(id) if id == sub_id))
                .cloned()
                .collect();
            let elem = match res.value(array) {
                Some(Value::Array { elem, .. }) => elem.clone(),
                _ => None,
            };
            res.set(array, Value::Array { elem, items: kept });
        }
    }
    doc.remove_sub_resource(sub_id);
}

/// Re-pack grid positions for every slot in a grid container so none overlap.
pub fn repack(doc: &mut Document, array: &str, catalog: &Catalog) {
    let slots = list_slots(doc, array, catalog);
    let mut sized: Vec<(String, u32, u32)> = slots
        .iter()
        .map(|s| {
            let (w, h) = catalog.size_of(&s.item_path);
            (s.sub_id.clone(), w, h)
        })
        .collect();
    sized.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));

    let mut occ = vec![vec![false; GRID_H as usize]; GRID_W as usize];
    for (id, w, h) in &sized {
        if let Some((gx, gy)) = first_fit(&mut occ, *w, *h) {
            set_slot_field(doc, id, "gridPosition", Value::Vector2(gx as f64 * CELL, gy as f64 * CELL));
        }
    }
}

fn first_fit(occ: &mut [Vec<bool>], w: u32, h: u32) -> Option<(u32, u32)> {
    if w == 0 || h == 0 || w > GRID_W || h > GRID_H {
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

/// The sub_id of the item currently occupying an equipment slot, if any.
pub fn equipment_occupant(doc: &Document, slot: &str) -> Option<String> {
    let res = doc.resource()?;
    if let Some(Value::Array { items, .. }) = res.value("equipment") {
        for v in items {
            if let Value::SubResource(id) = v {
                if let Some(sub) = doc.sub_resource(id) {
                    if sub.value("slot").and_then(|x| x.as_str()) == Some(slot) {
                        return Some(id.clone());
                    }
                }
            }
        }
    }
    None
}

/// Move a slot's array entry from one container to another (keeping the
/// sub-resource intact). Returns false if it wasn't in `from`.
pub fn transfer(doc: &mut Document, from: &str, to: &str, sub_id: &str) -> bool {
    let Some(res) = doc.resource_mut() else { return false };
    let mut present = false;
    if let Some(Value::Array { elem, items }) = res.value(from).cloned() {
        let mut kept = Vec::new();
        for v in items {
            if matches!(&v, Value::SubResource(id) if id == sub_id) {
                present = true;
            } else {
                kept.push(v);
            }
        }
        res.set(from, Value::Array { elem, items: kept });
    }
    if !present {
        return false;
    }
    let (elem, mut items) = match res.value(to).cloned() {
        Some(Value::Array { elem, items }) => (elem, items),
        _ => (None, Vec::new()),
    };
    items.push(Value::SubResource(sub_id.to_string()));
    res.set(to, Value::Array { elem, items });
    true
}

/// Recompute the `CharacterSave` weapon/gadget flags from the equipment slots,
/// so an equipped weapon is actually usable in-game.
pub fn sync_equip_flags(doc: &mut Document) {
    let mut slots: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(res) = doc.resource() {
        if let Some(Value::Array { items, .. }) = res.value("equipment") {
            for v in items {
                if let Value::SubResource(id) = v {
                    if let Some(sub) = doc.sub_resource(id) {
                        if let Some(sl) = sub.value("slot").and_then(|x| x.as_str()) {
                            if !sl.is_empty() {
                                slots.insert(sl.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(res) = doc.resource_mut() {
        res.set("primary", Value::Bool(slots.contains("Primary")));
        res.set("secondary", Value::Bool(slots.contains("Secondary")));
        res.set("knife", Value::Bool(slots.contains("Knife")));
        res.set("grenade1", Value::Bool(slots.contains("Grenade_1")));
        res.set("grenade2", Value::Bool(slots.contains("Grenade_2")));
        res.set("flashlight", Value::Bool(slots.contains("Light")));
        res.set("NVG", Value::Bool(slots.contains("NVG")));
    }
}

/// Equip an item into a slot, displacing any current occupant into the
/// inventory (rather than deleting it) and syncing the usability flags.
/// Returns the new equipment slot's sub_id.
pub fn equip(doc: &mut Document, catalog: &Catalog, slot: &str, item_path: &str) -> Option<String> {
    if let Some(old) = equipment_occupant(doc, slot) {
        if transfer(doc, "equipment", "inventory", &old) {
            set_slot_field(doc, &old, "slot", Value::Str(String::new()));
            set_slot_field(doc, &old, "gridRotated", Value::Bool(false));
            place_item(doc, "inventory", catalog, &old);
        }
    }
    let new_id = add_item(doc, catalog, "equipment", item_path, Some(slot));
    sync_equip_flags(doc);
    new_id
}

/// Unequip an item to the inventory grid (keeps the item, clears its slot).
pub fn unequip(doc: &mut Document, catalog: &Catalog, sub_id: &str) {
    if transfer(doc, "equipment", "inventory", sub_id) {
        set_slot_field(doc, sub_id, "slot", Value::Str(String::new()));
        place_item(doc, "inventory", catalog, sub_id);
        sync_equip_flags(doc);
    }
}

/// Delete an equipment item entirely and resync flags.
pub fn remove_equipment(doc: &mut Document, sub_id: &str) {
    remove_slot(doc, "equipment", sub_id);
    sync_equip_flags(doc);
}

fn unique_sub_id(doc: &Document, prefix: &str) -> String {
    let mut n = 0;
    loop {
        let id = format!("{}_{}", prefix, n);
        if doc.sub_resource(&id).is_none() {
            return id;
        }
        n += 1;
    }
}
