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
    let profile = profile_of(&exe).expect("a profile directory above the running executable");
    let target = profile.parent().expect("a build directory").to_path_buf();
    (target, profile)
}

/// The profile's directory `exe`, a test's executable, was built under: the parent of the nearest `deps` or `build`
/// directory above it. Cargo places a test's executable at `<target>/<profile>/deps/<exe>` in its old layout, and at
/// `<target>/<profile>/build/<package>/<hash>/out/<exe>` in its new build-directory layout, the default from Rust
/// 1.100 (`-Zbuild-dir-new-layout` before it).
pub(crate) fn profile_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .find(|dir| {
            dir.file_name()
                .is_some_and(|name| name == "deps" || name == "build")
        })
        .and_then(Path::parent)
        .map(Path::to_path_buf)
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
            // In rutent's own workspace, whichever workspace runs the test: `rutent-scenarios-vos` runs these in the
            // VOS client's (`T-024-02`).
            build
                .args(["build", "--quiet", "--locked", "--manifest-path"])
                .arg(root().join("Cargo.toml"))
                .args(["-p", "rutent-prov", "--bins", "--target-dir"])
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

/// A key for the sources `roots` hold: a hash of every file's path, relative to its root, and bytes, in order. Two
/// trees with the same sources have the same key and two that differ have different ones, whatever their files'
/// modification times and wherever they are checked out.
fn key_of(roots: &[PathBuf]) -> String {
    use rutent_prov::module::wire::sha256;

    fn files(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files(&path, found);
            } else {
                found.push(path);
            }
        }
    }

    let mut whole = Vec::new();
    for (n, root) in roots.iter().enumerate() {
        let mut found = Vec::new();
        files(root, &mut found);
        found.sort();
        for path in found {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let bytes = std::fs::read(&path).unwrap_or_default();
            whole.extend_from_slice(
                format!("{n}:{}:{}\n", relative.display(), bytes.len()).as_bytes(),
            );
            whole.extend_from_slice(&bytes);
        }
    }
    sha256(&whole)[..24].to_owned()
}

/// How many other builds of the simulated providers are kept beside the one in use: a build directory is shared by
/// every worktree that builds into it, and each of them has its own.
const KEPT_BUILDS: usize = 3;

/// Removes all but the newest [`KEPT_BUILDS`] builds under `parent` besides `current`, and what an earlier layout
/// left loose in `parent`.
fn prune(parent: &Path, current: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let mut builds = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path == current {
            continue;
        }
        if path.is_dir() {
            let time = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            builds.push((time, path));
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    builds.sort();
    let surplus = builds.len().saturating_sub(KEPT_BUILDS);
    for (_, path) in builds.into_iter().take(surplus) {
        let _ = std::fs::remove_dir_all(path);
    }
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

/// The simulated providers, built into a directory named by a hash of the sources they are built from, so a build
/// is never taken for another tree's: the build directory is shared by every worktree that runs tests in it, and a
/// marker's age says nothing of what it was built from.
fn built_guests() -> PathBuf {
    let (target, _) = build_directories();
    let root = root();
    let module_guests = root.join("components/prov/module-guests");
    let key = key_of(&[
        module_guests.clone(),
        root.join("components/prov/src/module/wit"),
        root.join("conformance/src"),
        root.join("contracts/src"),
    ]);
    let builds = target.join("rutent-sim-guests");
    let out = builds.join(&key);
    if !out.join("built").exists() {
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
    prune(&builds, &out);
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{key_of, profile_of};

    /// A build of the simulated providers is named by what it is built from, not by a file's age: the same sources
    /// give the same key wherever they are and whenever they were written, and any change of a path or a byte gives
    /// another.
    // verifies: DEC-18
    #[test]
    fn guests_key_follows_the_sources_and_not_their_age_or_place() {
        let base = std::env::temp_dir().join(format!("rutent-guests-key-{}", std::process::id()));
        let write = |dir: &str, name: &str, text: &str| {
            let path = base.join(dir).join(name);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
            std::fs::write(path, text).expect("a file");
        };
        write("a", "x.rs", "one");
        write("b", "x.rs", "one");
        write("c", "x.rs", "two");
        write("d", "y.rs", "one");
        let key = |dir: &str| key_of(&[base.join(dir)]);
        assert_eq!(key("a"), key("b"), "the same sources, another place");
        assert_ne!(key("a"), key("c"), "another byte");
        assert_ne!(key("a"), key("d"), "another path");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The profile is found under both of cargo's layouts, whatever the directories above the build directory are
    /// called.
    // verifies: DEC-18
    #[test]
    fn profile_is_found_under_both_of_cargos_layouts() {
        for exe in [
            "/work/build/rutent/target/debug/deps/rutent_scenarios-1ae828f0fa3ad402",
            "/work/build/rutent/target/debug/build/rutent-scenarios/1ae828f0fa3ad402/out/rutent_scenarios-1ae828f0fa3ad402",
        ] {
            assert_eq!(
                profile_of(Path::new(exe)).as_deref(),
                Some(Path::new("/work/build/rutent/target/debug")),
                "{exe}"
            );
        }
        assert_eq!(
            profile_of(Path::new(
                "/work/target/x86_64-unknown-linux-gnu/release/build/p/h/out/e"
            ))
            .as_deref(),
            Some(Path::new("/work/target/x86_64-unknown-linux-gnu/release"))
        );
    }
}
