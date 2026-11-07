//! The stand-ins' build tool, which `build.sh` runs. Never on admission's path.
//!
//! - `rutent-pvm-tool target-json <dir>`: writes PolkaVM's 64-bit Rust target JSON (the `1_91` one) under `<dir>`,
//!   through `polkavm_linker::target_json_path`, with the `dirs` stub's cache directory set to `<dir>`
//!   explicitly; prints its path.
//! - `rutent-pvm-tool link <elf> <blob>`: links a RISC-V ELF into a PolkaVM blob, stripped, with the linker's
//!   elision of loads it judges unnecessary off, so no read of the spec slot can be folded.

use std::path::PathBuf;

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, dir] if command == "target-json" => {
            if !dirs::set_cache_dir(PathBuf::from(dir)) {
                return Err("the cache directory was already set".to_owned());
            }
            let mut json = polkavm_linker::TargetJsonArgs::default();
            json.rustc_version = polkavm_linker::RustcVersion::Rustc_1_91;
            json.is_64_bit = true;
            let path = polkavm_linker::target_json_path(json)?;
            println!("{}", path.display());
            Ok(())
        }
        [command, elf, blob] if command == "link" => {
            let data = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
            let mut config = polkavm_linker::Config::default();
            config.set_strip(true);
            config.set_elide_unnecessary_loads(false);
            let program = polkavm_linker::program_from_elf(
                config,
                polkavm_linker::TargetInstructionSet::Latest,
                &data,
            )
            .map_err(|e| format!("{elf}: {e}"))?;
            std::fs::write(blob, program).map_err(|e| format!("{blob}: {e}"))
        }
        _ => Err("usage: rutent-pvm-tool target-json <dir> | link <elf> <blob>".to_owned()),
    }
}
