//! A stand-in's program: the module-boundary suite's `ModuleSpec`, written into a fixed slot of a stand-in's
//! built component (`BoundaryHarness::build`). Each stand-in is compiled once per part, category and language, with
//! the slot filled with `#` after its magic line; building a module for a spec overwrites the slot in place, so the
//! binary keeps its size and structure, and the stand-in's own code reads its steps from its own memory. The line
//! format is the stand-ins' (`module-guests/rust/src/lib.rs`, `module-guests/c/standin.c`); the guests' tool writes the C slot's initializer.

use rutent_conformance::boundary::{ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{OperationKind, PartName, Scheme};

/// The slot's size, in bytes.
pub const SLOT: usize = 8192;

/// The slot's first line, in the template and in every program.
pub const MAGIC: &[u8] = b"RUTENT-PROGRAM-SLOT-V1\n";

/// The binding's own probes, which the suite has no step for, after the spec's steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// Loop forever (`B-03`).
    Spin,
    /// Grow linear memory by this many 64 KiB pages, and trap if refused (`B-04`).
    Grow(u32),
    /// Trap (`B-05`).
    Trap,
    /// Ask the outside part `times` times, through outside requests for the operation it serves, to count to
    /// `iterations` and obtain `burned`: one budget of fuel bounds them all (`B-03`).
    Burn {
        /// What the outside part counts to, each time.
        iterations: u64,
        /// How many outside requests.
        times: usize,
    },
    /// Replace one value of the honest result: the JSON pointer and the JSON value (no spaces in the pointer), the
    /// output tests' way to break a pattern or a length. The Rust stand-ins only.
    Output {
        /// The JSON pointer to the value.
        pointer: String,
        /// The JSON it becomes.
        value: String,
    },
    /// The release's start fails with this error string, which the part itself chose. The Rust stand-ins only.
    StartError(String),
    /// Hostile: asked through an outside request for the operation the module serves first, the outside part
    /// looks up `secret` and calls `endpoint` naming the operation `claimed` instead.
    Forge {
        /// The operation the outside part names to the platform.
        claimed: String,
        /// The secret it looks up.
        secret: String,
        /// Where it calls.
        endpoint: rutent_conformance::boundary::Endpoint,
    },
    /// Hostile or honest, as the test says: the outside part sends a full request (a method, a path with its query
    /// and headers) to `endpoint` under `operation`, and carries what the earlier steps obtained as its body
    /// (`T-013-07`). A header's value holds no space and no comma.
    Fetch {
        /// The outside operation it is made under.
        operation: String,
        /// Where it goes (always `https`).
        endpoint: rutent_conformance::boundary::Endpoint,
        /// `GET`, `POST`, `PUT`, `PATCH` or `DELETE`.
        method: String,
        /// The origin-form target.
        path: String,
        /// Each header as its name, its value and whether the guest flags the value as a credential.
        headers: Vec<(String, String, bool)>,
    },
    /// The outside part looks up the secret `name` for `operation` and obtains the refusal's reason, by the name the
    /// binding gives it, when the lookup is refused (`T-013-07`).
    Reason {
        /// The outside operation it asks under.
        operation: String,
        /// The secret.
        name: String,
    },
    /// The outside part calls `callee`'s module for `kind`, as the suite's `Step::CallModule` does, and obtains
    /// the outcome the binding gives it: `delivered`, `unanswered` or `refused`.
    Ask {
        /// The provider called.
        callee: String,
        /// The kind named.
        kind: OperationKind,
    },
}

fn part(part: PartName) -> &'static str {
    match part {
        PartName::Sandboxed => "sandboxed",
        PartName::Outside => "outside",
    }
}

/// The kind's name as the provider-role contract writes it.
pub fn kind_name(kind: OperationKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The program for `spec`, followed by `probes`, up to its `end` line.
pub fn encode(spec: &ModuleSpec, probes: &[Probe]) -> String {
    let declaration = &spec.declaration;
    let serves = declaration
        .outside
        .operations
        .first()
        .map(|o| o.name.as_str().to_owned())
        .unwrap_or_else(|| "none".to_owned());
    let output = match spec.output {
        Output::Honest => "honest".to_owned(),
        Output::PaddedTo(n) => format!("padded {n}"),
        Output::Unparseable => "unparseable".to_owned(),
        Output::AnotherKind => "another-kind".to_owned(),
        Output::AnotherTransaction => "another-transaction".to_owned(),
        Output::UnknownField => "unknown-field".to_owned(),
    };
    let mut lines = vec![
        format!("provider {}", declaration.provider.as_str()),
        format!("version {}", declaration.version.as_str()),
        format!("starts {}", u8::from(spec.starts)),
        format!("serves {serves}"),
        format!("output {output}"),
    ];
    for step in &spec.steps {
        lines.push(match step {
            Step::LookUpSecret {
                part: p,
                operation,
                name,
            } => format!("secret {} {operation} {name}", part(*p)),
            Step::ReadState { part: p, key } => format!("state {} {key}", part(*p)),
            Step::Call {
                operation,
                endpoint,
            } => {
                let scheme = match endpoint.scheme {
                    Scheme::Https => "https",
                    Scheme::Wss => "wss",
                };
                format!(
                    "call {operation} {scheme} {} {}",
                    endpoint.host, endpoint.port
                )
            }
            Step::CallModule { callee, kind } => format!("module {callee} {}", kind_name(*kind)),
        });
    }
    for probe in probes {
        lines.push(match probe {
            Probe::Spin => "spin".to_owned(),
            Probe::Grow(pages) => format!("grow {pages}"),
            Probe::Trap => "trap".to_owned(),
            Probe::StartError(message) => format!("starts 0 {message}"),
            Probe::Output { pointer, value } => format!("output set {pointer} {value}"),
            Probe::Burn { iterations, times } => {
                vec![format!("burn {iterations}"); *times].join("\n")
            }
            Probe::Fetch {
                operation,
                endpoint,
                method,
                path,
                headers,
            } => {
                let headers = headers
                    .iter()
                    .map(|(name, value, sensitive)| {
                        format!("{}{name}={value}", if *sensitive { "!" } else { "" })
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "fetch {operation} {} {} {method} {path} {}",
                    endpoint.host,
                    endpoint.port,
                    if headers.is_empty() { "-" } else { &headers }
                )
            }
            Probe::Reason { operation, name } => format!("reason {operation} {name}"),
            Probe::Ask { callee, kind } => format!("ask {callee} {}", kind_name(*kind)),
            Probe::Forge {
                claimed,
                secret,
                endpoint,
            } => {
                let scheme = match endpoint.scheme {
                    Scheme::Https => "https",
                    Scheme::Wss => "wss",
                };
                format!(
                    "forge {claimed} {secret} {scheme} {} {}",
                    endpoint.host, endpoint.port
                )
            }
        });
    }
    let mut text = lines.join("\n");
    text.push_str("\nend\n");
    text
}

/// `template` with its slot holding `program`, or `None` if the template has no single slot or the program
/// doesn't fit. The slot is the one place the magic line is followed by `#` to the slot's end; the magic line can
/// appear elsewhere too, where a stand-in's code compares against it.
pub fn patch(template: &[u8], program: &str) -> Option<Vec<u8>> {
    let empty = |at: usize| {
        at + SLOT <= template.len()
            && template[at + MAGIC.len()..at + SLOT]
                .iter()
                .all(|b| *b == b'#')
    };
    let mut found = template
        .windows(MAGIC.len())
        .enumerate()
        .filter(|(i, w)| *w == MAGIC && empty(*i))
        .map(|(i, _)| i);
    let at = found.next()?;
    let body = program.as_bytes();
    if found.next().is_some() || MAGIC.len() + body.len() > SLOT {
        return None;
    }
    let mut out = template.to_vec();
    out[at + MAGIC.len()..at + MAGIC.len() + body.len()].copy_from_slice(body);
    Some(out)
}
