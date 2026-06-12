//! Integration tests driven by real save files: the good reference save, the
//! truncated/corrupt original, and a hand-built broken sample.

use std::path::PathBuf;

use rtv_save_core::tres::Value;
use rtv_save_core::{catalog, repair_document, tres, validate::validate};

const GOOD: &str = include_str!("data/good.tres");
const CORRUPT: &str = include_str!("data/corrupt.tres");

fn project_root() -> PathBuf {
    PathBuf::from("X:/RTVReversed")
}

#[test]
fn good_save_parses_clean() {
    let (doc, report) = validate(GOOD);
    assert!(report.is_ok(), "good save flagged errors: {:?}", report.diagnostics);
    let res = doc.resource().expect("has [resource]");
    assert_eq!(res.value("health").and_then(|v| v.as_f64()), Some(100.0));
    // inventory + equipment arrays present
    assert!(matches!(res.value("inventory"), Some(Value::Array { .. })));
    assert!(matches!(res.value("equipment"), Some(Value::Array { .. })));
}

#[test]
fn good_save_round_trips_byte_for_byte() {
    let (doc, _) = validate(GOOD);
    let out = doc.to_tres();
    // The parser normalizes line endings to LF by design, so compare against an
    // LF-normalized fixture (CI on Windows may check the file out as CRLF).
    let expected = GOOD.replace("\r\n", "\n");
    assert_eq!(out, expected, "round trip changed the file");
}

#[test]
fn corrupt_save_is_detected() {
    let (_doc, report) = validate(CORRUPT);
    assert!(!report.is_ok(), "corrupt save should have errors");
    // Must catch both the truncated `fals` and the missing [resource] block.
    let msgs: String = report.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n");
    assert!(msgs.contains("truncated `false`"), "should flag truncated false:\n{}", msgs);
    assert!(msgs.contains("missing main [resource]"), "should flag missing resource:\n{}", msgs);
    assert!(report.has_repairable(), "should be marked repairable");
}

#[test]
fn corrupt_save_repairs_into_loadable_document() {
    let cat = catalog::scan(&project_root());
    let (mut doc, _) = validate(CORRUPT);
    let log = repair_document(&mut doc, &cat);
    assert!(!log.actions.is_empty());

    // Re-validate the repaired document by serializing and parsing again.
    let repaired_text = doc.to_tres();
    let (doc2, report2) = validate(&repaired_text);
    assert!(
        report2.is_ok(),
        "repaired save still has errors: {:?}",
        report2.diagnostics
    );

    let res = doc2.resource().expect("repaired save has [resource]");

    // The two recovered weapons must be in equipment with their real conditions.
    let equip = match res.value("equipment") {
        Some(Value::Array { items, .. }) => items.clone(),
        _ => panic!("no equipment array"),
    };
    assert!(!equip.is_empty(), "equipment should contain recovered weapons");

    // Every inventory/equipment slot must reference a defined sub_resource and
    // a defined itemData ext_resource (no dangling refs).
    for arr in ["inventory", "equipment"] {
        if let Some(Value::Array { items, .. }) = res.value(arr) {
            for it in items {
                if let Value::SubResource(id) = it {
                    let sub = doc2.sub_resource(id).expect("slot sub_resource exists");
                    match sub.value("itemData") {
                        Some(Value::ExtResource(eid)) => {
                            assert!(doc2.ext_resource(eid).is_some(), "itemData ext exists");
                        }
                        _ => panic!("slot {} missing itemData", id),
                    }
                }
            }
        }
    }
}

#[test]
fn repaired_grid_positions_do_not_overlap() {
    let cat = catalog::scan(&project_root());
    let (mut doc, _) = validate(CORRUPT);
    repair_document(&mut doc, &cat);

    // Collect inventory slot footprints and assert no two overlap.
    let res = doc.resource().unwrap();
    let inv = match res.value("inventory") {
        Some(Value::Array { items, .. }) => items.clone(),
        _ => vec![],
    };
    let mut rects: Vec<(i64, i64, i64, i64)> = Vec::new();
    for it in &inv {
        let Value::SubResource(id) = it else { continue };
        let sub = doc.sub_resource(id).unwrap();
        let (px, py) = match sub.value("gridPosition") {
            Some(Value::Vector2(x, y)) => (*x as i64 / 64, *y as i64 / 64),
            _ => (0, 0),
        };
        let path = match sub.value("itemData") {
            Some(Value::ExtResource(eid)) => {
                doc.ext_resource(eid).and_then(|e| e.header.attr_unquoted("path")).unwrap_or_default()
            }
            _ => String::new(),
        };
        let (w, h) = cat.size_of(&path);
        rects.push((px, py, w as i64, h as i64));
    }
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            let a = rects[i];
            let b = rects[j];
            let overlap_x = a.0 < b.0 + b.2 && b.0 < a.0 + a.2;
            let overlap_y = a.1 < b.1 + b.3 && b.1 < a.1 + a.3;
            assert!(!(overlap_x && overlap_y), "slots {} and {} overlap: {:?} {:?}", i, j, a, b);
        }
    }
}

#[test]
fn add_then_remove_item_keeps_save_valid() {
    let cat = catalog::scan(&project_root());
    let (mut doc, _) = validate(GOOD);

    let before = rtv_save_core::edit::list_slots(&doc, "inventory", &cat).len();
    let new_id = rtv_save_core::edit::add_item(
        &mut doc,
        &cat,
        "inventory",
        "res://Items/Consumables/Potato/Potato.tres",
        None,
    )
    .expect("added");
    let after = rtv_save_core::edit::list_slots(&doc, "inventory", &cat).len();
    assert_eq!(after, before + 1);

    // Still valid after the edit (serialize + re-validate).
    let (_d2, rep) = validate(&doc.to_tres());
    assert!(rep.is_ok(), "save invalid after add: {:?}", rep.diagnostics);

    rtv_save_core::edit::remove_slot(&mut doc, "inventory", &new_id);
    let final_n = rtv_save_core::edit::list_slots(&doc, "inventory", &cat).len();
    assert_eq!(final_n, before);
    assert!(doc.sub_resource(&new_id).is_none(), "sub_resource removed");
}

#[test]
fn visual_placement_is_non_overlapping_and_bounded() {
    use rtv_save_core::edit;
    let cat = catalog::scan(&project_root());
    let (mut doc, _) = validate(GOOD);

    // Add several items; each should land in a free, in-bounds cell.
    for path in [
        "res://Items/Weapons/Remington_870/Remington_870.tres",
        "res://Items/Weapons/Glock_17/Glock_17.tres",
        "res://Items/Ammo/Ammo_223/Ammo_223.tres",
        "res://Items/Consumables/Potato/Potato.tres",
    ] {
        edit::add_item(&mut doc, &cat, "inventory", path, None);
    }

    // No two inventory items overlap, and all are within the 8x12 grid.
    let occ_check = edit::occupancy(&doc, "inventory", &cat, None);
    let total_cells: usize = occ_check.iter().flatten().filter(|b| **b).count();
    let expected: usize = edit::list_slots(&doc, "inventory", &cat)
        .iter()
        .map(|s| {
            let (w, h) = edit::effective_size(&cat, &s.item_path, s.rotated);
            (w * h) as usize
        })
        .sum();
    assert_eq!(total_cells, expected, "items overlap or spill out of bounds");

    // Move validation: cannot place an item out of bounds.
    let slots = edit::list_slots(&doc, "inventory", &cat);
    let first = &slots[0];
    assert!(!edit::move_item(&mut doc, "inventory", &cat, &first.sub_id, 7, 11)
        || edit::effective_size(&cat, &first.item_path, first.rotated) == (1, 1));

    // Still a valid save after all the moves.
    let (_d, rep) = validate(&doc.to_tres());
    assert!(rep.is_ok(), "invalid after visual edits: {:?}", rep.diagnostics);
}

#[test]
fn equipping_into_occupied_slot_swaps_and_syncs_flags() {
    use rtv_save_core::edit;
    let cat = catalog::scan(&project_root());
    let (mut doc, _) = validate(GOOD);

    // GOOD has a Colt in Secondary. Equip a different pistol into Secondary.
    let before_secondary = edit::equipment_occupant(&doc, "Secondary");
    assert!(before_secondary.is_some(), "fixture should have Secondary occupied");
    let inv_before = edit::list_slots(&doc, "inventory", &cat).len();

    let new_id = edit::equip(&mut doc, &cat, "Secondary", "res://Items/Weapons/Glock_17/Glock_17.tres");
    assert!(new_id.is_some());

    // Exactly one item in Secondary now, and it's the Glock.
    let occ = edit::equipment_occupant(&doc, "Secondary").unwrap();
    let occ_path = edit::list_slots(&doc, "equipment", &cat)
        .into_iter()
        .find(|s| s.sub_id == occ)
        .map(|s| s.item_path)
        .unwrap();
    assert!(occ_path.contains("Glock_17"), "Secondary should now hold the Glock");

    // The displaced Colt moved into inventory (not deleted).
    let inv_after = edit::list_slots(&doc, "inventory", &cat).len();
    assert_eq!(inv_after, inv_before + 1, "displaced item should land in inventory");

    // `secondary` flag stays true; still a valid save.
    let res = doc.resource().unwrap();
    assert_eq!(res.value("secondary").and_then(|v| v.as_bool()), Some(true));
    let (_d, rep) = validate(&doc.to_tres());
    assert!(rep.is_ok(), "invalid after equip swap: {:?}", rep.diagnostics);

    // Unequipping clears the flag and moves the item to inventory.
    edit::unequip(&mut doc, &cat, &occ);
    let res = doc.resource().unwrap();
    assert_eq!(res.value("secondary").and_then(|v| v.as_bool()), Some(false));
    assert!(edit::equipment_occupant(&doc, "Secondary").is_none());
}

#[test]
fn catalog_includes_weapons_with_real_sizes() {
    let cat = catalog::scan(&project_root());
    if cat.items.is_empty() {
        return; // project not present in this environment; skip
    }
    // Weapons use script_class WeaponData (not ItemData) — must still be cataloged.
    let mp5 = cat.get("res://Items/Weapons/MP5/MP5.tres");
    assert!(mp5.is_some(), "MP5 (WeaponData) missing from catalog");
    assert_eq!(mp5.unwrap().size, (4, 2), "MP5 should be 4x2, not defaulted");
    assert_eq!(
        cat.size_of("res://Items/Weapons/Remington_870/Remington_870.tres"),
        (6, 2)
    );

    // `slots` is serialized as a bare array (["Primary","Secondary"]) — must parse.
    let mp5 = cat.get("res://Items/Weapons/MP5/MP5.tres").unwrap();
    assert!(mp5.slots.contains(&"Primary".to_string()), "MP5 slots: {:?}", mp5.slots);
    assert!(mp5.slots.contains(&"Secondary".to_string()));

    // Every standard equipment slot should have at least one compatible item.
    use rtv_save_core::character::EQUIPMENT_SLOTS;
    let mut empty = Vec::new();
    for slot in EQUIPMENT_SLOTS {
        let any = cat.items.iter().any(|i| i.slots.iter().any(|s| s == slot));
        if !any {
            empty.push(*slot);
        }
    }
    assert!(empty.is_empty(), "slots with no catalog items: {:?}", empty);
}

#[test]
fn parses_bare_and_typed_arrays() {
    use rtv_save_core::tres::{parse_value, Value};
    let (v, _) = parse_value(r#"["Primary", "Secondary"]"#);
    match v {
        Value::Array { items, .. } => {
            let s: Vec<_> = items.iter().filter_map(|x| x.as_str()).collect();
            assert_eq!(s, vec!["Primary", "Secondary"]);
        }
        other => panic!("expected array, got {:?}", other),
    }
    // Typed form still works.
    let (v2, _) = parse_value(r#"Array[ExtResource("1")]([SubResource("a")])"#);
    assert!(matches!(v2, Value::Array { .. }));
}

#[test]
fn float_formatting_keeps_decimal_point() {
    assert_eq!(tres::format_float(100.0), "100.0");
    assert_eq!(tres::format_float(98.5), "98.5");
    assert_eq!(tres::format_float(0.0), "0.0");
}
