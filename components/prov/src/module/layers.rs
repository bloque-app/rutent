//! Which layers of the jail are applied (`T-025-02`, `DEC-27`, `THR-85`).
//!
//! In a shipped build [`Layers`] is a zero-sized type for which every layer is on, and nothing can say otherwise: no
//! string, no parser and no setter is compiled in. With the `jail-layers` feature, which only the package's own
//! tests turn on (a dev-dependency on itself), it is a set that the test-only builder in [`crate::module::jail`]
//! composes one layer at a time.

/// One layer of the jail.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The user namespace (root mapped to the platform's user).
    User,
    /// The network namespace.
    Net,
    /// The IPC namespace.
    Ipc,
    /// The UTS namespace.
    Uts,
    /// The mount namespace.
    Mount,
    /// The PID namespace (the child is PID 1 of it).
    Pid,
    /// `no_new_privs`.
    NoNewPrivs,
    /// The pivot to an empty, read-only root.
    EmptyRoot,
    /// Every capability dropped.
    DropCaps,
    /// Landlock with no rule.
    Landlock,
    /// The seccomp allow-list.
    Seccomp,
    /// Inherited descriptors closed before `exec`.
    CloseBeforeExec,
    /// Descriptors closed again inside the jail.
    CloseInJail,
    /// `RLIMIT_AS`.
    AddressSpace,
    /// The other resource limits (`RLIMIT_NPROC`, `RLIMIT_CORE`, `RLIMIT_FSIZE`, `RLIMIT_NOFILE`, `RLIMIT_CPU`).
    Limits,
}

#[cfg(not(feature = "jail-layers"))]
/// Every layer, always.
#[derive(Debug, Clone, Copy)]
pub struct Layers;

#[cfg(not(feature = "jail-layers"))]
impl Layers {
    /// Every layer.
    pub const ALL: Self = Layers;

    /// Whether `layer` is applied: always.
    #[inline(always)]
    pub const fn on(self, _layer: Layer) -> bool {
        true
    }
}

#[cfg(feature = "jail-layers")]
pub use set::{Layers, NAMES};

#[cfg(feature = "jail-layers")]
mod set {
    use super::Layer;

    /// Every layer, with the names the probe binary's command line uses.
    pub const NAMES: [(Layer, &str); 15] = [
        (Layer::User, "jail-layer:user"),
        (Layer::Net, "jail-layer:net"),
        (Layer::Ipc, "jail-layer:ipc"),
        (Layer::Uts, "jail-layer:uts"),
        (Layer::Mount, "jail-layer:mount"),
        (Layer::Pid, "jail-layer:pid"),
        (Layer::NoNewPrivs, "jail-layer:no-new-privs"),
        (Layer::EmptyRoot, "jail-layer:empty-root"),
        (Layer::DropCaps, "jail-layer:caps"),
        (Layer::Landlock, "jail-layer:landlock"),
        (Layer::Seccomp, "jail-layer:seccomp"),
        (Layer::CloseBeforeExec, "jail-layer:close-before-exec"),
        (Layer::CloseInJail, "jail-layer:close-in-jail"),
        (Layer::AddressSpace, "jail-layer:address-space"),
        (Layer::Limits, "jail-layer:limits"),
    ];

    /// A set of layers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct Layers(u32);

    fn bit(layer: Layer) -> u32 {
        1 << NAMES.iter().position(|(l, _)| *l == layer).unwrap_or(31)
    }

    impl Layers {
        /// Every layer.
        pub const ALL: Self = Layers((1 << 15) - 1);

        /// No layer.
        pub const fn none() -> Self {
            Layers(0)
        }

        /// This set with `layer` added.
        pub fn with(self, layer: Layer) -> Self {
            Layers(self.0 | bit(layer))
        }

        /// Whether `layer` is applied.
        pub fn on(self, layer: Layer) -> bool {
            self.0 & bit(layer) != 0
        }

        /// The names of the layers in the set, comma-separated.
        pub fn names(self) -> String {
            NAMES
                .iter()
                .filter(|(l, _)| self.on(*l))
                .map(|(_, n)| *n)
                .collect::<Vec<_>>()
                .join(",")
        }

        /// The set a comma-separated list of names stands for.
        pub fn parse(text: &str) -> Result<Self, String> {
            let mut set = Layers::none();
            for name in text.split(',').filter(|n| !n.is_empty()) {
                let (layer, _) = NAMES
                    .iter()
                    .find(|(_, n)| *n == name)
                    .ok_or_else(|| format!("jail-layer: unknown layer {name}"))?;
                set = set.with(*layer);
            }
            Ok(set)
        }

        /// Why this composition can't be made, if it can't: a layer that needs another beneath it.
        pub fn check(self) -> Result<(), String> {
            let needs = |layer: Layer, wants: &[Layer], why: &str| {
                if self.on(layer) && !wants.iter().all(|w| self.on(*w)) {
                    Err(format!("{layer:?} can't stand alone: {why}"))
                } else {
                    Ok(())
                }
            };
            let userns = "an unprivileged process needs the user namespace to make it";
            for layer in [Layer::Net, Layer::Ipc, Layer::Uts, Layer::Mount, Layer::Pid] {
                needs(layer, &[Layer::User], userns)?;
            }
            needs(
                Layer::EmptyRoot,
                &[Layer::Mount, Layer::User],
                "the root pivot needs the mount namespace beneath it, and the user namespace to make that",
            )?;
            needs(
                Layer::DropCaps,
                &[Layer::User],
                "an unprivileged process holds no capability to drop without the user namespace",
            )?;
            for layer in [Layer::Landlock, Layer::Seccomp] {
                if self.on(layer) && !self.on(Layer::NoNewPrivs) && !self.on(Layer::User) {
                    return Err(format!(
                        "{layer:?} can't stand alone: an unprivileged process may install it only with \
                         no_new_privs or a user namespace's capabilities"
                    ));
                }
            }
            Ok(())
        }
    }
}
