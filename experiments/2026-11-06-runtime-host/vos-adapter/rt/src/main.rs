//! Runs EXP-1's re-score passes and prints each one's report as Markdown.
//!
//! ```text
//! exp1-rt <run-id> <pass> <base>
//! ```
//!
//! The passes are listed in `passes::ALL`; `../run-rt.sh` runs them. The paths come from the environment:
//! `VOS_BIN` (the scored node, built without `faults`), `VOS_FAULTS_BIN` (the faults build), `VOS_AGENT_ELF`,
//! `PROGRAM_ELF`, `PROGRAM_NEXT_ELF` and `PROGRAM_NEXT_FORMAT_ELF`. `EXP1_DEV=1` marks a development run off the
//! scoring platform: the nodes run the interpreter, the platform is not asserted, and nothing it prints is scored.

mod passes;
mod pb;
mod rows;

use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, run_id, pass, base] = &args[..] else {
        eprintln!("usage: exp1-rt <run-id> <pass> <base>");
        std::process::exit(2);
    };
    let base = PathBuf::from(base);
    std::fs::create_dir_all(&base).expect("the base directory");
    let started = Instant::now();
    let env = passes::Env::read(run_id);
    if let Err(e) = env.check_platform() {
        println!("**The platform assertion failed, so this pass did not run:** {e}");
        eprintln!("exp1-rt: {e}");
        std::process::exit(1);
    }
    let ok = passes::run(&env, pass, base);
    println!(
        "\nElapsed: {} s (a measurement, not a target).",
        started.elapsed().as_secs()
    );
    if !ok {
        eprintln!("exp1-rt: pass {pass} did not complete");
        std::process::exit(1);
    }
}
