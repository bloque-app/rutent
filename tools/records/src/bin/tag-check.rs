//! `tag-check [--strict] [--root DIR]`: reports Rust tests without a valid `// verifies:` comment.
//!
//! Report mode (the default) prints findings and exits 0. Strict mode exits 1 when there are any.

use rutent_records::{ids::load_defined, tags};
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
    let run = || -> std::io::Result<(usize, Vec<rutent_records::Finding>)> {
        let defined = load_defined(&root)?;
        let mut tests = Vec::new();
        for path in tags::find_rust_files(&root)? {
            let text = std::fs::read_to_string(&path)?;
            let shown = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            tests.extend(tags::scan_tests(&shown, &text));
        }
        Ok((tests.len(), tags::check_tests(&tests, &defined)))
    };
    match run() {
        Ok((count, findings)) => {
            for finding in &findings {
                println!("{finding}");
            }
            println!(
                "tag-check: {count} tests, {} findings ({} mode)",
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
                "tag-check: cannot read the records under {}: {e}",
                root.display()
            );
            ExitCode::from(2)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: tag-check [--strict] [--root DIR]");
    ExitCode::from(2)
}
