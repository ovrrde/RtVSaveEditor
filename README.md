# RtV Save Editor

A desktop editor and corruption repair tool for **Road to Vostok** `Character.tres`
save files, written in Rust.

It was built after a truncated save (cut off mid-write at `casing = fals`, with the
entire `[resource]` block missing) needed manual reconstruction. This tool detects
and repairs that exact class of damage automatically, and lets you edit a save's
stats, inventory, equipment and stash.

## Features

- **Lossless `.tres` parsing** — untouched values round-trip byte-for-byte, so the
  tool never gratuitously rewrites a save.
- **Corruption detection** with precise, line-numbered diagnostics:
  - truncated literals (`fals` → `false`)
  - `SlotData` sub-resources cut off mid-write (missing fields)
  - dangling `ExtResource` / `SubResource` references
  - duplicate ids
  - a missing main `[resource]` block (the headline failure mode)
- **Auto-repair**, including reconstructing a missing `[resource]` block from the
  declared items and whatever slots survived — partitioning recovered slots into
  equipment vs. inventory, recreating lost items, and grid-packing the inventory so
  nothing overlaps the loader's placement check.
- **GUI editor** (egui) — a single, unified **Character** screen with a custom dark
  "Vostok" theme (cards, amber accents), plus a separate **Diagnostics** view:
  - **Left** — Vitals as interactive bar-sliders (drag to set, colour-graded), a
    Conditions chip panel, and an Equipment list grouped by slot type.
  - **Centre** — the in-game-style **8×12 grid**: items drawn at their real
    cell/size, colour-coded by category, with condition bars and stack counts.
    Drag to reposition (live green/red validity), **R** to rotate, click to select.
    A segmented toggle switches the grid between **Inventory** and **Stash**.
  - **Right** — selected-item details (condition/amount/rotate/remove) and an
    always-visible **Add item** search over the game's item catalog. Clicking an
    empty equipment slot fills it with a compatible item.
- **Backups**: every save first copies the existing file to `*.tres.bak`.

## Layout

```
core/    rtv_save_core — dependency-free parser + validator + repair + edit engine
         (all logic lives here and is unit-tested against real save files)
editor/  rtv_save_editor — thin egui GUI over the core
```

## Build & run

```sh
cargo run --release -p rtv_save_editor          # launch the GUI
cargo test -p rtv_save_core                     # run the engine tests
```

Set the **Project** path in the toolbar to your game install (default
`X:/RTVReversed`) and click *Rescan items* so the editor knows item names, grid
sizes and equipment slots. The editor works without it, but only with raw
`res://` paths.

### Headless CLI

```sh
# scan a save and print diagnostics
cargo run -p rtv_save_core --example repair_cli -- scan  Character.tres

# repair a broken save (third arg = game project root, for item sizes)
cargo run -p rtv_save_core --example repair_cli -- repair broken.tres fixed.tres X:/RTVReversed
```

## What repair can and can't recover

The `.tres` declares every item the character owned (the `ext_resource` list), so
**no items are lost** — repair returns them all. What truncation destroys and repair
cannot invent:

- exact **stats** (health/energy/…) — reset to 100,
- the original **inventory ↔ equipment ↔ stash split** and grid layout — items are
  returned to the carried inventory for you to rearrange,
- per-slot details (condition, loaded ammo) for any slot whose data didn't survive —
  set to sensible defaults. Slots that *did* survive keep their real values.

Repair always reports exactly what it changed in the Diagnostics → Repair log.

## Verified

The engine's test suite drives real save files (a good reference save and the
original corrupt one). Repaired output has additionally been confirmed to load in
Godot 4.6 as a valid `CharacterSave` with no broken references.
