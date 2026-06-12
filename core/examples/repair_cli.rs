//! Headless repair/scan tool.
//!
//!   cargo run -p rtv_save_core --example repair_cli -- scan  <file>
//!   cargo run -p rtv_save_core --example repair_cli -- repair <in> <out> [project_root]

use std::path::Path;
use std::process::ExitCode;

use rtv_save_core::{catalog, load, repair_document, Document};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage:\n  repair_cli scan <file>\n  repair_cli repair <in> <out> [project_root]");
        return ExitCode::from(2);
    }

    match args[1].as_str() {
        "scan" => {
            let (_, report) = match load(Path::new(&args[2])) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("read error: {}", e);
                    return ExitCode::FAILURE;
                }
            };
            for d in &report.diagnostics {
                let line = d.line.map(|l| format!(" (line {})", l)).unwrap_or_default();
                let fixable = if d.repairable { " [auto-repairable]" } else { "" };
                println!("{}{}: {}{}", d.severity.label(), line, d.message, fixable);
            }
            println!(
                "\n{} error(s), {} warning(s).",
                report.errors(),
                report.warnings()
            );
            if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        "repair" => {
            if args.len() < 4 {
                eprintln!("usage: repair_cli repair <in> <out> [project_root]");
                return ExitCode::from(2);
            }
            let text = match std::fs::read_to_string(&args[2]) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("read error: {}", e);
                    return ExitCode::FAILURE;
                }
            };
            let (mut doc, before): (Document, _) = rtv_save_core::validate::validate(&text);
            println!("Before: {} error(s), {} warning(s).", before.errors(), before.warnings());

            let root = args.get(4).map(String::as_str).unwrap_or("X:/RTVReversed");
            let cat = catalog::scan(Path::new(root));
            let log = repair_document(&mut doc, &cat);
            println!("\n--- repair actions ---");
            for a in &log.actions {
                println!("• {}", a);
            }

            let out_text = doc.to_tres();
            let (_, after) = rtv_save_core::validate::validate(&out_text);
            println!(
                "\nAfter: {} error(s), {} warning(s).",
                after.errors(),
                after.warnings()
            );
            if let Err(e) = std::fs::write(&args[3], out_text) {
                eprintln!("write error: {}", e);
                return ExitCode::FAILURE;
            }
            println!("Wrote {}", args[3]);
            if after.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        other => {
            eprintln!("unknown command: {}", other);
            ExitCode::from(2)
        }
    }
}
