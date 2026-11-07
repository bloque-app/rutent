//! One provider's process (ADR-0003, `REQ-MDL-13`, `B-07`): started isolated by the platform for one release of one
//! provider's module, with no network, no environment and no file to read or write. It reads the release's machine
//! code from its standard input, checks each part's digest against the one the admission artefact records before
//! loading it (`G-08`), starts both parts, and then hands each operation the platform sends to the sandboxed part.
//! Every import a part calls goes to the platform as a request (`rutent_wasm_binding::host`).
//!
//! `--probe [file] [unix socket]` instead prints, as JSON, which of its restrictions held when tested from inside it.

use std::cell::RefCell;
use std::io::{self, StdinLock, StdoutLock};
use std::process::ExitCode;
use std::rc::Rc;

use rutent_wasm_binding::engine;
use rutent_wasm_binding::host::{Loaded, Pipe, Shared};
use rutent_wasm_binding::wire::{FromProvider, ToProvider, hex, sha256};
use wasmtime::component::Component;

type StdPipe = Pipe<StdinLock<'static>, StdoutLock<'static>>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--probe") {
        println!("{}", rutent_wasm_sandbox::probe_report(&args[1..]));
        return ExitCode::SUCCESS;
    }
    let pipe = Rc::new(RefCell::new(Pipe::new(
        io::stdin().lock(),
        io::stdout().lock(),
    )));
    match serve(&pipe) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // The platform reads why; a release that doesn't start is an outcome, not an error to print.
            let _ = pipe.borrow_mut().send(&FromProvider::Failed { message });
            ExitCode::FAILURE
        }
    }
}

fn receive(pipe: &Rc<RefCell<StdPipe>>) -> Result<Option<Vec<u8>>, String> {
    pipe.borrow_mut().receive().map_err(|e| e.to_string())
}

fn load(pipe: &Rc<RefCell<StdPipe>>) -> Result<Loaded, String> {
    let message = receive(pipe)?.ok_or("no machine code")?;
    let Ok(ToProvider::Load { sandboxed, outside }) = serde_json::from_slice(&message) else {
        return Err("the first message wasn't the machine code".to_owned());
    };
    // Both parts' digests are checked before either is loaded, let alone instantiated.
    let mut codes = Vec::new();
    for expected in [sandboxed, outside] {
        let bytes = receive(pipe)?.ok_or("a part's machine code is missing")?;
        if sha256(&bytes) != expected {
            return Err(
                "a part's machine code isn't the one its admission artefact records".to_owned(),
            );
        }
        codes.push(bytes);
    }
    let engine = engine::engine().map_err(|e| format!("{e:#}"))?;
    let mut parts = Vec::new();
    for bytes in codes {
        // SAFETY: these bytes are the machine code the platform's own admitting compiler made from the admitted
        // binary, under this very configuration, and their digests were checked above against the ones the
        // admission artefact records. Nothing a publisher supplies is deserialised (`G-08`).
        let component = unsafe { Component::deserialize(&engine, &bytes) }
            .map_err(|e| format!("its machine code didn't load: {e:#}"))?;
        parts.push(component);
    }
    let outside = parts.pop().ok_or("no outside part")?;
    let sandboxed = parts.pop().ok_or("no sandboxed part")?;
    Loaded::new(engine, sandboxed, outside).map_err(|e| format!("{e:#}"))
}

fn serve(pipe: &Rc<RefCell<StdPipe>>) -> Result<(), String> {
    let loaded = load(pipe)?;
    let platform: Shared = pipe.clone();
    let mut running = loaded.start(platform.clone())?;
    pipe.borrow_mut()
        .send(&FromProvider::Ready)
        .map_err(|e| e.to_string())?;
    while let Some(message) = receive(pipe)? {
        let Ok(ToProvider::Handle { operation }) = serde_json::from_slice(&message) else {
            return Err("a message other than an operation".to_owned());
        };
        let reply = match running.handle(&operation) {
            Ok(output) => FromProvider::Done {
                output: hex(&output),
            },
            Err(message) => FromProvider::Trapped { message },
        };
        let again = matches!(reply, FromProvider::Trapped { .. }) || running.outside_poisoned();
        pipe.borrow_mut().send(&reply).map_err(|e| e.to_string())?;
        if again {
            // A trap leaves an instance unusable: set both parts up again, from the same machine code.
            running = loaded.start(platform.clone())?;
        }
    }
    Ok(())
}
