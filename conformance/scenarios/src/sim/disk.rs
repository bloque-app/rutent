//! Finding what a platform needs on disk: the admitting compiler and a provider's process, which are `rutent-prov`'s
//! two executables, and the simulated providers' components, which `module-guests/sim/build.sh` builds.
//!
//! Both are built the first time they are asked for, into the build directory the running test is in, and again when
//! a source they come from is newer. The simulated providers' build takes its own build directory beneath it, so it
//! never waits on the one that is running the test.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::SystemTime;

use rutent_prov::module::children::Paths;

/// The build directory the running executable is in, and the profile's directory in it.
fn build_directories() -> (PathBuf, PathBuf) {
    let exe = std::env::current_exe().expect("the running executable's path");
    // target/<profile>/deps/<executable>
    let profile = exe
        .ancestors()
        .nth(2)
        .expect("a profile directory")
        .to_path_buf();
    let target = profile.parent().expect("a build directory").to_path_buf();
    (target, profile)
}

/// The repository's root, from this package's manifest.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// The platform's two executables, built if they aren't.
pub fn paths() -> Paths {
    static BUILT: OnceLock<Paths> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let (target, profile) = build_directories();
            let mut build = cargo();
            build
                .args([
                    "build",
                    "--quiet",
                    "-p",
                    "rutent-prov",
                    "--bins",
                    "--target-dir",
                ])
                .arg(&target);
            if profile.file_name().is_some_and(|n| n == "release") {
                build.arg("--release");
            }
            let status = build.status().expect("cargo runs");
            assert!(
                status.success(),
                "building the platform's executables failed: {status}"
            );
            Paths {
                compiler: profile.join("rutent-wasm-compiler"),
                provider: profile.join("rutent-wasm-provider"),
            }
        })
        .clone()
}

/// The newest modification time of any file under `dir`.
fn newest(dir: &Path) -> SystemTime {
    let mut newest = SystemTime::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return newest;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let time = if path.is_dir() {
            newest_of(&path)
        } else {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };
        newest = newest.max(time);
    }
    newest
}

fn newest_of(dir: &Path) -> SystemTime {
    newest(dir)
}

/// The directory holding `sim-{role}-sandboxed.wasm` for each role and `sim-outside.wasm`: `RUTENT_SIM_GUESTS` if
/// set, else the one `module-guests/sim/build.sh` writes into the build directory.
pub fn guests() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = match std::env::var_os("RUTENT_SIM_GUESTS") {
            Some(dir) => PathBuf::from(dir),
            None => built_guests(),
        };
        assert!(
            dir.join("sim-reserve-sandboxed.wasm").exists(),
            "{} holds no simulated providers: run components/prov/module-guests/sim/build.sh",
            dir.display()
        );
        dir
    })
    .clone()
}

fn built_guests() -> PathBuf {
    let (target, _) = build_directories();
    let root = root();
    let module_guests = root.join("components/prov/module-guests");
    let out = target.join("rutent-sim-guests");
    let built = out.join("built");
    let sources = [
        newest(&module_guests),
        newest(&root.join("components/prov/src/module/wit")),
        newest(&root.join("conformance/src")),
        newest(&root.join("contracts/src")),
    ]
    .into_iter()
    .max()
    .expect("a time");
    let current = std::fs::metadata(&built)
        .and_then(|m| m.modified())
        .is_ok_and(|built| built >= sources);
    if !current {
        let status = Command::new(module_guests.join("sim/build.sh"))
            .arg(&out)
            .env("CARGO_TARGET_DIR", target.join("sim-guests-build"))
            .status()
            .expect("components/prov/module-guests/sim/build.sh runs");
        assert!(
            status.success(),
            "the simulated providers' build failed: {status}"
        );
    }
    out
}
