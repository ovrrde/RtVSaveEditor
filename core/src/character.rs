//! Typed knowledge about `CharacterSave` and `SlotData`, mirrored from
//! `Scripts/CharacterSave.gd` and `Scripts/SlotData.gd`. Used to validate
//! files, supply defaults during repair, and drive the editor UI.

use crate::tres::{Section, Value};

/// `CharacterSave` scalar stat fields (those edited as float sliders).
pub const STAT_FIELDS: &[(&str, f64)] = &[
    ("health", 100.0),
    ("energy", 100.0),
    ("hydration", 100.0),
    ("temperature", 100.0),
    ("mental", 100.0),
    ("cat", 100.0),
    ("bodyStamina", 100.0),
    ("armStamina", 100.0),
];

/// `CharacterSave` boolean status / state flags.
pub const FLAG_FIELDS: &[(&str, bool)] = &[
    ("catFound", false),
    ("catDead", false),
    ("overweight", false),
    ("starvation", false),
    ("dehydration", false),
    ("bleeding", false),
    ("fracture", false),
    ("burn", false),
    ("frostbite", false),
    ("insanity", false),
    ("rupture", false),
    ("headshot", false),
    ("initialSpawn", false),
    ("primary", false),
    ("secondary", false),
    ("knife", false),
    ("grenade1", false),
    ("grenade2", false),
    ("flashlight", false),
    ("NVG", false),
];

/// The three `Array[SlotData]` containers on a `CharacterSave`.
pub const SLOT_ARRAYS: &[&str] = &["inventory", "equipment", "catalog"];

/// Equipment slot names (must match equipment node children in Interface.tscn).
pub const EQUIPMENT_SLOTS: &[&str] = &[
    "Primary", "Secondary", "Knife", "Grenade_1", "Grenade_2", "Backpack", "Rig", "Helmet",
    "Head", "Torso", "Legs", "Belt", "Feet", "Hands", "Matches", "Light", "NVG",
];

/// One `SlotData` field, in serialization order, with a default value factory.
pub struct SlotField {
    pub name: &'static str,
    pub default: fn() -> Value,
}

/// `SlotData` fields in the exact order Godot serializes them. `itemData`,
/// `nested` and `storage` are handled specially during repair because they
/// need the correct ExtResource ids, so their defaults here are placeholders.
pub fn slotdata_fields() -> Vec<SlotField> {
    vec![
        SlotField { name: "condition", default: || Value::Int(100) },
        SlotField { name: "amount", default: || Value::Int(0) },
        SlotField { name: "position", default: || Value::Int(0) },
        SlotField { name: "mode", default: || Value::Int(1) },
        SlotField { name: "zoom", default: || Value::Int(1) },
        SlotField { name: "chamber", default: || Value::Bool(false) },
        SlotField { name: "casing", default: || Value::Bool(false) },
        SlotField { name: "state", default: || Value::Str(String::new()) },
        SlotField { name: "gridPosition", default: || Value::Vector2(0.0, 0.0) },
        SlotField { name: "gridRotated", default: || Value::Bool(false) },
        SlotField { name: "slot", default: || Value::Str(String::new()) },
    ]
}

/// Is this section a `SlotData` sub-resource (its script points at SlotData.gd)?
pub fn is_slotdata(section: &Section, slotdata_ext_id: Option<&str>) -> bool {
    if section.header.kind != "sub_resource" {
        return false;
    }
    match (section.value("script"), slotdata_ext_id) {
        (Some(Value::ExtResource(id)), Some(want)) => id == want,
        // Fall back to a structural guess if we couldn't resolve the script id.
        _ => section.get("itemData").is_some() && section.get("slot").is_some(),
    }
}

/// Human-readable name for an item, derived from its res:// path basename.
pub fn item_name_from_path(res_path: &str) -> String {
    res_path
        .trim_end_matches(".tres")
        .rsplit('/')
        .next()
        .unwrap_or(res_path)
        .replace('_', " ")
}
