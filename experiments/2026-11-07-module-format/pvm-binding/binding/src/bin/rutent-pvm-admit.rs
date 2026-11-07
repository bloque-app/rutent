//! The admitting compiler (ADR-0003: it parses hostile input, so it runs in its own isolation). One process per
//! admission, started by the binding under `isolation::Role::Admit`. It reads one release's two parts, checks
//! each against its part's allowed imports and required exports, compiles both with the binding's engine
//! settings, and answers with each part's digest: of machine code under the compiler, or of the blob under the
//! interpreter, which makes no machine code.
//!
//! On Linux it creates its engine, then confines itself (`isolation::confine`): an empty read-only root, resource
//! limits and a `seccomp` filter, before it reads the release.

use std::time::Instant;

use polkavm::Engine;
use rutent_contracts::module_interface::PartName;
use rutent_pvm_binding::isolation::{self, Role};
use rutent_pvm_binding::protocol::{AdmitRequest, AdmitResponse, receive, send, unhex};
use rutent_pvm_binding::{engine, probe};

fn admit(engine: &Engine, request: &AdmitRequest) -> Result<AdmitResponse, String> {
    let started = Instant::now();
    let mut parts = Vec::new();
    for (part, text) in [
        (PartName::Sandboxed, &request.sandboxed),
        (PartName::Outside, &request.outside),
    ] {
        let bytes = unhex(text).ok_or("a binary not in hexadecimal")?;
        let blob = engine::check(part, &bytes).map_err(|e| format!("{part:?} part: {e}"))?;
        let (_, compiled) =
            engine::compile(engine, &bytes, blob).map_err(|e| format!("{part:?} part: {e}"))?;
        parts.push(compiled);
    }
    let micros = started.elapsed().as_micros();
    let (sandboxed, outside) = (parts.remove(0), parts.remove(0));
    Ok(AdmitResponse::Compiled {
        machine_code: sandboxed.machine_code && outside.machine_code,
        machine_code_len: sandboxed.machine_code_len.zip(outside.machine_code_len),
        sandboxed: sandboxed.digest,
        outside: outside.digest,
        micros,
    })
}

fn main() {
    let started = isolation::started();
    let engine = Engine::new(&engine::config(0));
    if let Err(e) = isolation::confine(&started, Role::Admit) {
        let _ = std::io::Write::write_all(
            &mut std::io::stderr(),
            format!("rutent-pvm-admit: {e}\n").as_bytes(),
        );
        std::process::exit(1);
    }
    if let Some(code) = probe::run(&started.rest) {
        std::process::exit(code);
    }
    let response = match (
        engine,
        receive::<AdmitRequest>(&mut std::io::stdin().lock()),
    ) {
        (Ok(engine), Ok(Some(request))) => {
            admit(&engine, &request).unwrap_or_else(|reason| AdmitResponse::Refused { reason })
        }
        (Err(e), _) => AdmitResponse::Refused {
            reason: format!("no engine: {e}"),
        },
        _ => AdmitResponse::Refused {
            reason: "no request".to_owned(),
        },
    };
    if send(&mut std::io::stdout().lock(), &response).is_err() {
        std::process::exit(2);
    }
}
