//! Where the demo's binaries run from, and where they write.

use std::path::PathBuf;

/// The default output folder: `name` in cargo's target directory, beside the profile the running binary was built
/// under, so a run leaves nothing in the checkout; `<name>-out` in the current folder where that can't be found.
pub fn default_out(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.ancestors()
                .skip(1)
                .find(|d| d.file_name().is_some_and(|n| n != "deps"))
                .and_then(|profile| profile.parent())
                .map(|target| target.join(name))
        })
        .unwrap_or_else(|| PathBuf::from(format!("{name}-out")))
}

/// Runs the binary again from cargo's own copy of it under `deps/`, where it isn't already running from there.
/// `stem` is the binary's name as cargo writes it there, with `_` for `-`: `rutent_demo` for `rutent-demo`.
///
/// The assembly finds the platform's executables and the simulated providers' build by the build directory a *test*
/// executable sits in: the parent of the nearest `deps` or `build` directory above it
/// (`rutent_scenarios::sim::disk`). Cargo writes a binary as `<target>/<profile>/deps/<stem>-<hash>` and links it to
/// `<target>/<profile>/<name>`, which has no such directory above it. Run from the copy under `deps/`, the assembly
/// finds what it looks for, and nothing of `rutent-scenarios` changes.
#[cfg(unix)]
pub fn from_deps(stem: &str) {
    use std::os::unix::fs::MetadataExt as _;
    use std::os::unix::process::CommandExt as _;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = exe.parent() else { return };
    if dir.file_name().is_some_and(|n| n == "deps" || n == "out") {
        return;
    }
    let Ok(this) = std::fs::metadata(&exe) else {
        return;
    };
    let Ok(listed) = std::fs::read_dir(dir.join("deps")) else {
        return;
    };
    let prefix = format!("{stem}-");
    let mut candidates: Vec<(bool, std::time::SystemTime, PathBuf)> = listed
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && !n.contains('.'))
        })
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let same = meta.dev() == this.dev() && meta.ino() == this.ino();
            Some((same, meta.modified().ok()?, p))
        })
        .collect();
    // The one cargo linked this binary from, else the newest.
    candidates.sort();
    if let Some((_, _, copy)) = candidates.pop() {
        let error = std::process::Command::new(&copy)
            .args(std::env::args_os().skip(1))
            .exec();
        eprintln!("couldn't run {}: {error}", copy.display());
    }
}

/// Elsewhere the binary runs where it is.
#[cfg(not(unix))]
pub fn from_deps(_stem: &str) {}
