//! The checker's tool: `audit-check`, which an appointed checker runs to check the operator's history against the
//! checkpoints it holds (`REQ-AUD-9`, `REQ-AUD-10`, `T-022-04`), and, given the operator's evidence store, each
//! `verified` entry of the history against the statement kept behind it (`T-022-08`).
//!
//! It reads three files, and with `--evidence` a directory, and takes the rest from its arguments, and reads nothing
//! else (no environment, no network, no key it verifies or signs with but the log's public one):
//!
//! ```text
//! audit-check --history FILE --checkpoints FILE --openings FILE
//!             --key PUBLIC_KEY --log NAME --first TIME --every SECONDS (--through TICK | --now TIME)
//!             [--from TICK]
//!             [--evidence DIR --deployment NAME --operator-key PUBLIC_KEY]
//! ```
//!
//! A checkpoint and an opening are the core-internal contract's shapes (`urn:rutent:core-internal:checkpoint` and
//! `:checkpoint-opening`, `T-022-07`), which a party outside rutent's crates can read with the contract's schemas; one
//! in any other shape is no checkpoint, or no opening.
//!
//! `--history` is the log's file, a record to a line; `--checkpoints` the checkpoints as the checker received each
//! when it was published, a checkpoint to a line, kept by the checker apart from the operator; `--openings` the
//! openings the operator hands over, one to a line. The schedule is `--first`, the time of tick 0, and `--every`,
//! the seconds between ticks. The last tick a checkpoint is owed for is `--through`, or, with `--now`, the last whose
//! period has ended by that time. `--from` is the tick the checker's watch began at, 0 unless given.
//!
//! `--evidence` is the operator's evidence store's directory as it is on the disk, read and never written
//! (`statements::KeptOnDisk`). With it the tool runs `unproven` over the history: each `verified` entry a kept
//! statement doesn't bear out is a finding, printed `NotBorneOut { index: N }`. A stated entry whose statement isn't
//! filed is erased only where the history holds a decided erasure of it (the `done` result of a permitted
//! `erase_statement` effect) and the store marks it erased: printed apart as `erased: entry N's ...`, no finding,
//! since history must check after an erasure (`REQ-AUD-13`). Every other case is a finding: a statement neither
//! filed nor marked (`StatementLost { index: N }`), a mark with no decided erasure (`MarkWithoutItsErasure`), and a
//! mark whose bytes are still filed (`MarkedYetFiled`), which are held to the entry all the same. `unproven` holds a statement to the deployment it names, so `--evidence` takes the deployment as
//! statements name it: `--deployment`, its name, and `--operator-key`, the operator's public key, which is part of
//! that name and is only compared, never verified or signed with. What `unproven` can't check is that a key is its
//! maker's: that is the log's register, which the log holds on opening. Without `--evidence`, the output says the
//! evidence was not read, so a clean run isn't taken for more than it checked.
//!
//! Exit status: 0 where the history holds against every checkpoint, none is missing and, with `--evidence`, every
//! `verified` entry whose statement is kept is borne out; 1 where it found something (each finding on its own line of
//! standard output); 2 where it couldn't run: a file or the evidence directory it couldn't read, or an argument that
//! is missing or isn't one.

use std::path::PathBuf;
use std::process::ExitCode;

use rutent_contracts::provider_role::{Deployment, PublicKey};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_core_audit::checkpoint::{Checker, Schedule, read_lines};
use rutent_core_audit::log::read_history;
use rutent_core_audit::statements::{KeptOnDisk, unproven};

/// The evidence the checker was handed, and the deployment its statements name.
struct Given {
    dir: PathBuf,
    deployment: Deployment,
}

/// What the arguments gave.
struct Arguments {
    history: String,
    checkpoints: String,
    openings: String,
    log: Id,
    checker: Checker,
    through: Option<u64>,
    evidence: Option<Given>,
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
    let mut checker = Checker::new(key, log.clone(), schedule);
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
    let evidence = match value(args, "--evidence") {
        Some(dir) => Some(Given {
            dir: PathBuf::from(dir),
            deployment: Deployment {
                name: need("--deployment")?.to_owned(),
                operator_key: PublicKey::new(need("--operator-key")?)
                    .ok_or("--operator-key is a public key")?,
            },
        }),
        None if value(args, "--deployment").is_some()
            || value(args, "--operator-key").is_some() =>
        {
            return Err("--deployment and --operator-key are read only with --evidence".to_owned());
        }
        None => None,
    };
    Ok(Arguments {
        history: need("--history")?.to_owned(),
        checkpoints: need("--checkpoints")?.to_owned(),
        openings: need("--openings")?.to_owned(),
        log,
        checker,
        through,
        evidence,
    })
}

fn run(args: &[String]) -> Result<bool, String> {
    let given = arguments(args)?;
    let read = |path: &str| {
        std::fs::read(path)
            .map(|bytes| read_lines(&bytes))
            .map_err(|e| format!("{path} can't be read: {e}"))
    };
    let records = read(&given.history)?;
    let report = given.checker.check(
        &records,
        &read(&given.checkpoints)?,
        &read(&given.openings)?,
        given.through,
    );
    // The evidence is read whole before anything is printed, so a run that can't read it prints no half answer.
    let evidence = match &given.evidence {
        None => None,
        Some(e) => {
            let mut kept = KeptOnDisk::read(&e.dir)?;
            let history = read_history(&given.log, &records);
            let found = unproven(&history, &mut kept, &e.deployment)
                .map_err(|_| format!("{} can't be read", e.dir.display()))?;
            Some((found, kept.erasures(&history)))
        }
    };
    for finding in &report.findings {
        println!("{finding}");
    }
    if report.is_clean() {
        println!("the history holds against every checkpoint");
    }
    let mut clean = report.is_clean();
    match evidence {
        None => println!(
            "the evidence was not read: no verified entry was held to the statement kept behind it"
        ),
        Some((found, erasures)) => {
            for (receipt, _) in &found.found {
                println!("NotBorneOut {{ index: {} }}", receipt.index.get());
            }
            for receipt in &erasures.lost {
                println!("StatementLost {{ index: {} }}", receipt.index.get());
            }
            for commitment in &erasures.undecided {
                println!(
                    "MarkWithoutItsErasure {{ commitment: {} }}",
                    commitment.as_str()
                );
            }
            for commitment in &erasures.marked_yet_filed {
                println!("MarkedYetFiled {{ commitment: {} }}", commitment.as_str());
            }
            for receipt in &erasures.erased {
                println!(
                    "erased: entry {}'s statement was erased on the log's decided erasure, so it was held to nothing",
                    receipt.index.get()
                );
            }
            if found.found.is_empty() {
                println!("every verified entry whose statement is kept is borne out by it");
            }
            clean &= found.found.is_empty() && erasures.is_clean();
        }
    }
    Ok(clean)
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
