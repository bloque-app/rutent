//! The checker's tool: `audit-check`, which an appointed checker runs to check the operator's history against the
//! checkpoints it holds (`REQ-AUD-9`, `REQ-AUD-10`, `T-022-04`).
//!
//! It reads three files and takes the rest from its arguments, and reads nothing else (no environment, no
//! network, no key but the log's public one):
//!
//! ```text
//! audit-check --history FILE --checkpoints FILE --openings FILE
//!             --key PUBLIC_KEY --log NAME --first TIME --every SECONDS (--through TICK | --now TIME)
//!             [--from TICK]
//! ```
//!
//! `--history` is the log's file, a record to a line; `--checkpoints` the checkpoints as the checker received each
//! when it was published, a checkpoint to a line, kept by the checker apart from the operator; `--openings` the
//! openings the operator hands over, one to a line. The schedule is `--first`, the time of tick 0, and `--every`,
//! the seconds between ticks. The last tick a checkpoint is owed for is `--through`, or, with `--now`, the last whose
//! period has ended by that time. `--from` is the tick the checker's watch began at, 0 unless given.
//!
//! Exit status: 0 where the history holds against every checkpoint and none is missing, 1 where it found something
//! (each finding on its own line of standard output), 2 where it couldn't run: a file it couldn't read, or an
//! argument that is missing or isn't one.

use std::process::ExitCode;

use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::checkpoint::{Checker, Schedule, read_lines};

/// What the arguments gave.
struct Arguments {
    history: String,
    checkpoints: String,
    openings: String,
    checker: Checker,
    through: Option<u64>,
}

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn arguments(args: &[String]) -> Result<Arguments, String> {
    let need = |flag: &str| value(args, flag).ok_or(format!("{flag} is needed"));
    let time =
        |flag: &str| Timestamp::new(need(flag)?).ok_or(format!("{flag} is an RFC 3339 UTC time"));
    let number = |text: &str, flag: &str| {
        text.parse::<u64>()
            .map_err(|_| format!("{flag} is a whole number"))
    };
    let schedule = Schedule::new(&time("--first")?, number(need("--every")?, "--every")?)
        .map_err(|v| v.0.to_owned())?;
    let key = PublicKey::new(need("--key")?).ok_or("--key is a public key")?;
    let log = Id::new(need("--log")?).ok_or("--log is a name")?;
    let mut checker = Checker::new(key, log, schedule);
    if let Some(from) = value(args, "--from") {
        checker = checker.from(number(from, "--from")?);
    }
    let through = match (value(args, "--through"), value(args, "--now")) {
        (Some(tick), None) => Some(number(tick, "--through")?),
        (None, Some(_)) => schedule.closed_through(time("--now")?.unix_nanos()),
        (None, None) => return Err("--through or --now is needed".to_owned()),
        (Some(_), Some(_)) => {
            return Err("--through and --now are two ways to say one thing".to_owned());
        }
    };
    Ok(Arguments {
        history: need("--history")?.to_owned(),
        checkpoints: need("--checkpoints")?.to_owned(),
        openings: need("--openings")?.to_owned(),
        checker,
        through,
    })
}

fn run(args: &[String]) -> Result<bool, String> {
    let given = arguments(args)?;
    let read = |path: &str| {
        std::fs::read(path)
            .map(|bytes| read_lines(&bytes))
            .map_err(|e| format!("{path} can't be read: {e}"))
    };
    let report = given.checker.check(
        &read(&given.history)?,
        &read(&given.checkpoints)?,
        &read(&given.openings)?,
        given.through,
    );
    for finding in &report.findings {
        println!("{finding}");
    }
    if report.is_clean() {
        println!("the history holds against every checkpoint");
    }
    Ok(report.is_clean())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(why) => {
            eprintln!("audit-check: {why}");
            ExitCode::from(2)
        }
    }
}
