//! The host's configuration: the deployment it answers for, and each reader it runs (`T-015-04`).
//!
//! It is read at start, and again at each restart ([`HostConfig::read`]): a restarted host runs what its
//! configuration says then, and each reader starts refusing until its checks pass again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rutent_contracts::provider_role::{Deployment, PublicKey};
use rutent_contracts::reader::{NetworkIdentity, TrustRule, check_pairing};
use rutent_contracts::router_api::{Id, ProtocolVersion, Version};
use serde::{Deserialize, Serialize};

/// What the host runs, and for whom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostConfig {
    /// Core's deployment: each answer's envelope names it (`REQ-PRT-1`).
    pub deployment: Deployment,
    /// The protocol version each answer's envelope names (`REQ-PRT-1`).
    pub protocol_version: ProtocolVersion,
    /// The readers, each a child process of its own.
    pub readers: Vec<ReaderConfig>,
}

/// One reader: who it is, the network it reads, the program the host starts for it, its statement key by name, its
/// trust rule and what it may reach.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderConfig {
    /// The reader's name on this host: the maker of its answers, which the listing's endpoint names.
    pub name: Id,
    /// The network it reads, as an outer node names it.
    pub network: Id,
    /// The reader's version, which each answer names and Core holds to the listing's (`REQ-AUD-4`).
    pub version: Version,
    /// The network's identity, which the reader's start-up checks must report before it answers anything.
    pub identity: NetworkIdentity,
    /// The reader's trust rule, which its `network_rules` answer must name.
    pub trust: TrustRule,
    /// The executable the host starts for it, and its arguments. Its environment is cleared.
    pub program: PathBuf,
    /// The program's arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// The reader's statement key, the public half the listing names: the host resolves its private half by this
    /// key's text, under the reader's name, as the platform resolves a provider's, and never hands it to the reader's
    /// process (`REQ-MDL-6` applied to readers).
    pub statement_key: PublicKey,
    /// What a light-client reader may reach: required for `light_client`, refused for `rpc_pool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_client: Option<LightClientRule>,
    /// The host's bounds on the reader's process.
    #[serde(default)]
    pub limits: ReaderLimits,
}

/// How a light client's connections are reached (`DEC-28`): by protocol and port, not by address, since its peers
/// are discovered, with a cap on how many it holds at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightClientRule {
    /// The protocols and ports it may connect to: the network's p2p ports and `wss`.
    pub ports: Vec<PortRule>,
    /// The most connections it may hold open at once.
    pub max_connections: u32,
}

/// One protocol and port a light client may connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortRule {
    /// The protocol.
    pub protocol: Protocol,
    /// The port.
    pub port: u16,
}

/// The protocols a light client speaks to its peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// The network's own peer-to-peer transport over TCP.
    P2p,
    /// WebSocket over TLS.
    Wss,
}

impl Protocol {
    /// Its name, as a record writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::P2p => "p2p",
            Protocol::Wss => "wss",
        }
    }
}

/// The host's bounds on one reader's process (`REQ-MDL-11`). Each is the host's own, on its own monotonic clock;
/// none is the answer's deadline, which is Core's and carried in the question. The reader's memory is bounded by the
/// jail, as a provider's process's is ([`rutent_prov::confine::PROVIDER_ADDRESS_SPACE`]), and the operator's task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ReaderLimits {
    /// How long the reader may take over one question before the host takes it as hung, kills it, answers the
    /// question unreadable and restarts it, in milliseconds.
    pub hang_after_ms: u64,
    /// How long a reader may take over its start-up checks before the host kills it and starts it again, in
    /// milliseconds.
    pub start_within_ms: u64,
    /// The most questions waiting for the reader, the one it is on included; one more is answered unreadable at once.
    pub pending: u32,
    /// How long the host waits before it starts a reader again after a crash, a hang, a limit hit or failed checks,
    /// in milliseconds.
    pub restart_after_ms: u64,
    /// How long one egress call may take, every hop included, before the host gives up on it, in milliseconds.
    pub egress_give_up_after_ms: u64,
    /// The largest frame the reader may send the host, in bytes; a longer one is a limit hit. The frames waiting for
    /// the host from one reader are bounded too, at four such frames' worth of bytes and [`READ_FRAMES`] in number,
    /// and those waiting for the reader at [`WRITE_FRAMES`] and [`WRITE_BYTES`]: a reader over either is stopped.
    pub max_frame: u32,
    /// The most egress calls (RPC requests, light-client connections and sends on them) the reader may ask for over one
    /// question, or over its start-up checks; one more is a limit hit.
    pub calls_per_question: u32,
}

/// The most frames from one reader waiting for the host to read them.
pub const READ_FRAMES: usize = 64;

/// The most frames waiting to be written to one reader.
pub const WRITE_FRAMES: usize = 32;

/// The most bytes waiting to be written to one reader.
pub const WRITE_BYTES: usize = 4 << 20;

impl Default for ReaderLimits {
    fn default() -> Self {
        ReaderLimits {
            hang_after_ms: 10_000,
            start_within_ms: 60_000,
            pending: 16,
            restart_after_ms: 1_000,
            egress_give_up_after_ms: 5_000,
            max_frame: 1 << 20,
            calls_per_question: 64,
        }
    }
}

impl ReaderLimits {
    /// [`ReaderLimits::hang_after_ms`].
    pub fn hang_after(&self) -> Duration {
        Duration::from_millis(self.hang_after_ms)
    }

    /// [`ReaderLimits::start_within_ms`].
    pub fn start_within(&self) -> Duration {
        Duration::from_millis(self.start_within_ms)
    }

    /// [`ReaderLimits::restart_after_ms`].
    pub fn restart_after(&self) -> Duration {
        Duration::from_millis(self.restart_after_ms)
    }

    /// [`ReaderLimits::egress_give_up_after_ms`].
    pub fn egress_give_up_after(&self) -> Duration {
        Duration::from_millis(self.egress_give_up_after_ms)
    }
}

/// Why a configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for ConfigError {}

impl HostConfig {
    /// Reads and checks the configuration at `path`, a JSON file.
    pub fn read(path: &Path) -> Result<Self, ConfigError> {
        let text =
            std::fs::read(path).map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
        let config: HostConfig = serde_json::from_slice(&text)
            .map_err(|e| ConfigError(format!("{}: {e}", path.display())))?;
        config.check()?;
        Ok(config)
    }

    /// Checks what JSON can't: each reader named once, its identity paired with its trust rule, a light-client rule
    /// exactly for a light client, and bounds above zero.
    pub fn check(&self) -> Result<(), ConfigError> {
        let bad = |reader: &Id, why: &str| Err(ConfigError(format!("reader {reader}: {why}")));
        for (i, reader) in self.readers.iter().enumerate() {
            if self.readers[..i].iter().any(|r| r.name == reader.name) {
                return bad(&reader.name, "named twice");
            }
            if check_pairing(&reader.identity, &reader.trust).is_err() {
                return bad(
                    &reader.name,
                    "a chain id is read from an rpc pool, and a genesis hash by a light client",
                );
            }
            match (&reader.trust, &reader.light_client) {
                (TrustRule::LightClient(_), None) => {
                    return bad(&reader.name, "a light client needs its rule");
                }
                (TrustRule::RpcPool { .. }, Some(_)) => {
                    return bad(&reader.name, "an rpc pool's reader reaches its pool alone");
                }
                (_, Some(rule)) if rule.max_connections == 0 || rule.ports.is_empty() => {
                    return bad(
                        &reader.name,
                        "a light client reaches at least one port and one connection",
                    );
                }
                _ => {}
            }
            let l = &reader.limits;
            if l.hang_after_ms == 0
                || l.start_within_ms == 0
                || l.pending == 0
                || l.egress_give_up_after_ms == 0
                || l.max_frame == 0
                || l.calls_per_question == 0
            {
                return bad(&reader.name, "every bound is above zero");
            }
        }
        Ok(())
    }
}
