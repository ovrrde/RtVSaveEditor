<!-- Replace OWNER/REPO below with your GitHub path once the repo is created. -->

# RtV Save Editor

A desktop editor and corruption-repair tool for **Road to Vostok** `Character.tres` save files.

[![CI](https://github.com/OWNER/REPO/actions/workflows/ci.yml/badge.svg)](https://github.com/OWNER/REPO/actions/workflows/ci.yml)

<p align="center">
  <img src="media/screenshot.png" alt="RtV Save Editor" width="820">
</p>

## Features

- **Visual inventory** — an in-game-style grid; drag to move items, rotate, edit condition and amounts.
- **Equipment** — equip, swap, and unequip weapons, armor, and gear by slot.
- **Vitals & status** — tweak health, energy, hydration, and condition flags.
- **Add items** — browse and drop in any item from the game's catalog.
- **Auto-repair** — detects and fixes broken/truncated saves (missing data, cut-off writes, dangling references), even rebuilding a save that won't load.
- **Safe** — every save writes a `.tres.bak` backup first.

## Download

Grab the latest `rtv-save-editor.exe` from the [Releases](https://github.com/OWNER/REPO/releases) page. No install needed — just run it.

## Build from source

Requires [Rust](https://rustup.rs/).

```sh
cargo run --release -p rtv_save_editor   # launch the app
cargo test                               # run the test suite
```

## Usage

1. **Open** your `Character.tres` (usually in `%APPDATA%\Roaming\Road to Vostok\`).
2. Set the **project path** to your game install so the editor knows item names, sizes, and slots.
3. Edit, then **Save** (a backup is written automatically).

## How repair works

The save lists every item the character owns, so repair never loses items. When a save is truncated, it reconstructs the missing data — completing cut-off entries and rebuilding the character body from what survived. Anything it can't recover (exact stats, original layout) is reset to sensible defaults, and every change is logged in the Diagnostics tab.

## Project layout

| Crate    | Purpose                                                |
| -------- | ------------------------------------------------------ |
| `core`   | `.tres` parser, validator, and repair engine (no deps) |
| `editor` | the egui desktop GUI                                   |

---

Not affiliated with or endorsed by the developers of Road to Vostok. Back up your saves.
