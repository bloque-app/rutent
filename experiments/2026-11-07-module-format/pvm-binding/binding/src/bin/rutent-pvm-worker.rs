//! A provider's worker process (ADR-0003: one OS process per provider). It runs the provider's two parts as two
//! PolkaVM instances and carries everything they reach for to the platform, over its standard input and output.
//!
//! - **The sandboxed part** has three host functions: the operation in, the outside request, the result out. Each
//!   outside request goes to the platform, which checks it against the contract's shape, the transaction and the
//!   running release's declaration before the outside part may serve it, and checks the response after.
//! - **The outside part** serves each outside request the platform let through. Its secret lookups, state reads,
//!   outbound calls and cross-module calls go to the platform, which binds each to the request in flight and
//!   writes every record. The worker sends no provider name: the platform knows the provider by the pipe.
//! - **Isolation.** The worker confines itself before it reads anything (`isolation::confine`), and compiles
//!   every blob inside that isolation. A load compiles both parts and compares their digests with the admitted
//!   ones before it instantiates anything.

use std::io::Write;

use polkavm::{Caller, Engine, Instance, Linker};
use rutent_contracts::module_interface::PartName;
use rutent_pvm_binding::isolation::{self, Role};
use rutent_pvm_binding::protocol::{
    FromWorker, HostCall, Reply, ToWorker, hex, receive, send, unhex,
};
use rutent_pvm_binding::{engine, probe};

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
}

struct Outside {
    instance: Instance<OutsideState, String>,
    state: OutsideState,
}

struct SandboxedState {
    outside: Outside,
    input: Vec<u8>,
    output: Option<Vec<u8>>,
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
                match ask(HostCall::Secret { operation, name }) {
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
                match ask(HostCall::Egress {
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

/// Asks the platform to check something, and waits for its status: 0 lets it through.
fn allowed(message: FromWorker) -> bool {
    tell(&message);
    matches!(
        receive::<ToWorker>(&mut std::io::stdin().lock()),
        Ok(Some(ToWorker::Reply(Reply::Status(0))))
    )
}

/// One outside request from the sandboxed part: the platform checks it, the outside part serves it, and the
/// platform checks the response.
fn outside_request(state: &mut SandboxedState, bytes: &[u8]) -> Option<Vec<u8>> {
    let request = String::from_utf8(bytes.to_vec()).ok()?;
    if !allowed(FromWorker::Outside { request }) {
        return None;
    }
    let outside = &mut state.outside;
    outside.state.request = bytes.to_vec();
    outside.state.response = None;
    outside.instance.set_gas(engine::GAS_PER_CALL);
    let served = outside
        .instance
        .call_typed(&mut outside.state, "serve", ())
        .is_ok();
    let response = outside
        .state
        .response
        .take()
        .filter(|_| served)
        .and_then(|r| String::from_utf8(r).ok());
    let passed = allowed(FromWorker::OutsideDone {
        response: response.clone(),
    });
    response.filter(|_| passed).map(String::into_bytes)
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

/// Why a load was refused.
enum Refusal {
    /// A blob or its compilation isn't what admission recorded.
    Digest(String),
    /// Anything else: unreadable, or it didn't start.
    Other(String),
}

impl From<String> for Refusal {
    fn from(reason: String) -> Self {
        Refusal::Other(reason)
    }
}

/// Checks a part's blob is the one admission received, then compiles it, inside this worker's isolation, and checks
/// its compilation's digest is the admitted one.
fn compile(
    engine: &Engine,
    part: PartName,
    bytes: &[u8],
    received: &str,
    expected: &str,
) -> Result<(polkavm::Module, String), Refusal> {
    let blob_digest = rutent_pvm_binding::sha256::hex_digest(bytes);
    if blob_digest != received {
        return Err(Refusal::Digest(format!(
            "the {part:?} part's blob is {blob_digest}, not the received {received}"
        )));
    }
    let blob = engine::check(part, bytes)?;
    let (module, compiled) = engine::compile(engine, bytes, blob)?;
    if compiled.digest != expected {
        return Err(Refusal::Digest(format!(
            "the {part:?} part compiled to {}, not the admitted {expected}",
            compiled.digest
        )));
    }
    Ok((module, compiled.digest))
}

fn instantiate<T>(
    linker: &Linker<T, String>,
    module: &polkavm::Module,
) -> Result<(Instance<T, String>, Option<u32>), String> {
    let pre = linker.instantiate_pre(module).map_err(|e| e.to_string())?;
    let instance = pre.instantiate().map_err(|e| e.to_string())?;
    let pid = instance.pid();
    Ok((instance, pid))
}

fn start<T>(instance: &mut Instance<T, String>, state: &mut T) -> Result<(), String> {
    instance.set_gas(engine::GAS_PER_CALL);
    match instance.call_typed_and_get_result::<u32, ()>(state, "start", ()) {
        Ok(1) => Ok(()),
        Ok(other) => Err(format!("start returned {other}")),
        Err(e) => Err(format!("start failed: {e:?}")),
    }
}

/// Compiles both parts, checks their digests, and only then instantiates and starts them.
fn load(
    engine: &Engine,
    sandboxed: &[u8],
    outside: &[u8],
    expect: [&str; 4],
) -> Result<(Running, FromWorker), Refusal> {
    let [
        received_sandboxed,
        received_outside,
        expect_sandboxed,
        expect_outside,
    ] = expect;
    let (outside_module, outside_digest) = compile(
        engine,
        PartName::Outside,
        outside,
        received_outside,
        expect_outside,
    )?;
    let (sandboxed_module, sandboxed_digest) = compile(
        engine,
        PartName::Sandboxed,
        sandboxed,
        received_sandboxed,
        expect_sandboxed,
    )?;
    let (mut outside_instance, outside_pid) = instantiate(&outside_linker()?, &outside_module)?;
    let (mut sandboxed_instance, sandboxed_pid) =
        instantiate(&sandboxed_linker()?, &sandboxed_module)?;
    let mut outside_state = OutsideState {
        request: Vec::new(),
        response: None,
    };
    start(&mut outside_instance, &mut outside_state)?;
    let mut state = SandboxedState {
        outside: Outside {
            instance: outside_instance,
            state: outside_state,
        },
        input: Vec::new(),
        output: None,
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

fn handle(running: &mut Running, operation: String) -> FromWorker {
    running.state.input = operation.into_bytes();
    running.state.output = None;
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

fn fail(what: &str) -> ! {
    let _ = writeln!(std::io::stderr(), "rutent-pvm-worker: {what}");
    std::process::exit(1);
}

fn main() {
    let started = isolation::started();
    if let Err(e) = isolation::confine(&started, Role::Worker) {
        fail(&e);
    }
    if let Some(code) = probe::run(&started.rest) {
        std::process::exit(code);
    }
    let engine = Engine::new(&engine::config(2)).unwrap_or_else(|e| fail(&e.to_string()));
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
                expect_sandboxed,
                expect_outside,
                received_sandboxed,
                received_outside,
                ..
            } => match (unhex(&sandboxed), unhex(&outside)) {
                (Some(s), Some(o)) => {
                    let expect = [
                        received_sandboxed.as_str(),
                        received_outside.as_str(),
                        expect_sandboxed.as_str(),
                        expect_outside.as_str(),
                    ];
                    match load(&engine, &s, &o, expect) {
                        Ok((next, loaded)) => {
                            running = Some(next);
                            loaded
                        }
                        Err(Refusal::Digest(reason)) => FromWorker::DigestMismatch { reason },
                        Err(Refusal::Other(reason)) => FromWorker::NotStarted { reason },
                    }
                }
                _ => FromWorker::NotStarted {
                    reason: "binaries not in hexadecimal".to_owned(),
                },
            },
            ToWorker::Handle { operation, .. } => match running.as_mut() {
                Some(running) => handle(running, operation),
                None => FromWorker::Failed {
                    reason: "nothing running".to_owned(),
                },
            },
            ToWorker::Reply(_) => return,
        };
        tell(&answer);
    }
}
