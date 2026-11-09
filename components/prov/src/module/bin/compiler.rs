//! The admitting compiler (ADR-0003, `B-08`): one fresh process per admission, started isolated by the platform
//! (`rutent_prov::module::sandbox::isolated`), with no network, no environment, no file to read or write and no secret. It
//! reads a [`CompileRequest`] and the two parts' portable binaries on its standard input, and writes, per part, a
//! [`Compiled`] verdict and the machine code Wasmtime made from the binary. It never loads machine code it is given
//! (`B-15`, `G-08`): a binary Wasmtime recognises as precompiled is refused.
//!
//! `--probe [file] [unix socket] [pid]` instead prints, as JSON, which of its restrictions held when tested from inside it.

use std::io::{self, Write};
use std::process::ExitCode;

use rutent_prov::module::engine::{self, MAX_BINARY};
use rutent_prov::module::host;
use rutent_prov::module::sandbox::{read_frame, write_frame};
use rutent_prov::module::wire::{CompileRequest, Compiled};
use wasmtime::Engine;
use wasmtime::component::Component;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Before anything is read: on Linux the jail (`rutent_prov::module::sandbox::confine`), elsewhere nothing to do.
    if let Err(message) =
        rutent_prov::module::sandbox::confine(rutent_prov::module::sandbox::Role::Compiler)
    {
        eprintln!("rutent-wasm-compiler: not confined, so not started: {message}");
        return ExitCode::FAILURE;
    }
    if args.first().map(String::as_str) == Some("--probe") {
        println!("{}", rutent_prov::module::sandbox::probe_report(&args[1..]));
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("rutent-wasm-compiler: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let request = read_frame(&mut input)
        .map_err(|e| e.to_string())?
        .ok_or("no request")?;
    let request: CompileRequest = serde_json::from_slice(&request).map_err(|e| e.to_string())?;
    if request.platform_version != engine::platform_version() {
        return Err(format!(
            "asked for platform version {}, but this is {}",
            request.platform_version,
            engine::platform_version()
        ));
    }
    // Both binaries are read before anything is written, so that neither side blocks on a full pipe.
    let mut binaries = Vec::new();
    for _ in 0..2 {
        binaries.push(
            read_frame(&mut input)
                .map_err(|e| e.to_string())?
                .ok_or("a part is missing")?,
        );
    }
    let engine = engine::engine().map_err(|e| format!("{e:#}"))?;
    for (part, binary) in ["sandboxed", "outside"].into_iter().zip(&binaries) {
        let (verdict, machine_code) = compile(&engine, part, binary);
        let verdict = serde_json::to_vec(&verdict).map_err(|e| e.to_string())?;
        write_frame(&mut output, &verdict).map_err(|e| e.to_string())?;
        write_frame(&mut output, &machine_code).map_err(|e| e.to_string())?;
    }
    output.flush().map_err(|e| e.to_string())
}

fn compile(engine: &Engine, part: &str, binary: &[u8]) -> (Compiled, Vec<u8>) {
    let mut verdict = Compiled {
        part: part.to_owned(),
        refusal: None,
        disallowed_import: false,
        imports: Vec::new(),
        exports: Vec::new(),
    };
    let refuse = |mut verdict: Compiled, why: String| {
        verdict.refusal = Some(why);
        (verdict, Vec::new())
    };
    if binary.len() > MAX_BINARY {
        return refuse(
            verdict,
            format!("{} bytes is over the {MAX_BINARY}-byte limit", binary.len()),
        );
    }
    if Engine::detect_precompiled(binary).is_some() {
        return refuse(
            verdict,
            "it is precompiled machine code, which is never loaded".to_owned(),
        );
    }
    let component = match Component::new(engine, binary) {
        Ok(component) => component,
        Err(e) => {
            return refuse(
                verdict,
                format!("it isn't a component Wasmtime takes: {e:#}"),
            );
        }
    };
    let ty = component.component_type();
    verdict.imports = ty.imports(engine).map(|(n, _)| n.to_owned()).collect();
    verdict.exports = ty.exports(engine).map(|(n, _)| n.to_owned()).collect();
    if let Err(refusal) = host::check_part(engine, part, &component) {
        verdict.disallowed_import = refusal.import;
        return refuse(verdict, refusal.why);
    }
    match component.serialize() {
        Ok(machine_code) => (verdict, machine_code),
        Err(e) => refuse(
            verdict,
            format!("its machine code couldn't be written: {e:#}"),
        ),
    }
}
