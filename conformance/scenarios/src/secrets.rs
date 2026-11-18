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
                surfaces.push(Surface {
                    name: format!("Core's log, record {n}"),
                    bytes: record,
                });
            }
        },
    );
    assert!(
        !run.orders.is_empty() && !run.log.is_empty() && run.stopped.is_empty(),
        "the run didn't complete: orders {}, log {}, stopped {:?}",
        run.orders.len(),
        run.log.len(),
        run.stopped
    );
    assert!(
        run.hosted.len() == run.orders.len() + usize::from(leak.is_some()),
        "the host holds {} of {} orders",
        run.hosted.len(),
        run.orders.len()
    );
    surfaces.push(Surface::of(
        "the log read back",
        serde_json::to_vec(&run.log),
    ));
    surfaces.push(Surface::of(
        "the inputs fed to the host",
        serde_json::to_vec(&run.inputs),
    ));
    Scanned {
        canaries: planted.into_inner(),
        surfaces,
        run,
        module_state,
    }
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
