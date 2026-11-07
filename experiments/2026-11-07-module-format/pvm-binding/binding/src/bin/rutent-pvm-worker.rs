//! A provider's worker process (ADR-0003: one OS process per provider). It runs the provider's two parts as two
//! PolkaVM instances and carries everything they reach for to the platform, over its standard input and output.
//!
//! - **The sandboxed part** has three host functions: the operation in, the outside request, the result out. An
//!   outside request is checked here against the contract's shape (`OutsideRequest`, which names no secret and no
//!   destination), its transaction and the release's declared operations, before the outside part sees it.
//! - **The outside part** serves each outside request. Its secret lookups, state reads, outbound calls and
//!   cross-module calls go to the platform, which checks each against the running release's declaration and
//!   writes every record. The worker sends no provider name: the platform knows the provider by the pipe.

use std::io::Write;

use polkavm::{Caller, Engine, Instance, Linker};
use rutent_contracts::module_interface::{OutsideRequest, OutsideResponse, PartName};
use rutent_pvm_binding::engine;
use rutent_pvm_binding::protocol::{
    FromWorker, HostCall, Reply, ToWorker, hex, receive, send, unhex,
};

const NONE: u64 = u64::MAX;

fn tell(message: &FromWorker) {
    let mut out = std::io::stdout().lock();
    if send(&mut out, message).is_err() {
        std::process::exit(2);
    }
}

/// Asks the platform one host call and waits for its reply.
fn ask(call: HostCall) -> Reply {
    tell(&FromWorker::Host(call));
    match receive::<ToWorker>(&mut std::io::stdin().lock()) {
        Ok(Some(ToWorker::Reply(reply))) => reply,
        _ => std::process::exit(2),
    }
}

struct OutsideState {
    request: Vec<u8>,
    response: Option<Vec<u8>>,
    transaction: String,
}

struct Outside {
    instance: Instance<OutsideState, String>,
    state: OutsideState,
}

struct SandboxedState {
    outside: Outside,
    input: Vec<u8>,
    output: Option<Vec<u8>>,
    transaction: String,
    operations: Vec<String>,
}

struct Running {
    sandboxed: Instance<SandboxedState, String>,
    state: SandboxedState,
}

fn read(caller: &mut Caller<'_, impl Sized>, ptr: u64, len: u64) -> Result<Vec<u8>, String> {
    let (ptr, len) = (
        u32::try_from(ptr).map_err(|e| e.to_string())?,
        u32::try_from(len).map_err(|e| e.to_string())?,
    );
    caller
        .instance
        .read_memory(ptr, len)
        .map_err(|e| e.to_string())
}

fn read_text(caller: &mut Caller<'_, impl Sized>, ptr: u64, len: u64) -> Result<String, String> {
    String::from_utf8(read(caller, ptr, len)?).map_err(|e| e.to_string())
}

/// Writes `bytes` into the guest's buffer if they fit; their length, or `NONE`.
fn write(
    caller: &mut Caller<'_, impl Sized>,
    ptr: u64,
    cap: u64,
    bytes: &[u8],
) -> Result<u64, String> {
    if bytes.len() as u64 > cap {
        return Ok(NONE);
    }
    let ptr = u32::try_from(ptr).map_err(|e| e.to_string())?;
    caller
        .instance
        .write_memory(ptr, bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes.len() as u64)
}

fn outside_linker() -> Result<Linker<OutsideState, String>, String> {
    let mut linker = Linker::<OutsideState, String>::new();
    let e = |e: polkavm::Error| e.to_string();
    linker
        .define_typed(
            "rt_request",
            |mut c: Caller<OutsideState>, ptr: u64, cap: u64| {
                let request = c.user_data.request.clone();
                write(&mut c, ptr, cap, &request)
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_respond",
            |mut c: Caller<OutsideState>, ptr: u64, len: u64| {
                let response = read(&mut c, ptr, len)?;
                c.user_data.response = Some(response);
                Ok::<(), String>(())
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_secret",
            |mut c: Caller<OutsideState>,
             op: u64,
             op_len: u64,
             name: u64,
             name_len: u64,
             out: u64,
             cap: u64| {
                let operation = read_text(&mut c, op, op_len)?;
                let name = read_text(&mut c, name, name_len)?;
                let transaction = c.user_data.transaction.clone();
                match ask(HostCall::Secret {
                    transaction,
                    operation,
                    name,
                }) {
                    Reply::Value(value) => write(&mut c, out, cap, value.as_bytes()),
                    _ => Ok(NONE),
                }
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_state",
            |mut c: Caller<OutsideState>, key: u64, key_len: u64, out: u64, cap: u64| {
                let key = read_text(&mut c, key, key_len)?;
                match ask(HostCall::State { key }) {
                    Reply::Value(value) => write(&mut c, out, cap, value.as_bytes()),
                    _ => Ok(NONE),
                }
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_egress",
            |mut c: Caller<OutsideState>,
             op: u64,
             op_len: u64,
             url: u64,
             url_len: u64,
             body: u64,
             len: u64| {
                let operation = read_text(&mut c, op, op_len)?;
                let url = read_text(&mut c, url, url_len)?;
                let body = read_text(&mut c, body, len)?;
                let transaction = c.user_data.transaction.clone();
                match ask(HostCall::Egress {
                    transaction,
                    operation,
                    url,
                    body,
                }) {
                    Reply::Status(status) => Ok::<u64, String>(status),
                    _ => Ok(NONE),
                }
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_call",
            |mut c: Caller<OutsideState>,
             callee: u64,
             callee_len: u64,
             kind: u64,
             kind_len: u64| {
                let callee = read_text(&mut c, callee, callee_len)?;
                let kind = read_text(&mut c, kind, kind_len)?;
                match ask(HostCall::Call { callee, kind }) {
                    Reply::Status(status) => Ok::<u64, String>(status),
                    _ => Ok(NONE),
                }
            },
        )
        .map_err(e)?;
    Ok(linker)
}

/// One outside request from the sandboxed part: checked, served by the outside part, its response checked.
fn outside_request(state: &mut SandboxedState, bytes: &[u8]) -> Option<Vec<u8>> {
    // A request naming a secret isn't an outside request; it is refused here, and never reaches the outside part
    // or the proxy. It is reported, so that the refused attempt is seen.
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes)
        && let Some(name) = value.get("secret").and_then(|s| s.as_str())
    {
        let operation = value["operation"].as_str().unwrap_or_default().to_owned();
        tell(&FromWorker::SandboxedSecretRefused {
            operation,
            name: name.to_owned(),
        });
        return None;
    }
    let request: OutsideRequest = serde_json::from_slice(bytes).ok()?;
    if request.transaction.as_str() != state.transaction
        || !state
            .operations
            .iter()
            .any(|o| o == request.operation.as_str())
    {
        return None;
    }
    let outside = &mut state.outside;
    outside.state.request = bytes.to_vec();
    outside.state.response = None;
    outside.state.transaction = state.transaction.clone();
    outside.instance.set_gas(engine::GAS_PER_CALL);
    outside
        .instance
        .call_typed(&mut outside.state, "serve", ())
        .ok()?;
    let response = outside.state.response.take()?;
    let parsed: OutsideResponse = serde_json::from_slice(&response).ok()?;
    (parsed.transaction == request.transaction && parsed.operation == request.operation)
        .then_some(response)
}

fn sandboxed_linker() -> Result<Linker<SandboxedState, String>, String> {
    let mut linker = Linker::<SandboxedState, String>::new();
    let e = |e: polkavm::Error| e.to_string();
    linker
        .define_typed(
            "rt_input",
            |mut c: Caller<SandboxedState>, ptr: u64, cap: u64| {
                let input = c.user_data.input.clone();
                write(&mut c, ptr, cap, &input)
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_outside",
            |mut c: Caller<SandboxedState>, req: u64, req_len: u64, resp: u64, cap: u64| {
                let bytes = read(&mut c, req, req_len)?;
                match outside_request(c.user_data, &bytes) {
                    Some(response) => write(&mut c, resp, cap, &response),
                    None => Ok(NONE),
                }
            },
        )
        .map_err(e)?;
    linker
        .define_typed(
            "rt_output",
            |mut c: Caller<SandboxedState>, ptr: u64, len: u64| {
                let output = read(&mut c, ptr, len)?;
                c.user_data.output = Some(output);
                Ok::<(), String>(())
            },
        )
        .map_err(e)?;
    Ok(linker)
}

/// An instance, its part's digest and its sandbox worker's process, if any.
type Instantiated<T> = (Instance<T, String>, String, Option<u32>);

fn instantiate<T>(
    engine: &Engine,
    linker: &Linker<T, String>,
    part: PartName,
    bytes: &[u8],
) -> Result<Instantiated<T>, String> {
    let blob = engine::check(part, bytes)?;
    let (module, compiled) = engine::compile(engine, bytes, blob)?;
    let pre = linker.instantiate_pre(&module).map_err(|e| e.to_string())?;
    let instance = pre.instantiate().map_err(|e| e.to_string())?;
    let pid = instance.pid();
    Ok((instance, compiled.digest, pid))
}

fn start<T>(instance: &mut Instance<T, String>, state: &mut T) -> Result<(), String> {
    instance.set_gas(engine::GAS_PER_CALL);
    match instance.call_typed_and_get_result::<u32, ()>(state, "start", ()) {
        Ok(1) => Ok(()),
        Ok(other) => Err(format!("start returned {other}")),
        Err(e) => Err(format!("start failed: {e:?}")),
    }
}

fn load(
    engine: &Engine,
    sandboxed: &[u8],
    outside: &[u8],
    operations: Vec<String>,
) -> Result<(Running, FromWorker), String> {
    let (mut outside_instance, outside_digest, outside_pid) =
        instantiate(engine, &outside_linker()?, PartName::Outside, outside)?;
    let (mut sandboxed_instance, sandboxed_digest, sandboxed_pid) =
        instantiate(engine, &sandboxed_linker()?, PartName::Sandboxed, sandboxed)?;
    let mut outside_state = OutsideState {
        request: Vec::new(),
        response: None,
        transaction: String::new(),
    };
    start(&mut outside_instance, &mut outside_state)?;
    let mut state = SandboxedState {
        outside: Outside {
            instance: outside_instance,
            state: outside_state,
        },
        input: Vec::new(),
        output: None,
        transaction: String::new(),
        operations,
    };
    start(&mut sandboxed_instance, &mut state)?;
    let loaded = FromWorker::Loaded {
        sandboxed: sandboxed_digest,
        outside: outside_digest,
        guest_pids: [sandboxed_pid, outside_pid].into_iter().flatten().collect(),
    };
    Ok((
        Running {
            sandboxed: sandboxed_instance,
            state,
        },
        loaded,
    ))
}

fn handle(running: &mut Running, operation: String, transaction: String) -> FromWorker {
    running.state.input = operation.into_bytes();
    running.state.output = None;
    running.state.transaction = transaction;
    running.sandboxed.set_gas(engine::GAS_PER_CALL);
    if let Err(e) = running
        .sandboxed
        .call_typed(&mut running.state, "handle", ())
    {
        return FromWorker::Failed {
            reason: format!("{e:?}"),
        };
    }
    match running.state.output.take() {
        Some(output) => FromWorker::Handled {
            output: hex(&output),
        },
        None => FromWorker::Failed {
            reason: "no output".to_owned(),
        },
    }
}

fn main() {
    let engine = match Engine::new(&engine::config(2)) {
        Ok(engine) => engine,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "rutent-pvm-worker: {e}");
            std::process::exit(1);
        }
    };
    let mut running: Option<Running> = None;
    loop {
        let message = match receive::<ToWorker>(&mut std::io::stdin().lock()) {
            Ok(Some(message)) => message,
            _ => return,
        };
        let answer = match message {
            ToWorker::Load {
                sandboxed,
                outside,
                operations,
                ..
            } => match (unhex(&sandboxed), unhex(&outside)) {
                (Some(s), Some(o)) => match load(&engine, &s, &o, operations) {
                    Ok((next, loaded)) => {
                        running = Some(next);
                        loaded
                    }
                    Err(reason) => FromWorker::NotStarted { reason },
                },
                _ => FromWorker::NotStarted {
                    reason: "binaries not in hexadecimal".to_owned(),
                },
            },
            ToWorker::Handle {
                operation,
                transaction,
            } => match running.as_mut() {
                Some(running) => handle(running, operation, transaction),
                None => FromWorker::Failed {
                    reason: "nothing running".to_owned(),
                },
            },
            ToWorker::Reply(_) => return,
        };
        tell(&answer);
    }
}
