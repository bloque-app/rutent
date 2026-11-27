//! `T-025-03`'s scan: after a full simulated run, the log, the state machine's journals and snapshot, and where a
//! caller has them the nodes' own files are searched for what no record may carry: a provider's credential, a secret's
//! plaintext, and a module's private state (`THR-79`, `REQ-AUD-6`; `EXP-1`'s `G-26`).
//!
//! # How it fails closed
//!
//! - It looks for **planted** values as well as the world's real ones: [`plant`] gives each provider a credential
//!   and a private state entry that the run holds and no record may carry, and takes the real private keys (each
//!   provider's statement key, the operator's) as secrets too. A scan that found nothing could be a scan that
//!   can't see, so the tests' controls leak each kind through the state machine's boundary once, and the scan must
//!   find it ([`run_leaking`]), and format a secret with its redaction off, which it must find too.
//! - A scan with no value to seek, a value too short to be told from chance ([`MIN_LEN`]), or a surface with nothing
//!   in it is refused: there would be nothing to say it saw.
//! - Each value is sought in the forms a record writes it: as it is, as JSON writes it, in hex, and in base64 (both
//!   alphabets, padded or not, at each of the three alignments it may fall on inside a longer string), as the JSON
//!   array of bytes serde_json writes a byte string as, and with every byte percent-encoded.

use std::collections::BTreeSet;

use rutent_contracts::state_machine::host::SnapshotEntry;
use rutent_contracts::state_machine::{Id, Observed, RecordId, RecordedInput};

use rutent_conformance::sim::{CREDENTIAL, PRIVATE_NOTES_KEY};

use crate::assembly::Core;
use crate::assembly::machine::{Environment, Machine};
use crate::harness::{Given, Run, Scenario, Step, reference};
use crate::kit::{AMOUNT, RealKey};

/// The shortest value a scan seeks: a shorter one could be found by chance.
pub const MIN_LEN: usize = 8;

/// What a value is, which decides which row of the threat it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A provider's credential: what its outside part resolves through the credential proxy.
    Credential,
    /// A secret's plaintext: a private key.
    Secret,
    /// A module's private state.
    ModuleState,
    /// A balance a simulated account held (`REQ-AUD-6`'s "no balance", `T-024-20`).
    Balance,
}

/// A value to seek.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canary {
    /// What it is called in a report: never its value.
    pub name: String,
    /// What it is.
    pub kind: Kind,
    /// The value.
    pub bytes: Vec<u8>,
}

impl Canary {
    /// A text value.
    pub fn text(name: impl Into<String>, kind: Kind, text: &str) -> Self {
        Self {
            name: name.into(),
            kind,
            bytes: text.as_bytes().to_vec(),
        }
    }
}

/// Somewhere to look: a name, and the bytes found there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    /// What it is, for a report.
    pub name: String,
    /// What it holds.
    pub bytes: Vec<u8>,
}

impl Surface {
    /// A surface holding a record as JSON writes it: `written` is `serde_json::to_vec` of it.
    ///
    /// # Panics
    ///
    /// If it couldn't be written as JSON, which no record of the run can't be.
    pub fn of(name: impl Into<String>, written: Result<Vec<u8>, serde_json::Error>) -> Self {
        Self {
            name: name.into(),
            bytes: written.expect("a record is JSON"),
        }
    }
}

/// A value found where it must not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The value's name.
    pub canary: String,
    /// Its kind.
    pub kind: Kind,
    /// Where it was found.
    pub surface: String,
    /// The form it was found in.
    pub form: &'static str,
}

/// What a scan that found nothing looked at: each surface, and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Each surface and its size in bytes.
    pub looked_at: Vec<(String, usize)>,
    /// How many values, in how many forms, it sought.
    pub sought: usize,
}

/// The values to seek, in every form.
#[derive(Debug, Clone)]
pub struct Scan {
    canaries: Vec<Canary>,
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64(bytes: &[u8], alphabet: &[u8; 64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(alphabet[(n >> (18 - 6 * i) & 63) as usize]);
        }
    }
    out
}

/// The base64 text a value gives wherever it sits in a longer string, which depends on how many bytes come before it
/// (`offset` mod 3): only the characters its own bits decide, so the neighbours' bytes can't spoil the match.
fn base64_inside(bytes: &[u8], alphabet: &[u8; 64], offset: usize) -> Vec<u8> {
    let mut padded = vec![0u8; offset];
    padded.extend_from_slice(bytes);
    let text = base64(&padded, alphabet);
    let from = (offset * 8).div_ceil(6);
    let to = (padded.len() * 8) / 6;
    text[from..to].to_vec()
}

/// The bytes a base64url text stands for, or `None` if it isn't one.
pub fn from_base64url(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = URL.iter().position(|a| *a == c)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits & 0xff) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn hex(bytes: &[u8], upper: bool) -> Vec<u8> {
    let digits: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    bytes
        .iter()
        .flat_map(|b| [digits[usize::from(b >> 4)], digits[usize::from(b & 15)]])
        .collect()
}

/// A value's forms, each named.
pub type Forms = Vec<(&'static str, Vec<u8>)>;

/// Every form a record may write `bytes` in, named.
pub fn forms(bytes: &[u8]) -> Forms {
    let mut forms = vec![("as it is", bytes.to_vec())];
    if let Ok(text) = std::str::from_utf8(bytes) {
        let json = serde_json::to_string(text).expect("text is JSON");
        let inner = json.as_bytes()[1..json.len() - 1].to_vec();
        if inner != bytes {
            forms.push(("escaped as JSON", inner));
        }
    }
    forms.push(("as a JSON array of bytes", byte_array(bytes)));
    forms.push(("percent-encoded", percent(bytes)));
    forms.push(("in lower-case hex", hex(bytes, false)));
    forms.push(("in upper-case hex", hex(bytes, true)));
    for (name, alphabet) in [("in base64", STANDARD), ("in base64url", URL)] {
        for offset in 0..3 {
            let text = base64_inside(bytes, alphabet, offset);
            if text.len() >= MIN_LEN {
                forms.push((name, text));
            }
        }
    }
    forms
}

/// How serde_json writes a byte string with no better form: `[215,132,72]`.
fn byte_array(bytes: &[u8]) -> Vec<u8> {
    let list: Vec<String> = bytes.iter().map(u8::to_string).collect();
    format!("[{}]", list.join(",")).into_bytes()
}

/// Every byte as `%XX`, which any URL decoder reads back to the value whatever the letter; the unreserved ones are
/// written plain in the text a URL carries, so this is the form that hides a value from a plain search.
fn percent(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .flat_map(|b| format!("%{b:02X}").into_bytes())
        .collect()
}

fn found_in(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.len() >= needle.len()
        && haystack.windows(needle.len()).any(|w| w == needle)
}

impl Scan {
    /// A scan for `canaries`.
    ///
    /// # Errors
    ///
    /// If there is none, one is shorter than [`MIN_LEN`], or two share a name: a scan like that would say nothing.
    pub fn new(canaries: Vec<Canary>) -> Result<Self, String> {
        if canaries.is_empty() {
            return Err("a scan with nothing to seek".to_owned());
        }
        let mut names = BTreeSet::new();
        for c in &canaries {
            if c.bytes.len() < MIN_LEN {
                return Err(format!(
                    "`{}` is {} bytes: under {MIN_LEN}, it could be found by chance",
                    c.name,
                    c.bytes.len()
                ));
            }
            if !names.insert(c.name.clone()) {
                return Err(format!("two values are named `{}`", c.name));
            }
        }
        Ok(Self { canaries })
    }

    /// The kinds the scan seeks.
    pub fn kinds(&self) -> BTreeSet<Kind> {
        self.canaries.iter().map(|c| c.kind).collect()
    }

    /// The values' names.
    pub fn names(&self) -> Vec<&str> {
        self.canaries.iter().map(|c| c.name.as_str()).collect()
    }

    /// Every place a value is found, in every form.
    pub fn find(&self, surfaces: &[Surface]) -> Vec<Hit> {
        let mut hits = Vec::new();
        for canary in &self.canaries {
            let forms = forms(&canary.bytes);
            for surface in surfaces {
                if let Some((form, _)) = forms.iter().find(|(_, f)| found_in(&surface.bytes, f)) {
                    hits.push(Hit {
                        canary: canary.name.clone(),
                        kind: canary.kind,
                        surface: surface.name.clone(),
                        form,
                    });
                }
            }
        }
        hits
    }

    /// Looks over `surfaces`: what it looked at if it found nothing.
    ///
    /// # Errors
    ///
    /// The hits, or, if it was given no surface or one with nothing in it, a message saying so: nothing was seen.
    pub fn check(&self, surfaces: &[Surface]) -> Result<Report, Vec<String>> {
        if surfaces.is_empty() {
            return Err(vec!["no surface was given, so nothing was seen".to_owned()]);
        }
        let empty: Vec<String> = surfaces
            .iter()
            .filter(|s| s.bytes.is_empty())
            .map(|s| format!("surface `{}` is empty, so nothing was seen there", s.name))
            .collect();
        if !empty.is_empty() {
            return Err(empty);
        }
        let hits = self.find(surfaces);
        if !hits.is_empty() {
            return Err(hits
                .iter()
                .map(|h| {
                    format!(
                        "{:?} `{}` found in `{}`, {}",
                        h.kind, h.canary, h.surface, h.form
                    )
                })
                .collect());
        }
        Ok(Report {
            looked_at: surfaces
                .iter()
                .map(|s| (s.name.clone(), s.bytes.len()))
                .collect(),
            sought: self.canaries.iter().map(|c| forms(&c.bytes).len()).sum(),
        })
    }
}

/// What a scan of a directory looked at, and found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirScan {
    /// How many files it read.
    pub files: usize,
    /// How many bytes it read.
    pub bytes: u64,
    /// Every place a value was found, a file each.
    pub hits: Vec<Hit>,
}

impl Scan {
    /// Looks through everything `reader` gives, in chunks, as one surface named `name`: a value that straddles two
    /// chunks is found too. Returns what it found and how many bytes it read.
    ///
    /// # Errors
    ///
    /// If the reader fails: a file that couldn't be read is not a file with nothing in it.
    pub fn find_in_reader(
        &self,
        name: &str,
        mut reader: impl std::io::Read,
    ) -> std::io::Result<(Vec<Hit>, u64)> {
        const CHUNK: usize = 1 << 20;
        let forms: Vec<(&Canary, Forms)> =
            self.canaries.iter().map(|c| (c, forms(&c.bytes))).collect();
        let overlap = forms
            .iter()
            .flat_map(|(_, f)| f.iter().map(|(_, b)| b.len()))
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        let mut found: Vec<Hit> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut buffer: Vec<u8> = Vec::new();
        let mut total = 0u64;
        let mut chunk = vec![0u8; CHUNK];
        loop {
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            buffer.extend_from_slice(&chunk[..n]);
            for (canary, forms) in &forms {
                if seen.contains(&canary.name) {
                    continue;
                }
                if let Some((form, _)) = forms.iter().find(|(_, f)| found_in(&buffer, f)) {
                    seen.insert(canary.name.clone());
                    found.push(Hit {
                        canary: canary.name.clone(),
                        kind: canary.kind,
                        surface: name.to_owned(),
                        form,
                    });
                }
            }
            if buffer.len() > overlap {
                buffer.drain(..buffer.len() - overlap);
            }
        }
        Ok((found, total))
    }

    /// Looks through every regular file under `dir`, as the nodes of a cluster keep their state in files: each is a
    /// surface, named by its path under `dir`. Links, sockets and the like are not followed.
    ///
    /// # Errors
    ///
    /// If a directory or a file can't be read.
    pub fn find_in_dir(&self, dir: &std::path::Path) -> std::io::Result<DirScan> {
        let mut scan = DirScan::default();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(at) = pending.pop() {
            for entry in std::fs::read_dir(&at)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    pending.push(entry.path());
                } else if kind.is_file() {
                    let path = entry.path();
                    let name = path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .display()
                        .to_string();
                    let (hits, bytes) = self.find_in_reader(&name, std::fs::File::open(&path)?)?;
                    scan.files += 1;
                    scan.bytes += bytes;
                    scan.hits.extend(hits);
                }
            }
        }
        Ok(scan)
    }
}

/// What a planted run holds that no record may carry.
pub fn plant(core: &mut Core, providers: &[String], seed: u64) -> Vec<Canary> {
    let mut canaries = Vec::new();
    for provider in providers {
        let credential = format!("rutent-canary-credential-{provider}-{seed:016x}");
        let state = format!("rutent-canary-module-state-{provider}-{seed:016x}");
        core.world().hold_secret(provider, CREDENTIAL, &credential);
        core.world().seed_state(provider, PRIVATE_NOTES_KEY, &state);
        canaries.push(Canary::text(
            format!("credential of {provider}"),
            Kind::Credential,
            &credential,
        ));
        canaries.push(Canary::text(
            format!("private state of {provider}"),
            Kind::ModuleState,
            &state,
        ));
    }
    let held = core.world().held_secrets();
    for (provider, key) in held.statement_keys {
        if let Some(bytes) = from_base64url(&key) {
            canaries.push(Canary {
                name: format!("statement key of {provider}"),
                kind: Kind::Secret,
                bytes,
            });
        }
        canaries.push(Canary::text(
            format!("statement key of {provider}, as held"),
            Kind::Secret,
            &key,
        ));
    }
    canaries.push(Canary {
        name: "the operator's signing key".to_owned(),
        kind: Kind::Secret,
        bytes: RealKey::assembly().0.to_bytes().to_vec(),
    });
    canaries
}

/// What a machine holds, as its host answers: each order's state and journal, and the whole snapshot. The snapshot
/// takes the host halted, so it is halted, read and resumed.
///
/// # Errors
///
/// If the host fails: a scan that couldn't read the host says nothing about it.
pub fn machine_surfaces(machine: &mut dyn Machine, of: &str) -> Result<Vec<Surface>, String> {
    let fail = |what: &str, e: &dyn std::fmt::Debug| format!("{of}: {what}: {e:?}");
    let orders = machine.orders().map_err(|e| fail("its orders", &e))?;
    let mut surfaces = vec![Surface::of(
        format!("{of}, its orders"),
        serde_json::to_vec(&orders),
    )];
    for order in &orders {
        let state = machine.state(order).map_err(|e| fail("a state", &e))?;
        let journal = machine.journal(order).map_err(|e| fail("a journal", &e))?;
        surfaces.push(Surface::of(
            format!("{of}, state of {}", order.as_str()),
            serde_json::to_vec(&state),
        ));
        surfaces.push(Surface::of(
            format!("{of}, journal of {}", order.as_str()),
            serde_json::to_vec(&journal),
        ));
    }
    machine.halt().map_err(|e| fail("its halt", &e))?;
    let entries: Result<Vec<SnapshotEntry>, String> =
        machine.export().map_err(|e| fail("its snapshot", &e));
    let resumed = machine.resume().map_err(|e| fail("its resume", &e));
    surfaces.push(Surface::of(
        format!("{of}, its snapshot"),
        serde_json::to_vec(&entries?),
    ));
    resumed?;
    Ok(surfaces)
}

/// What a run left, to scan: Core's log as read back from its disk, the inputs it fed the host, and, from the
/// `after` hook, what its disk and the host held.
pub struct Scanned {
    /// The values the run was planted with and holds.
    pub canaries: Vec<Canary>,
    /// Everything to look in.
    pub surfaces: Vec<Surface>,
    /// The run.
    pub run: Run,
    /// Where each provider's module state sat during the run, as the platform's world held it: provider and key.
    /// It is the platform's `World`, in Core's own process, and never the state machine's host or Core's log
    /// (`CMP-PROV`).
    pub module_state: Vec<(String, String)>,
}

/// The scenario every scan runs: two orders on the base route, run through, with the credentials, the private state
/// and the real keys present throughout.
pub fn full_run() -> Scenario {
    Scenario::new(
        "a full run, scanned",
        crate::checks::BASE,
        Given::one_reserve(crate::assembly::Staged::AtOnce),
    )
    .when(Step::Order {
        request: "req-scan-1".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Order {
        request: "req-scan-2".to_owned(),
        amount: AMOUNT.to_owned(),
    })
    .when(Step::Run)
}

/// Runs [`full_run`] from `seed` in `environment`, planted, and gathers the surfaces: the boundary as the assembly
/// has it.
///
/// # Panics
///
/// If the run doesn't complete, or the host can't be read after it: a scan of a run that didn't happen says nothing.
pub fn run_planted(environment: &mut dyn Environment, seed: u64) -> Scanned {
    run_with(environment, seed, None)
}

/// [`run_planted`] with the boundary open once: before the surfaces are read, a value of `kind` the run holds is
/// let into the state machine's journal, in an input (`leaked_input`). The scan must find it; the control that shows
/// it can see.
///
/// # Panics
///
/// As [`run_planted`], and if the run holds no value of `kind` that is text.
pub fn run_leaking(environment: &mut dyn Environment, seed: u64, kind: Kind) -> Scanned {
    run_with(environment, seed, Some(kind))
}

fn run_with(environment: &mut dyn Environment, seed: u64, leak: Option<Kind>) -> Scanned {
    let scenario = full_run();
    let providers: Vec<String> = scenario
        .given
        .providers
        .iter()
        .map(|s| s.provider.clone())
        .collect();
    let planted: std::cell::RefCell<Vec<Canary>> = std::cell::RefCell::new(Vec::new());
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut module_state: Vec<(String, String)> = Vec::new();
    let run = scenario.play_probed(
        seed,
        environment,
        reference(),
        &mut |core| *planted.borrow_mut() = plant(core, &providers, seed),
        &mut |core| {
            if let Some(kind) = leak {
                let value = planted
                    .borrow()
                    .iter()
                    .filter(|c| c.kind == kind)
                    .find_map(|c| String::from_utf8(c.bytes.clone()).ok())
                    .unwrap_or_else(|| panic!("a planted {kind:?} that is text"));
                let inputs: Vec<RecordedInput> = core
                    .feed
                    .deliveries
                    .iter()
                    .map(|(input, _)| input.clone())
                    .collect();
                core.feed
                    .machine_mut()
                    .submit(leaked_input(&inputs, &value))
                    .unwrap_or_else(|e| panic!("the host refused the leaked input: {e:?}"));
            }
            module_state = core
                .world()
                .held_secrets()
                .state
                .into_iter()
                .map(|(provider, key, _)| (provider, key))
                .collect();
            read_core(core, &mut surfaces);
        },
    );
    completed(&run, leak.is_some());
    read_run(&run, &mut surfaces);
    Scanned {
        canaries: planted.into_inner(),
        surfaces,
        run,
        module_state,
    }
}

/// What Core and its host hold once a run is over: the host's answers, the evidence shelf, and the log's records.
fn read_core(core: &mut Core, surfaces: &mut Vec<Surface>) {
    let machine = core.feed.machine_mut();
    let mut host = machine_surfaces(machine, "the host").unwrap_or_else(|e| panic!("{e}"));
    surfaces.append(&mut host);
    for (n, kept) in core
        .disk
        .shelved()
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .enumerate()
    {
        surfaces.push(Surface {
            name: format!("Core's evidence shelf, entry {n}"),
            bytes: kept,
        });
    }
    for (n, record) in core.disk.records().into_iter().enumerate() {
        // A holding account's difference stated by a raise is a figure of the raise (`T-020-63`): where Core's own
        // segments rest nothing at the wallet's node, which the scenario's landings leave it, what the account holds is
        // what it is over by, and the raise states it. It is the difference the contract carries, never a reading, which
        // the scan seeks, so the raise is left out, and a figure planted in any other record is still found.
        if serde_json::from_slice::<serde_json::Value>(&record)
            .is_ok_and(|value| states_a_holding_difference(&value))
        {
            continue;
        }
        surfaces.push(Surface {
            name: format!("Core's log, record {n}"),
            bytes: record,
        });
    }
    // The decision records Core signs: the log holds only their digests (`T-024-22`), so they are read where Core
    // keeps them, as signed.
    let signed = core.decision_record_bytes();
    // A record that doesn't read back as one is still scanned, as signed, and the run is refused: it would be a
    // record the scan could not tell was there.
    assert_eq!(
        signed.len(),
        core.decision_records().len(),
        "a decision record Core signed doesn't read back as one"
    );
    for (id, bytes) in signed {
        surfaces.push(Surface {
            name: format!("Core's decision record {id}"),
            bytes,
        });
    }
}

/// Whether a log record is a raise of a holding account's difference (`Difference::Holding`): the figure it carries is
/// the difference between the balance and what the orders hold, which the contract states (`T-020-63`).
pub fn states_a_holding_difference(record: &serde_json::Value) -> bool {
    record["what"]["call"] == "raise" && record["what"]["raise"]["difference"]["kind"] == "holding"
}

/// Holds that a run completed, with one more order on the host where an input was let in.
fn completed(run: &Run, leaked: bool) {
    assert!(
        !run.orders.is_empty() && !run.log.is_empty() && run.stopped.is_empty(),
        "the run didn't complete: orders {}, log {}, stopped {:?}",
        run.orders.len(),
        run.log.len(),
        run.stopped
    );
    assert!(
        run.hosted.len() == run.orders.len() + usize::from(leaked),
        "the host holds {} of {} orders",
        run.hosted.len(),
        run.orders.len()
    );
}

/// What the run, once read, adds: the log as read back and the inputs fed to the host.
fn read_run(run: &Run, surfaces: &mut Vec<Surface>) {
    let log = serde_json::to_value(&run.log).expect("the log is JSON");
    let log: Vec<&serde_json::Value> = log
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter(|entry| !states_a_holding_difference(entry))
                .collect()
        })
        .unwrap_or_default();
    surfaces.push(Surface::of("the log read back", serde_json::to_vec(&log)));
    surfaces.push(Surface::of(
        "the inputs fed to the host",
        serde_json::to_vec(&run.inputs),
    ));
    // The text Core produces about the run, which no other surface holds (`T-024-22`): why a route stopped, what
    // Core told each order's customer, why a swap failed and whether the log read back.
    surfaces.push(Surface::of(
        "why the routes stopped",
        serde_json::to_vec(&run.stopped),
    ));
    surfaces.push(Surface::of(
        "what Core told of each order",
        serde_json::to_vec(&run.followed),
    ));
    surfaces.push(Surface::of(
        "why a swap failed",
        serde_json::to_vec(&run.swap_failed),
    ));
    surfaces.push(Surface::of(
        "whether the log read back",
        serde_json::to_vec(&run.reopened),
    ));
}

/// A copy of the first input that opened an order, under an order and a record id that carry `leak`: a boundary
/// that lets a value through into the state machine's journal.
///
/// # Panics
///
/// If `inputs` holds no input that opens an order.
pub fn leaked_input(inputs: &[RecordedInput], leak: &str) -> RecordedInput {
    let mut leaked = inputs
        .iter()
        .find(|i| matches!(i.observed, Observed::RouteChosen { .. }))
        .expect("an input that opens an order")
        .clone();
    leaked.record_id = RecordId::new(format!("record-{leak}")).expect("a record id");
    leaked.subject.order = Id::new(format!("order-{leak}")).expect("an id");
    // And in a field of the payload, which a host that keeps its keys raw and its values another way still holds.
    if let Observed::RouteChosen {
        decision_record, ..
    } = &mut leaked.observed
    {
        *decision_record = Id::new(format!("decision-{leak}")).expect("an id");
    }
    leaked
}

/// The fields of a decision record a control plants a figure in: the request's, the request's round's, the bounds'
/// policy version and the router's version, which are neither the request's nor its record's id (`T-024-22`). Each is a text field that
/// still reads back as a record with the figure in it. A candidate's own fields (its amounts, its providers) are not
/// here: the record holds them to the snapshot and to each other, so a figure written in one is a record that no
/// longer reads back, which `read_core` refuses.
pub const DECISION_FIELDS: [&str; 4] = [
    "/request/request_id",
    "/request/round/refines/request_id",
    "/bounds/policy_version",
    "/router_version",
];

/// Where a balance is planted in a run, for the controls (`T-024-20`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Planted {
    /// In the log: as the customer's request identifier, which Core records.
    InLog(String),
    /// In the state machine's journal: in an input the boundary lets through.
    InJournal(String),
    /// In a decision record Core signed, which the log holds by its digest alone, in the text field `.1` names, a
    /// JSON pointer under the statement (`T-024-22`, [`DECISION_FIELDS`]).
    InDecisionRecord(String, &'static str),
    /// A decision record Core signed that no longer reads back as a record, for the control of the check that every
    /// record kept is read (`T-024-22`).
    CorruptDecisionRecord,
}

/// A run scanned for balances (`T-024-20`).
pub struct Balanced {
    /// What the run left, with the balances as its canaries.
    pub scanned: Scanned,
    /// The node of the holding account.
    pub node: String,
    /// What was landed at the holding account, by order.
    pub landed: Vec<(String, String)>,
    /// The holding account's balance, as its wallet states it.
    pub holding: u128,
    /// Every amount the run states, as text.
    pub stated: Vec<String>,
}

/// The node of the holding account the balance run gives its wallet.
pub const WALLET_NODE: &str = "sim-wallet-node";

/// What the balance run lands at the wallet's account, by order: three figures of eight digits that no amount of the
/// run, no fee and no other balance takes.
pub const LANDED: [(&str, &str); 3] = [
    ("ord-held-1", "31415926"),
    ("ord-held-2", "27182818"),
    ("ord-held-3", "16180339"),
];

/// The two orders of the balance run, in amounts that are no balance's digits.
const ORDERED: [(&str, &str); 2] = [("req-bal-1", "12345678"), ("req-bal-2", "9876543")];

/// The opening balance the balance run gives its reserves under `seed`: between 61 000 000 and 90 000 000, so it
/// covers the orders, and never a round number.
pub fn opening_for(seed: u64) -> u128 {
    let drawn = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 37;
    61_234_567 + u128::from(drawn % 28_000_000)
}

/// The scenario every balance scan runs: [`full_run`]'s shape, with orders of amounts of its own, and, when `planted`
/// is a log plant, a request identifier that carries the figure.
fn balance_run(seed: u64, planted: Option<&Planted>) -> Scenario {
    let mut given = Given::one_reserve(crate::assembly::Staged::AtOnce);
    // The run sets each reserve's opening balance, to a figure of eight digits that is not the stand-in's constant,
    // nobody's amount and no round number, and another for each seed (`T-024-22`).
    for spec in &mut given.providers {
        if spec.role == rutent_conformance::sim::Role::Reserve {
            spec.opening = Some(opening_for(seed));
        }
    }
    let mut scenario = Scenario::new(
        "a full run, scanned for balances",
        crate::checks::BASE,
        given,
    );
    for (n, (request, amount)) in ORDERED.iter().enumerate() {
        let request = match planted {
            Some(Planted::InLog(figure)) if n == 0 => format!("req-{figure}"),
            _ => (*request).to_owned(),
        };
        scenario = scenario.when(Step::Order {
            request,
            amount: (*amount).to_owned(),
        });
    }
    scenario.when(Step::Run)
}

/// The largest amount `reserve`'s bank covers, as the balance checks say it: yes or no to each amount asked, which
/// is all they say, so it is found by halving. This is how the balance is read from the account itself.
///
/// # Panics
///
/// If a check is refused, or the bank covers more than 2^40 or less than one.
fn covered(core: &mut Core, reserve: &str, session: &rutent_contracts::router_api::Id) -> u128 {
    use rutent_prov::catalogue::Verdict;
    let mut covers = |amount: u128| {
        core.check_balance(reserve, session, &amount.to_string())
            .unwrap_or_else(|e| panic!("the balance check of {reserve} was refused: {e:?}"))
            == Verdict::Yes
    };
    let (mut low, mut high) = (1u128, 1u128 << 40);
    assert!(
        covers(low) && !covers(high),
        "{reserve}'s balance is out of range"
    );
    while high - low > 1 {
        let mid = low + (high - low) / 2;
        if covers(mid) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// What each reserve of the scenario's given held when no step had run, as its bank answers: read on a Core with the
/// reference host, since a stand-in's opening balance doesn't depend on the host.
fn opening_balances(scenario: &Scenario, seed: u64) -> Vec<(String, u128)> {
    let bare = Scenario::new(
        "reserves at the start",
        crate::checks::BASE,
        scenario.given.clone(),
    );
    let (mut core, _) = bare.core(seed, &mut crate::assembly::machine::Reference);
    reserves_held(&mut core, &reserves_of(scenario))
}

/// The reserves the scenario is given.
fn reserves_of(scenario: &Scenario) -> Vec<String> {
    scenario
        .given
        .providers
        .iter()
        .filter(|s| s.role == rutent_conformance::sim::Role::Reserve)
        .map(|s| s.provider.clone())
        .collect()
}

fn reserves_held(core: &mut Core, reserves: &[String]) -> Vec<(String, u128)> {
    core.set_check_bound(10_000);
    let session = core
        .open_session(&crate::kit::customer())
        .unwrap_or_else(|e| panic!("a session to ask the banks in: {e:?}"));
    reserves
        .iter()
        .map(|reserve| (reserve.clone(), covered(core, reserve, &session)))
        .collect()
}

/// What a reserve's books held after each entry of `journal` (the world's, as the platform's rail keeps it), replayed
/// into a fresh stand-in of `spec` the way a restored one is: the balance of each account before the first entry and
/// after every one. Nothing is asked of the reserve, so nothing is added to any record the scan reads, and a balance
/// held between two movements of a route, which no step ends on, is read too (`T-024-22`).
fn replayed(
    spec: &rutent_conformance::sim::Spec,
    context: &rutent_conformance::role::SuiteContext,
    journal: &[rutent_conformance::sim::Entry],
) -> Vec<u128> {
    let mut books = rutent_conformance::sim::Simulated::new(spec.clone(), context);
    let mut held = balances_in(&books.state());
    for entry in journal {
        books.answer(&entry.operation, &entry.behaviour);
        held.extend(balances_in(&books.state()));
    }
    held
}

/// One tick of Core's schedule with its holding check due: the wallet's account as the round read and checked it
/// (`REQ-AUD-15`, `T-020-48`). The reading is taken through the platform, as the deployment's timer would have it.
fn tick_holding(core: &mut Core) -> rutent_core_orch::schedule::HoldingChecked {
    core.set_cadence(rutent_core_orch::schedule::Job::HoldingCheck, 0);
    core.tick()
        .holding
        .into_iter()
        .find(|h| h.node == WALLET_NODE)
        .expect("the round read the wallet's account")
}

/// The balance the holding account states now, as one tick read it.
fn holding_balance(core: &mut Core) -> u128 {
    tick_holding(core)
        .reading
        .unwrap_or_else(|e| panic!("the holding account wasn't read: {e:?}"))
        .balance
        .as_str()
        .parse()
        .expect("a figure")
}

/// The balances a stand-in's fingerprint states: the figures of its first map, `{"account": 50000000}`.
fn balances_in(state: &str) -> Vec<u128> {
    let Some(end) = state.find('}') else {
        return Vec::new();
    };
    state[..end]
        .split(", ")
        .filter_map(|pair| pair.rsplit(": ").next()?.trim_matches('}').parse().ok())
        .collect()
}

/// [`run_planted`]'s sibling for `REQ-AUD-6`'s "no balance" half (`T-024-20`, widened by `T-024-22`): a full simulated
/// run in which the simulated accounts hold figures the run states nowhere else, scanned for them.
///
/// The figures are read from the accounts, and not only at the ends of the run: each reserve's bank is asked, by the
/// checks that answer yes or no, what it covers, at the start (on a Core of its own) and once the run is over; each
/// reserve's books are replayed entry by entry from the platform's journal, which gives the balance it held between
/// the movements of a route; and the holding account's wallet states its balance after each landing and after each
/// step. A figure the run states itself (the first landing is one) can't be told from its own mention and is left out
/// ([`Balanced::stated`]). The reserves' opening balance is set by the run ([`opening_for`]), a figure of its own.
/// The wallet is then asked for its balance as a statement, which the evidence store keeps
/// (`holding-balance-evidence`) and the log records by commitment alone, and Core's check of the reading is recorded.
/// Nothing else of the run is told the figures. With `plant`, one more figure is let into the log (which the decision
/// records Core signs carry too, as the route's request) or the journal, for the controls.
///
/// # Panics
///
/// If the run doesn't complete, the accounts can't be read, or a figure the scan seeks is a figure the run states.
pub fn run_balanced(
    environment: &mut dyn Environment,
    seed: u64,
    plant: Option<&Planted>,
) -> Balanced {
    use rutent_contracts::core_internal::About;
    use rutent_contracts::provider_role::Operation;
    let scenario = balance_run(seed, plant);
    let opening = opening_balances(&scenario, seed);
    let reserves = reserves_of(&scenario);
    let specs: Vec<rutent_conformance::sim::Spec> = scenario
        .given
        .providers
        .iter()
        .filter(|s| reserves.contains(&s.provider))
        .cloned()
        .collect();
    let wallet = std::cell::RefCell::new(None);
    let running = std::cell::RefCell::new(Vec::<(String, u128)>::new());
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut closing: Vec<(String, u128)> = Vec::new();
    let mut holding = 0u128;
    // Lands `LANDED[n]` at the wallet, and reads what the account then holds.
    let land = |core: &mut Core, n: usize| {
        let (order, amount) = LANDED[n];
        core.land(WALLET_NODE, "USDC", order, amount)
            .unwrap_or_else(|e| panic!("the wallet didn't take {amount}: {e:?}"));
        let held = holding_balance(core);
        running.borrow_mut().push((
            format!("balance of the holding account, after landing {}", n + 1),
            held,
        ));
    };
    let read_holding = |core: &mut Core| -> u128 { holding_balance(core) };
    // What each reserve's books held after each entry of its journal so far, and what the holding account held.
    let read_running = |core: &mut Core, when: &str| {
        let context = core.fleet().context().clone();
        for spec in &specs {
            let journal = core.world().journal(&spec.provider).to_vec();
            let mut seen = Vec::new();
            for (n, held) in replayed(spec, &context, &journal).into_iter().enumerate() {
                if !seen.contains(&held) {
                    seen.push(held);
                    // Index 0 is the books before any entry: the opening, not a balance held mid-route.
                    let name = if n == 0 {
                        format!(
                            "balance of reserve {}, before any movement ({when})",
                            spec.provider
                        )
                    } else {
                        format!(
                            "balance of reserve {}, after movement {n} ({when})",
                            spec.provider
                        )
                    };
                    running.borrow_mut().push((name, held));
                }
            }
        }
    };
    let run = scenario.play_stepped(
        seed,
        environment,
        reference(),
        &mut |core| {
            let installed = core
                .install_wallet(WALLET_NODE, "USDC")
                .unwrap_or_else(|e| panic!("the wallet wasn't installed: {e:?}"));
            // The first landing is before any step; the others come between them, so the holding account's balance
            // after a step is not one figure for the whole run.
            land(core, 0);
            *wallet.borrow_mut() = Some(installed);
        },
        &mut |core, step| {
            if step < LANDED.len() {
                land(core, step);
            }
            let held = read_holding(core);
            running.borrow_mut().push((
                format!("balance of the holding account, after step {step}"),
                held,
            ));
            read_running(core, &format!("step {step}"));
        },
        &mut |core| {
            let provider = wallet
                .borrow()
                .as_ref()
                .expect("the wallet")
                .provider
                .clone();
            // The reading, as `REQ-AUD-15` takes it, checked, and the wallet's own statement of it kept.
            let round = tick_holding(core);
            let reading = round
                .reading
                .unwrap_or_else(|e| panic!("the holding account wasn't read: {e:?}"));
            holding = reading.balance.as_str().parse().expect("a figure");
            round
                .check
                .unwrap_or_else(|e| panic!("the check wasn't recorded: {e:?}"));
            let question: Operation = serde_json::from_value(serde_json::json!({
                "kind": "holding_balance", "transaction": "tx-holding-balance",
                "provider": provider, "node": WALLET_NODE, "asset": "USDC",
            }))
            .expect("an operation");
            core.ask_wallet_stating(
                &provider,
                question,
                &About {
                    order: Some(crate::kit::id(LANDED[0].0)),
                    segment: None,
                    transaction: Some(crate::kit::id("tx-holding-balance")),
                    node: None,
                },
            )
            .unwrap_or_else(|e| panic!("the wallet's statement wasn't kept: {e:?}"));
            if matches!(plant, Some(Planted::CorruptDecisionRecord)) {
                assert!(
                    core.corrupt_decision_record(),
                    "Core kept no decision record to corrupt"
                );
            }
            if let Some(Planted::InDecisionRecord(figure, at)) = plant {
                assert!(
                    core.leak_into_decision_record(figure, at),
                    "Core kept no decision record with text at {at} to write the figure in"
                );
            }
            if let Some(Planted::InJournal(figure)) = plant {
                let inputs: Vec<RecordedInput> = core
                    .feed
                    .deliveries
                    .iter()
                    .map(|(input, _)| input.clone())
                    .collect();
                core.feed
                    .machine_mut()
                    .submit(leaked_input(&inputs, figure))
                    .unwrap_or_else(|e| panic!("the host refused the leaked input: {e:?}"));
            }
            read_core(core, &mut surfaces);
            // What the platform keeps: each provider's books and what it sent outside.
            for provider in reserves.iter().chain(std::iter::once(&provider)) {
                surfaces.push(Surface::of(
                    format!("the platform's journal of {provider}"),
                    serde_json::to_vec(core.world().journal(provider)),
                ));
            }
            surfaces.push(Surface {
                name: "the outside requests the platform delivered".to_owned(),
                bytes: format!("{:?}", core.world().delivered()).into_bytes(),
            });
            let account = crate::assembly::context().account;
            surfaces.push(Surface {
                name: "the catalogue's log of balance checks".to_owned(),
                bytes: format!("{:?}", core.catalogue().check_log(&account)).into_bytes(),
            });
            // Last, because it asks the banks and so adds to what the catalogue logs.
            closing = reserves_held(core, &reserves);
        },
    );
    completed(&run, matches!(plant, Some(Planted::InJournal(_))));
    read_run(&run, &mut surfaces);
    let landed: Vec<(String, String)> = LANDED
        .iter()
        .map(|(o, a)| ((*o).to_owned(), (*a).to_owned()))
        .collect();
    let mut stated: Vec<String> = ORDERED.iter().map(|(_, a)| (*a).to_owned()).collect();
    stated.extend(LANDED.iter().map(|(_, a)| (*a).to_owned()));
    let mut canaries = vec![Canary::text(
        format!("balance of the holding account at {WALLET_NODE}"),
        Kind::Balance,
        &holding.to_string(),
    )];
    for (when, held) in [("at the start", &opening), ("at the end", &closing)] {
        for (reserve, balance) in held {
            canaries.push(Canary::text(
                format!("balance of reserve {reserve}, {when}"),
                Kind::Balance,
                &balance.to_string(),
            ));
        }
    }
    for (name, held) in running.into_inner() {
        let figure = held.to_string();
        // A figure the run states can't be told from its own mention; one under `MIN_LEN` could be found by chance.
        if !stated.contains(&figure) && figure.len() >= MIN_LEN {
            canaries.push(Canary::text(name, Kind::Balance, &figure));
        }
    }
    Balanced {
        scanned: Scanned {
            canaries,
            surfaces,
            run,
            module_state: Vec::new(),
        },
        node: WALLET_NODE.to_owned(),
        landed,
        holding,
        stated,
    }
}
