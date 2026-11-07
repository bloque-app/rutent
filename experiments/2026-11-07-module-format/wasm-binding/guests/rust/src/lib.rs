//! The Rust stand-ins of the Wasm binding (`T-008-02`): one module per provider category, each part its own
//! component. A part and a category are chosen by features (see `Cargo.toml`).
//!
//! Every stand-in carries the module-boundary suite's `ModuleSpec` as a program the binding writes into the
//! `PROGRAM` slot of the built binary (`conformance/README.md`, "Module-boundary suite"). The sandboxed part drives
//! it: it runs its own steps through the `denied` interface, hands each outside step to the outside part as an
//! outside request carrying the values obtained so far, and renders the output the program names. The outside part
//! runs the step it is asked for through the platform's proxy, state, egress and cross-module interfaces.
//!
//! The program's lines, after the slot's magic line, up to `end`:
//!
//! - `provider <id>`, `version <v>`, `starts <0|1>`, `serves <operation>`;
//! - `output honest | padded <n> | unparseable | another-kind | another-transaction | unknown-field`;
//! - `secret <sandboxed|outside> <operation> <name>`, `state <sandboxed|outside> <key>`,
//!   `call <operation> <https|wss> <host> <port>`, `module <callee> <kind>`: the suite's steps;
//! - `spin`, `grow <pages>`, `trap`: the binding's own probes of its limits, run by the sandboxed part;
//! - `forge <operation> <secret> <https|wss> <host> <port>`: the binding's hostile probe, an outside step asked for
//!   under the `serves` operation, in which the outside part looks up the secret and calls the endpoint naming
//!   `<operation>` instead;
//! - `burn <iterations>`: the binding's probe of the outside part's fuel, an outside step in which the outside part
//!   counts to `<iterations>` and then obtains `burned`.

#[cfg(all(feature = "sandboxed", feature = "outside"))]
compile_error!("choose one part: `sandboxed` or `outside`");

/// The provider category this stand-in is built for.
#[cfg(feature = "reserve")]
pub const CATEGORY: &str = "reserve";
/// The provider category this stand-in is built for.
#[cfg(feature = "exchange")]
pub const CATEGORY: &str = "exchange";
/// The provider category this stand-in is built for.
#[cfg(feature = "ramp")]
pub const CATEGORY: &str = "ramp";
/// The provider category this stand-in is built for.
#[cfg(feature = "verifier")]
pub const CATEGORY: &str = "verifier";

/// The operation kind of this category's role that the stand-in answers besides the suite's withdrawal.
#[cfg_attr(feature = "outside", allow(dead_code))]
fn category_kind() -> &'static str {
    match CATEGORY {
        "reserve" => "deposit",
        "exchange" => "conversion",
        "ramp" => "transfer",
        _ => "judgement",
    }
}

const SLOT: usize = 8192;
const MAGIC: &[u8] = b"RUTENT-PROGRAM-SLOT-V1\n";

/// The slot the binding writes the program into. It is filled with `#`, never zero, so that it sits in a data
/// segment of exactly this size, and read only through volatile reads, so that the compiler can't fold the
/// placeholder into the code.
#[used]
static PROGRAM: [u8; SLOT] = {
    let mut slot = [b'#'; SLOT];
    let mut i = 0;
    while i < MAGIC.len() {
        slot[i] = MAGIC[i];
        i += 1;
    }
    slot
};

fn program_text() -> Option<String> {
    let base = (&raw const PROGRAM).cast::<u8>();
    let mut bytes = Vec::with_capacity(SLOT);
    for i in 0..SLOT {
        // SAFETY: `i` is within the static's bounds.
        bytes.push(unsafe { core::ptr::read_volatile(base.add(i)) });
    }
    let rest = bytes.strip_prefix(MAGIC)?;
    let text = core::str::from_utf8(rest).ok()?;
    let end = text.find("\nend\n")?;
    Some(text[..end].to_owned())
}

/// One step of the program. Each part reads the fields it uses.
#[allow(dead_code)]
#[derive(Debug, Clone)]
enum Step {
    Secret {
        sandboxed: bool,
        operation: String,
        name: String,
    },
    State {
        sandboxed: bool,
        key: String,
    },
    Call {
        operation: String,
        scheme: String,
        host: String,
        port: u16,
    },
    Module {
        callee: String,
        kind: String,
    },
    Spin,
    Grow(u32),
    Trap,
    Burn(u64),
    Forge {
        claimed: String,
        name: String,
        scheme: String,
        host: String,
        port: u16,
    },
}

impl Step {
    #[cfg_attr(feature = "outside", allow(dead_code))]
    fn is_outside(&self) -> bool {
        match self {
            Self::Secret { sandboxed, .. } | Self::State { sandboxed, .. } => !sandboxed,
            Self::Call { .. } | Self::Module { .. } | Self::Forge { .. } | Self::Burn(_) => true,
            Self::Spin | Self::Grow(_) | Self::Trap => false,
        }
    }
}

#[derive(Debug, Clone)]
struct Program {
    provider: String,
    version: String,
    starts: bool,
    serves: String,
    output: Vec<String>,
    steps: Vec<Step>,
}

fn program() -> Option<Program> {
    let text = program_text()?;
    let mut program = Program {
        provider: String::new(),
        version: String::new(),
        starts: false,
        serves: String::new(),
        output: Vec::new(),
        steps: Vec::new(),
    };
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let part = |w: &str| w == "sandboxed";
        match words.as_slice() {
            ["provider", p] => program.provider = (*p).to_owned(),
            ["version", v] => program.version = (*v).to_owned(),
            ["starts", s] => program.starts = *s == "1",
            ["serves", o] => program.serves = (*o).to_owned(),
            ["output", rest @ ..] => {
                program.output = rest.iter().map(|w| (*w).to_owned()).collect()
            }
            ["secret", p, o, n] => program.steps.push(Step::Secret {
                sandboxed: part(p),
                operation: (*o).to_owned(),
                name: (*n).to_owned(),
            }),
            ["state", p, k] => program.steps.push(Step::State {
                sandboxed: part(p),
                key: (*k).to_owned(),
            }),
            ["call", o, s, h, port] => program.steps.push(Step::Call {
                operation: (*o).to_owned(),
                scheme: (*s).to_owned(),
                host: (*h).to_owned(),
                port: port.parse().ok()?,
            }),
            ["module", c, k] => program.steps.push(Step::Module {
                callee: (*c).to_owned(),
                kind: (*k).to_owned(),
            }),
            ["spin"] => program.steps.push(Step::Spin),
            ["grow", n] => program.steps.push(Step::Grow(n.parse().ok()?)),
            ["trap"] => program.steps.push(Step::Trap),
            ["burn", n] => program.steps.push(Step::Burn(n.parse().ok()?)),
            ["forge", o, n, s, h, port] => program.steps.push(Step::Forge {
                claimed: (*o).to_owned(),
                name: (*n).to_owned(),
                scheme: (*s).to_owned(),
                host: (*h).to_owned(),
                port: port.parse().ok()?,
            }),
            _ => return None,
        }
    }
    Some(program)
}

fn start() -> Result<(), String> {
    match program() {
        None => Err("the program slot holds no program".to_owned()),
        Some(p) if !p.starts => Err("this release does not start".to_owned()),
        Some(_) => Ok(()),
    }
}

#[cfg(feature = "sandboxed")]
mod sandboxed {
    use serde_json::{Value, json};

    use super::{Program, Step, category_kind, program};

    wit_bindgen::generate!({ world: "sandboxed", path: "../../wit" });

    use rutent::binding::{denied, outside_requests};

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            super::start()
        }

        fn handle(operation: String) -> Vec<u8> {
            let Some(program) = program() else {
                return b"no program".to_vec();
            };
            let envelope: Value = serde_json::from_str(&operation).unwrap_or(Value::Null);
            let statement = &envelope["statement"];
            let kind = statement["kind"].as_str().unwrap_or_default();
            if kind == category_kind() && kind != "withdrawal" {
                return refused(statement);
            }
            let obtained = run(&program, statement);
            render(&program, statement, &obtained)
        }
    }

    /// Runs the steps in order: its own through `denied`, each outside one as an outside request.
    fn run(program: &Program, statement: &Value) -> Vec<String> {
        let transaction = statement["transaction"].as_str().unwrap_or_default();
        let mut obtained: Vec<String> = Vec::new();
        for (index, step) in program.steps.iter().enumerate() {
            if step.is_outside() {
                let operation = match step {
                    Step::Secret { operation, .. } | Step::Call { operation, .. } => {
                        operation.as_str()
                    }
                    _ => program.serves.as_str(),
                };
                let request = json!({
                    "transaction": transaction,
                    "operation": operation,
                    "arguments": { "step": index, "carried": obtained },
                });
                let response = outside_requests::request(&request.to_string());
                let response: Value = serde_json::from_str(&response).unwrap_or(Value::Null);
                if response["outcome"]["status"] == "completed"
                    && let Some(values) = response["outcome"]["body"]["obtained"].as_array()
                {
                    obtained.extend(values.iter().filter_map(|v| v.as_str().map(str::to_owned)));
                }
                continue;
            }
            match step {
                Step::Secret { name, .. } => denied::look_up_secret(name),
                Step::State { key, .. } => denied::read_state(key),
                Step::Spin => spin(),
                Step::Grow(pages) => grow(*pages),
                Step::Trap => core::arch::wasm32::unreachable(),
                Step::Call { .. } | Step::Module { .. } | Step::Forge { .. } | Step::Burn(_) => {}
            }
        }
        obtained
    }

    fn spin() {
        let mut n: u64 = 0;
        loop {
            n = core::hint::black_box(n.wrapping_add(1));
        }
    }

    fn grow(pages: u32) {
        if core::arch::wasm32::memory_grow(0, pages as usize) == usize::MAX {
            core::arch::wasm32::unreachable();
        }
    }

    /// The honest withdrawal result, compact, as the suite pins it: its one evidence reference is
    /// `{provider}-{version}`, then `-{value}` for each value obtained.
    fn honest(program: &Program, statement: &Value, obtained: &[String], pad: usize) -> String {
        let mut reference = format!("{}-{}", program.provider, program.version);
        for value in obtained {
            reference.push('-');
            reference.push_str(value);
        }
        reference.push_str(&"x".repeat(pad));
        let amount = statement["amount"].as_str().unwrap_or_default();
        format!(
            "{{\"kind\":{},\"transaction\":{},\"outcome\":{{\"status\":\"succeeded\",\"amount_in\":{},\
             \"amount_out\":{},\"fee\":{{\"asset\":{},\"amount\":\"0\"}},\"evidence\":[{{\"grade\":3,\
             \"reference\":{},\"by\":{}}}]}}}}",
            quote(statement["kind"].as_str().unwrap_or_default()),
            quote(statement["transaction"].as_str().unwrap_or_default()),
            quote(amount),
            quote(amount),
            quote(statement["asset"].as_str().unwrap_or_default()),
            quote(&reference),
            quote(&program.provider),
        )
    }

    fn render(program: &Program, statement: &Value, obtained: &[String]) -> Vec<u8> {
        let words: Vec<&str> = program.output.iter().map(String::as_str).collect();
        let text = match words.as_slice() {
            ["padded", n] => {
                let size: usize = n.parse().unwrap_or_default();
                let length = honest(program, statement, obtained, 0).len();
                honest(program, statement, obtained, size.saturating_sub(length))
            }
            ["unparseable"] => return b"not a result {".to_vec(),
            ["another-kind"] => {
                let mut changed = statement.clone();
                changed["kind"] = Value::from("deposit");
                honest(program, &changed, obtained, 0)
            }
            ["another-transaction"] => {
                let mut changed = statement.clone();
                let transaction = format!(
                    "{}-another",
                    statement["transaction"].as_str().unwrap_or_default()
                );
                changed["transaction"] = Value::from(transaction);
                honest(program, &changed, obtained, 0)
            }
            ["unknown-field"] => {
                let text = honest(program, statement, obtained, 0);
                format!("{},\"extra\":true}}", &text[..text.len() - 1])
            }
            _ => honest(program, statement, obtained, 0),
        };
        text.into_bytes()
    }

    /// The category's own operation, answered with a final refusal: a valid result of its kind.
    fn refused(statement: &Value) -> Vec<u8> {
        let kind = statement["kind"].as_str().unwrap_or_default();
        let outcome = "{\"status\":\"refused\",\"refusal\":{\"class\":\"final\"}}";
        let text = match statement["transaction"].as_str() {
            Some(transaction) if kind != "judgement" => {
                format!(
                    "{{\"kind\":{},\"transaction\":{},\"outcome\":{outcome}}}",
                    quote(kind),
                    quote(transaction)
                )
            }
            _ => format!("{{\"kind\":{},\"outcome\":{outcome}}}", quote(kind)),
        };
        text.into_bytes()
    }

    fn quote(text: &str) -> String {
        serde_json::to_string(text).unwrap_or_default()
    }

    export!(Part);
}

#[cfg(feature = "outside")]
mod outside {
    use serde_json::{Value, json};

    use super::{Step, program};

    wit_bindgen::generate!({ world: "outside", path: "../../wit" });

    use rutent::binding::egress::{self, Scheme};
    use rutent::binding::{modules, proxy, state};

    struct Part;

    impl Guest for Part {
        fn start() -> Result<(), String> {
            super::start()
        }

        fn perform(outside_request: String) -> String {
            let request: Value = serde_json::from_str(&outside_request).unwrap_or(Value::Null);
            let transaction = request["transaction"].as_str().unwrap_or_default();
            let operation = request["operation"].as_str().unwrap_or_default();
            let carried: Vec<String> = request["arguments"]["carried"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let step = request["arguments"]["step"]
                .as_u64()
                .and_then(|i| program().and_then(|p| p.steps.get(i as usize).cloned()));
            let mut obtained: Vec<String> = Vec::new();
            match step {
                Some(Step::Secret { name, .. }) => {
                    if let Ok(value) = proxy::look_up(transaction, operation, &name) {
                        obtained.push(value);
                    }
                }
                Some(Step::State { key, .. }) => obtained.extend(state::read(&key)),
                Some(Step::Call {
                    scheme, host, port, ..
                }) => {
                    let scheme = if scheme == "wss" {
                        Scheme::Wss
                    } else {
                        Scheme::Https
                    };
                    egress::call(operation, scheme, &host, port, &carried.join("\n"));
                }
                Some(Step::Module { callee, kind }) => {
                    modules::call(&callee, &kind);
                }
                Some(Step::Burn(iterations)) => {
                    let mut n: u64 = 0;
                    for _ in 0..iterations {
                        n = core::hint::black_box(n.wrapping_add(1));
                    }
                    obtained.push("burned".to_owned());
                }
                Some(Step::Forge {
                    claimed,
                    name,
                    scheme,
                    host,
                    port,
                }) => {
                    if let Ok(value) = proxy::look_up(transaction, &claimed, &name) {
                        obtained.push(value);
                    }
                    let scheme = if scheme == "wss" {
                        Scheme::Wss
                    } else {
                        Scheme::Https
                    };
                    egress::call(&claimed, scheme, &host, port, &carried.join("\n"));
                }
                _ => {}
            }
            json!({
                "transaction": transaction,
                "operation": operation,
                "outcome": { "status": "completed", "body": { "obtained": obtained } },
            })
            .to_string()
        }
    }

    export!(Part);
}
