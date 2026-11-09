//! The stand-ins' build tool, which `run.sh` drives:
//!
//! - `componentize <core.wasm> <component.wasm>`: wraps a core module, whose component-type custom section names
//!   its world, into a component (`wit-component`);
//! - `c-bindings <wit-dir> <world> <out-dir>`: writes the C bindings of a world (`wit-bindgen-c`), the
//!   `*_component_type.o` object included;
//! - `slot-header <out.h>`: writes the C stand-ins' program slot initializer;
//! - `digest <file>...`: prints each file's SHA-256 digest.

use std::path::Path;
use std::process::ExitCode;

use rutent_wasm_binding::{program, wire};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = match args.as_slice() {
        ["componentize", core, out] => componentize(Path::new(core), Path::new(out)),
        ["c-bindings", wit, world, out] => c_bindings(Path::new(wit), world, Path::new(out)),
        ["slot-header", out] => {
            std::fs::write(out, program::c_slot_header()).map_err(|e| e.to_string())
        }
        ["digest", files @ ..] => files.iter().try_for_each(|file| {
            let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            println!("{}  {file}", wire::sha256(&bytes));
            Ok(())
        }),
        _ => Err(
            "usage: rutent-wasm-guests componentize|c-bindings|slot-header|digest ...".to_owned(),
        ),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("rutent-wasm-guests: {message}");
            ExitCode::FAILURE
        }
    }
}

fn componentize(core: &Path, out: &Path) -> Result<(), String> {
    let module = std::fs::read(core).map_err(|e| format!("{}: {e}", core.display()))?;
    let component = wit_component::ComponentEncoder::default()
        .module(&module)
        .and_then(|encoder| encoder.validate(true).encode())
        .map_err(|e| format!("{}: {e:#}", core.display()))?;
    std::fs::write(out, component).map_err(|e| format!("{}: {e}", out.display()))
}

fn c_bindings(wit: &Path, world: &str, out: &Path) -> Result<(), String> {
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve.push_dir(wit).map_err(|e| format!("{e:#}"))?;
    let world = resolve
        .select_world(&[package], Some(world))
        .map_err(|e| format!("{e:#}"))?;
    let mut files = wit_bindgen_core::Files::default();
    wit_bindgen_c::Opts::default()
        .build()
        .generate(&mut resolve, world, &mut files)
        .map_err(|e| format!("{e:#}"))?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for (name, bytes) in files.iter() {
        std::fs::write(out.join(name), bytes).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}
