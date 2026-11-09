//! The binding's interface (`wit/binding.wit`) on the host side, as a provider's process serves it: each import a
//! part calls becomes one request to the platform over the process's standard output, answered on its standard
//! input. Nothing in the provider's process holds a secret it wasn't handed, another provider's anything, or a
//! check: the platform makes every check, and knows the provider by the process the request came from (`DEC-9`).

use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::rc::Rc;

use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};

use crate::module::engine::{FUEL_PER_PART, MEMORIES_PER_PART, MEMORY_PER_PART, TABLE_ELEMENTS};
use crate::module::wire::{FromProvider, Reply};

/// The sandboxed part's world, as `bindgen!` generates it.
#[allow(missing_docs)]
pub mod sandboxed {
    wasmtime::component::bindgen!({ world: "sandboxed", path: "src/module/wit" });
}

/// The outside part's world, as `bindgen!` generates it.
#[allow(missing_docs)]
pub mod outside {
    wasmtime::component::bindgen!({ world: "outside", path: "src/module/wit" });
}

/// The imports each part may name, and nothing else (`REQ-MDL-16`, `B-02`).
pub fn allowed_imports(part: &str) -> &'static [&'static str] {
    match part {
        "sandboxed" => &[
            "rutent:binding/outside-requests@0.1.0",
            "rutent:binding/denied@0.1.0",
        ],
        _ => &[
            "rutent:binding/proxy@0.1.0",
            "rutent:binding/state@0.1.0",
            "rutent:binding/egress@0.1.0",
            "rutent:binding/modules@0.1.0",
        ],
    }
}

/// The platform, as a provider's process reaches it: one request, one reply.
pub trait Platform {
    /// Sends `request` and waits for the platform's reply.
    fn ask(&mut self, request: FromProvider) -> Reply;
}

/// The platform over a pair of streams, as a provider's process has it.
pub struct Pipe<R, W> {
    input: R,
    output: W,
}

impl<R: Read, W: Write> Pipe<R, W> {
    /// The platform over these streams.
    pub fn new(input: R, output: W) -> Self {
        Self { input, output }
    }

    /// Sends one message.
    pub fn send(&mut self, message: &FromProvider) -> io::Result<()> {
        let bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
        crate::module::sandbox::write_frame(&mut self.output, &bytes)
    }

    /// Receives one frame.
    pub fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        crate::module::sandbox::read_frame(&mut self.input)
    }
}

impl<R: Read, W: Write> Platform for Pipe<R, W> {
    fn ask(&mut self, request: FromProvider) -> Reply {
        // A platform that is gone ends the process: there is no one left to answer to.
        let answer = self.send(&request).and_then(|()| self.receive());
        match answer {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes).unwrap_or(Reply::Noted),
            _ => std::process::exit(3),
        }
    }
}

/// The platform shared by both parts' stores in one process.
pub type Shared = Rc<RefCell<dyn Platform>>;

/// The outside part's store data.
pub struct OutsideState {
    platform: Shared,
    limits: StoreLimits,
}

/// The sandboxed part's store data: it reaches the outside part only through the platform's check of each request.
pub struct SandboxedState {
    platform: Shared,
    limits: StoreLimits,
    outside: OutsideSlot,
    /// The outside part's fuel left for the operation in hand: one budget across all its outside requests.
    outside_fuel: u64,
}

/// The outside part's instance, inside the sandboxed part's store data so that an outside request can reach it.
pub struct OutsideSlot {
    store: Store<OutsideState>,
    bindings: outside::Outside,
    /// Set when a call into it trapped; the process sets both parts up again after the operation.
    pub poisoned: bool,
}

fn limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(MEMORY_PER_PART)
        .table_elements(TABLE_ELEMENTS)
        .instances(16)
        .tables(4)
        .memories(MEMORIES_PER_PART)
        .trap_on_grow_failure(false)
        .build()
}

impl sandboxed::rutent::binding::outside_requests::Host for SandboxedState {
    fn request(&mut self, outside_request: String) -> String {
        let reply = self
            .platform
            .borrow_mut()
            .ask(FromProvider::OutsideRequest {
                json: outside_request.clone(),
            });
        let response = match reply {
            Reply::Answer { json } => return json,
            Reply::Perform => self.perform(&outside_request),
            _ => return unknown_response(&outside_request),
        };
        match self
            .platform
            .borrow_mut()
            .ask(FromProvider::OutsideResponse { json: response })
        {
            Reply::Answer { json } => json,
            _ => unknown_response(&outside_request),
        }
    }
}

impl SandboxedState {
    /// Calls the outside part on what is left of the operation's outside budget, and keeps what it didn't use.
    fn perform(&mut self, request: &str) -> String {
        let slot = &mut self.outside;
        if slot.poisoned
            || self.outside_fuel == 0
            || slot.store.set_fuel(self.outside_fuel).is_err()
        {
            return unknown_response(request);
        }
        let result = slot.bindings.call_perform(&mut slot.store, request);
        self.outside_fuel = slot.store.get_fuel().unwrap_or(0);
        match result {
            Ok(response) => response,
            Err(_) => {
                slot.poisoned = true;
                unknown_response(request)
            }
        }
    }
}

/// An outside response saying no answer came, for a request whose outside part didn't answer.
fn unknown_response(request: &str) -> String {
    let request: serde_json::Value = serde_json::from_str(request).unwrap_or_default();
    serde_json::json!({
        "transaction": request["transaction"],
        "operation": request["operation"],
        "outcome": { "status": "unknown" },
    })
    .to_string()
}

impl sandboxed::rutent::binding::denied::Host for SandboxedState {
    fn look_up_secret(&mut self, name: String) {
        self.platform
            .borrow_mut()
            .ask(FromProvider::DeniedSecret { name });
    }

    fn read_state(&mut self, key: String) {
        self.platform
            .borrow_mut()
            .ask(FromProvider::DeniedState { key });
    }
}

use outside::rutent::binding::{egress, modules, proxy, state};

impl proxy::Host for OutsideState {
    fn look_up(
        &mut self,
        transaction: String,
        operation: String,
        name: String,
    ) -> Result<String, proxy::SecretRefusal> {
        match self.platform.borrow_mut().ask(FromProvider::Lookup {
            transaction,
            operation,
            name,
        }) {
            Reply::Secret { value } => Ok(value),
            Reply::SecretRefused { reason } if reason == "not_found" => {
                Err(proxy::SecretRefusal::NotFound)
            }
            _ => Err(proxy::SecretRefusal::UndeclaredSecret),
        }
    }
}

impl state::Host for OutsideState {
    fn read(&mut self, key: String) -> Option<String> {
        match self.platform.borrow_mut().ask(FromProvider::State { key }) {
            Reply::State { value } => value,
            _ => None,
        }
    }
}

impl egress::Host for OutsideState {
    fn call(
        &mut self,
        operation: String,
        scheme: egress::Scheme,
        host: String,
        port: u16,
        body: String,
    ) -> egress::Outcome {
        let scheme = match scheme {
            egress::Scheme::Https => "https",
            egress::Scheme::Wss => "wss",
        };
        match self.platform.borrow_mut().ask(FromProvider::Egress {
            operation,
            scheme: scheme.to_owned(),
            host,
            port,
            body,
        }) {
            Reply::Egress { outcome } => match outcome.as_str() {
                "completed" => egress::Outcome::Completed,
                "failed" => egress::Outcome::Failed,
                "unknown" => egress::Outcome::Unknown,
                _ => egress::Outcome::Refused,
            },
            _ => egress::Outcome::Refused,
        }
    }
}

impl modules::Host for OutsideState {
    fn call(&mut self, callee: String, kind: String) -> modules::CallOutcome {
        match self
            .platform
            .borrow_mut()
            .ask(FromProvider::Module { callee, kind })
        {
            Reply::Module { outcome } if outcome == "delivered" => modules::CallOutcome::Delivered,
            _ => modules::CallOutcome::Refused,
        }
    }
}

/// Both parts of one provider's module, compiled, from which the process instantiates them.
pub struct Loaded {
    engine: Engine,
    sandboxed: Component,
    outside: Component,
    sandboxed_linker: Linker<SandboxedState>,
    outside_linker: Linker<OutsideState>,
}

/// The instances of both parts.
pub struct Running {
    store: Store<SandboxedState>,
    bindings: sandboxed::Sandboxed,
}

impl Loaded {
    /// Both parts' components, with linkers that serve their imports through `platform`.
    pub fn new(engine: Engine, sandboxed: Component, outside: Component) -> wasmtime::Result<Self> {
        let mut sandboxed_linker = Linker::new(&engine);
        sandboxed::Sandboxed::add_to_linker::<_, HasSelf<_>>(&mut sandboxed_linker, |s| s)?;
        let mut outside_linker = Linker::new(&engine);
        outside::Outside::add_to_linker::<_, HasSelf<_>>(&mut outside_linker, |s| s)?;
        Ok(Self {
            engine,
            sandboxed,
            outside,
            sandboxed_linker,
            outside_linker,
        })
    }

    /// Instantiates both parts and starts them: the outside part first, then the sandboxed part. An error is a
    /// release that doesn't start.
    pub fn start(&self, platform: Shared) -> Result<Running, String> {
        let mut outside_store = Store::new(
            &self.engine,
            OutsideState {
                platform: platform.clone(),
                limits: limits(),
            },
        );
        outside_store.limiter(|s| &mut s.limits);
        // Instantiating runs code too, so it is metered like any call.
        outside_store
            .set_fuel(FUEL_PER_PART)
            .map_err(|e| e.to_string())?;
        let bindings =
            outside::Outside::instantiate(&mut outside_store, &self.outside, &self.outside_linker)
                .map_err(|e| format!("the outside part didn't instantiate: {e}"))?;
        outside_store
            .set_fuel(FUEL_PER_PART)
            .map_err(|e| e.to_string())?;
        bindings
            .call_start(&mut outside_store)
            .map_err(|e| format!("the outside part trapped starting: {e}"))?
            .map_err(|e| format!("the outside part didn't start: {e}"))?;
        let mut store = Store::new(
            &self.engine,
            SandboxedState {
                platform,
                limits: limits(),
                outside: OutsideSlot {
                    store: outside_store,
                    bindings,
                    poisoned: false,
                },
                outside_fuel: FUEL_PER_PART,
            },
        );
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_PER_PART).map_err(|e| e.to_string())?;
        let bindings =
            sandboxed::Sandboxed::instantiate(&mut store, &self.sandboxed, &self.sandboxed_linker)
                .map_err(|e| format!("the sandboxed part didn't instantiate: {e}"))?;
        store.set_fuel(FUEL_PER_PART).map_err(|e| e.to_string())?;
        bindings
            .call_start(&mut store)
            .map_err(|e| format!("the sandboxed part trapped starting: {e}"))?
            .map_err(|e| format!("the sandboxed part didn't start: {e}"))?;
        Ok(Running { store, bindings })
    }
}

impl Running {
    /// Hands one operation to the sandboxed part: its output, or why it gave none. After an error the instances
    /// can't be used again.
    pub fn handle(&mut self, operation: &str) -> Result<Vec<u8>, String> {
        self.store.data_mut().outside_fuel = FUEL_PER_PART;
        self.store
            .set_fuel(FUEL_PER_PART)
            .map_err(|e| e.to_string())?;
        self.bindings
            .call_handle(&mut self.store, operation)
            .map_err(|e| format!("{e:#}"))
    }

    /// Whether the outside part trapped during the last operation.
    pub fn outside_poisoned(&self) -> bool {
        self.store.data().outside.poisoned
    }
}

/// A platform that answers nothing: the compiler links each part against the binding's interface with it, to
/// check its imports and exports without running it.
pub struct Nowhere;

impl Platform for Nowhere {
    fn ask(&mut self, _: FromProvider) -> Reply {
        Reply::Noted
    }
}

/// Checks a compiled part against the binding's interface: every import is one the part may name, of the type the
/// binding serves, and its exports are the world's. Nothing runs.
pub fn check_part(engine: &Engine, part: &str, component: &Component) -> Result<(), String> {
    let allowed = allowed_imports(part);
    let ty = component.component_type();
    for (name, _) in ty.imports(engine) {
        if !allowed.contains(&name) {
            return Err(format!("it imports {name}, which its part may not"));
        }
    }
    match part {
        "sandboxed" => {
            let mut linker: Linker<SandboxedState> = Linker::new(engine);
            sandboxed::Sandboxed::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            let pre = linker
                .instantiate_pre(component)
                .map_err(|e| format!("its imports aren't the binding's: {e}"))?;
            sandboxed::SandboxedPre::new(pre)
                .map_err(|e| format!("its exports aren't the sandboxed world's: {e}"))?;
        }
        _ => {
            let mut linker: Linker<OutsideState> = Linker::new(engine);
            outside::Outside::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            let pre = linker
                .instantiate_pre(component)
                .map_err(|e| format!("its imports aren't the binding's: {e}"))?;
            outside::OutsidePre::new(pre)
                .map_err(|e| format!("its exports aren't the outside world's: {e}"))?;
        }
    }
    Ok(())
}
