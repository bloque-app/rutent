//! The admitting compiler (ADR-0003: it parses hostile input, so it runs in its own isolation). One process per
//! admission, started by the binding under `isolation::Role::Admit`. It reads one release's two parts, checks
//! each against its part's allowed imports and required exports, compiles both with the binding's engine
//! settings, and answers with each part's digest: of machine code under the compiler, or of the blob under the
//! interpreter, which makes no machine code.

use std::time::Instant;

use polkavm::Engine;
use rutent_contracts::module_interface::PartName;
use rutent_pvm_binding::engine;
use rutent_pvm_binding::protocol::{AdmitRequest, AdmitResponse, receive, send, unhex};

fn admit(request: &AdmitRequest) -> Result<AdmitResponse, String> {
    let engine = Engine::new(&engine::config(0)).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut parts = Vec::new();
    for (part, text) in [
        (PartName::Sandboxed, &request.sandboxed),
        (PartName::Outside, &request.outside),
    ] {
        let bytes = unhex(text).ok_or("a binary not in hexadecimal")?;
        let blob = engine::check(part, &bytes).map_err(|e| format!("{part:?} part: {e}"))?;
        let (_, compiled) =
            engine::compile(&engine, &bytes, blob).map_err(|e| format!("{part:?} part: {e}"))?;
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
    let response = match receive::<AdmitRequest>(&mut std::io::stdin().lock()) {
        Ok(Some(request)) => {
            admit(&request).unwrap_or_else(|reason| AdmitResponse::Refused { reason })
        }
        _ => AdmitResponse::Refused {
            reason: "no request".to_owned(),
        },
    };
    if send(&mut std::io::stdout().lock(), &response).is_err() {
        std::process::exit(2);
    }
}
