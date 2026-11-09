//! The guests' build tool, which `build.sh` drives:
//!
//! - `componentize <core.wasm> <component.wasm>`: wraps a core module, whose component-type custom section names
//!   its world, into a component (`wit-component`);
//! - `c-bindings <wit-dir> <world> <out-dir>`: writes the C bindings of a world (`wit-bindgen-c`), the
//!   `*_component_type.o` object included;
//! - `slot-header <out.h>`: writes the C stand-ins' program slot initializer;
//! - `hostile <out-dir>`: writes the components the admission tests judge, each a sandboxed part importing the
//!   binding's two interfaces and one thing more (or nothing more, for the control);
//! - `digest <file>...`: prints each file's SHA-256 digest.

use std::path::Path;
use std::process::ExitCode;

use sha2::{Digest, Sha256};

/// The program slot's size, in bytes. The stand-ins' own code and the tests' `program` module agree on it.
const SLOT: usize = 8192;

/// The slot's first line, in the template and in every program.
const MAGIC: &str = "RUTENT-PROGRAM-SLOT-V1\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let outcome = match args.as_slice() {
        ["componentize", core, out] => componentize(Path::new(core), Path::new(out)),
        ["c-bindings", wit, world, out] => c_bindings(Path::new(wit), world, Path::new(out)),
        ["slot-header", out] => std::fs::write(out, c_slot_header()).map_err(|e| e.to_string()),
        ["hostile", out] => hostile(Path::new(out)),
        ["digest", files @ ..] => files.iter().try_for_each(|file| {
            let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
            let sum: String = Sha256::digest(&bytes)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            println!("{sum}  {file}");
            Ok(())
        }),
        _ => Err(
            "usage: rutent-wasm-guests componentize|c-bindings|slot-header|hostile|digest ..."
                .to_owned(),
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

/// The C stand-in's slot initializer: the magic line, then `#` to the slot's size, as one string literal of
/// exactly [`SLOT`] characters.
fn c_slot_header() -> String {
    let mut text = String::from(
        "/* Written by `rutent-wasm-guests slot-header`: the program slot of a C stand-in. */\n\
         #define RUTENT_SLOT_SIZE 8192\n#define RUTENT_SLOT_INIT \\\n",
    );
    text.push_str(&format!("  \"{}\" \\\n", MAGIC.replace('\n', "\\n")));
    let mut left = SLOT - MAGIC.len();
    while left > 0 {
        let n = left.min(64);
        text.push_str(&format!("  \"{}\"", "#".repeat(n)));
        left -= n;
        text.push_str(if left > 0 { " \\\n" } else { "\n" });
    }
    text
}

/// The components the admission tests judge: `hostile-control.wasm` imports nothing beyond the binding's two
/// interfaces, and each of the others imports one thing more. Written with `wit-component`'s dummy module, whose
/// functions do nothing, which is enough for admission to judge a part's imports.
fn hostile(out: &Path) -> Result<(), String> {
    for (file, import) in [
        ("hostile-control.wasm", ""),
        ("hostile-clock.wasm", "import wasi:clocks/wall-clock@0.2.0;"),
        ("hostile-file.wasm", "import wasi:filesystem/types@0.2.0;"),
        ("hostile-socket.wasm", "import wasi:sockets/tcp@0.2.0;"),
        ("hostile-undeclared.wasm", "import undeclared;"),
    ] {
        let component = hostile_component(import)?;
        std::fs::write(out.join(file), component).map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}

fn hostile_component(imports: &str) -> Result<Vec<u8>, String> {
    use wit_parser_of_wit_component::{ManglingAndAbi, Resolve};
    let wit = format!(
        r#"package rutent:binding@0.1.0;

interface outside-requests {{ request: func(outside-request: string) -> string; }}
interface denied {{
    look-up-secret: func(name: string);
    read-state: func(key: string);
}}
interface undeclared {{ anything: func(what: string) -> string; }}

world sandboxed {{
    import outside-requests;
    import denied;
    {imports}
    export start: func() -> result<_, string>;
    export handle: func(operation: string) -> list<u8>;
}}

package wasi:clocks@0.2.0 {{ interface wall-clock {{ now: func() -> u64; }} }}
package wasi:filesystem@0.2.0 {{ interface types {{ read: func(path: string) -> list<u8>; }} }}
package wasi:sockets@0.2.0 {{ interface tcp {{ connect: func(host: string, port: u16) -> bool; }} }}
"#
    );
    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("hostile.wit", &wit)
        .map_err(|e| format!("{e:#}"))?;
    let world = resolve
        .select_world(&[package], Some("rutent:binding/sandboxed@0.1.0"))
        .map_err(|e| format!("{e:#}"))?;
    let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .map_err(|e| format!("{e:#}"))?;
    wit_component::ComponentEncoder::default()
        .module(&module)
        .and_then(|e| e.validate(true).encode())
        .map_err(|e| format!("{e:#}"))
}
