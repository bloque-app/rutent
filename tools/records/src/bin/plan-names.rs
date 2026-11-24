//! `plan-names [--strict] [--through-wave N] [--root DIR]`: reports the tests `PLAN.md` names for a row of `M1` to
//! `M3` that are no test function of the workspace.
//!
//! Report mode (the default) prints findings and exits 0. Strict mode exits 1 when there are any. With
//! `--through-wave N` only the rows of waves up to `N` are held to the code.

use rutent_records::{names, tags};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut strict = false;
    let mut through = None;
    let mut root = PathBuf::from(".");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--strict" => strict = true,
            "--through-wave" => match args.next().and_then(|n| n.parse::<usize>().ok()) {
                Some(n) => through = Some(n),
                None => return usage(),
            },
            "--root" => match args.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let run = || -> std::io::Result<(usize, usize, Vec<rutent_records::Finding>)> {
        let plan = std::fs::read_to_string(root.join("PLAN.md"))?;
        let mut tests = BTreeSet::new();
        for path in tags::find_rust_files(&root)? {
            let text = std::fs::read_to_string(&path)?;
            tests.extend(tags::scan_tests("", &text).into_iter().map(|t| t.name));
        }
        let named = names::named_tests(&plan, through);
        let rows = named.iter().map(|n| &n.task).collect::<BTreeSet<_>>().len();
        Ok((
            named.len(),
            rows,
            names::check_names(&plan, &tests, through),
        ))
    };
    match run() {
        Ok((named, rows, findings)) => {
            for finding in &findings {
                println!("{finding}");
            }
            println!(
                "plan-names: {named} named tests in {rows} rows, {} findings ({} mode)",
                findings.len(),
                if strict { "strict" } else { "report" }
            );
            // A plan that names no test at all (its tables changed shape) would pass for want of anything to hold.
            if strict && (!findings.is_empty() || named == 0) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!(
                "plan-names: cannot read the records under {}: {e}",
                root.display()
            );
            ExitCode::from(2)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: plan-names [--strict] [--through-wave N] [--root DIR]");
    ExitCode::from(2)
}
