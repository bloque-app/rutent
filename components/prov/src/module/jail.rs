//! The test-only builder of the jail (`T-025-02`, `DEC-27`, `THR-85`, `GUA-5`): the layers of [`super::sandbox`]
//! composed one at a time, so that a test can probe each alone. It exists only with the `jail-layers` feature, which
//! the package's own tests turn on; the shipped `isolated` and `confine` take [`Layers::ALL`], and a release build
//! carries no layer name, no parser and no setter (a test reads the release binary to show it).

use std::io;
use std::path::Path;
use std::process::Command;

pub use super::layers::{Layer, Layers, NAMES as LAYER_NAMES};
use super::sandbox::{self, Limits, Role};

/// The `unshare` arguments `layers` stands for, ending in `--`, or none if it names no namespace.
pub fn unshare_arguments(layers: Layers) -> Vec<&'static str> {
    sandbox::unshare_args(layers)
}

/// A command that starts `executable` under `layers` alone. A composition that can't stand alone is refused with
/// its reason.
pub fn isolated(
    executable: &Path,
    args: &[&str],
    workdir: &Path,
    limits: Limits,
    layers: Layers,
) -> io::Result<Command> {
    layers
        .check()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    sandbox::isolated_layers(executable, args, workdir, limits, layers)
}

/// Confines this process under `layers` alone, for the probe binary.
pub fn confine(role: Role, layers: Layers) -> Result<(), String> {
    layers.check()?;
    sandbox::confine_layers(role, layers)
}
