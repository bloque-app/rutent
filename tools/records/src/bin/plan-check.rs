//! `plan-check [--strict] [--root DIR]`: checks `PLAN.md`'s tasks and waves.
//!
//! Report mode (the default) prints findings and exits 0. Strict mode exits 1 when there are any.

use rutent_records::{ids::load_defined, plan};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut strict = false;
    let mut root = PathBuf::from(".");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict" => strict = true,
            "--root" => match args.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let run = || -> std::io::Result<Vec<rutent_records::Finding>> {
        let defined = load_defined(&root)?;
        let text = std::fs::read_to_string(root.join("PLAN.md"))?;
        Ok(plan::check_plan(&text, &defined))
    };
    match run() {
        Ok(findings) => {
            for finding in &findings {
                println!("{finding}");
            }
            println!(
                "plan-check: {} findings ({} mode)",
                findings.len(),
                if strict { "strict" } else { "report" }
            );
            if strict && !findings.is_empty() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!(
                "plan-check: cannot read the records under {}: {e}",
                root.display()
            );
            ExitCode::from(2)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: plan-check [--strict] [--root DIR]");
    ExitCode::from(2)
}
