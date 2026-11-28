//! The reader host (`T-015-04`, `DEC-28`, ADR-0007 as amended): `CMP-EVIDENCE`'s standalone part.
//!
//! Each public network's reader is a reader service, a separate process of the operator's own, never linked into
//! Core. The reader host runs them: one process, deployable as one ECS task, that serves several readers, each its own
//! child process. Core asks it over mTLS under the operator's CA, in the `reader` contract
//! ([`rutent_contracts::reader`]). This crate is a `DEC-20` path: the answers' signing, the link and the egress.
//!
//! # The six parts
//!
//! 1. **mTLS** ([`link`]). The host serves the `reader` contract over TLS on `rustls` with its `ring` provider, never
//!    `aws-lc-rs`. It takes a connection only from a client certificate the operator's CA issued for Core, by the
//!    subject name its configuration names, and presents the server certificate the operator's CA issued under the
//!    subject name the listing names. Core holds that certificate to the CA and the name, never to a pinned leaf, so
//!    the leaf may rotate with no listing change. It answers each question under the request id and the deadline Core
//!    sent, and never draws either.
//! 2. **One process per reader** ([`Host`], [`config::ReaderLimits`]). Each reader is a child process the host starts
//!    from its configuration. A reader's crash, hang or limit hit stops that reader alone: the host answers its
//!    questions as unreadable and restarts it, and the other readers answer on. The host bounds each reader's memory
//!    (the jail's address space, which the host's launcher sets and its check reads, `T-015-05`), the time it may take over a question (read on every turn of its worker, and every
//!    egress call given up with it), the questions waiting for it, the frames waiting each way between it and the host
//!    (so a reader that stops reading, or talks faster than the host reads, is stopped, never waited on), and its
//!    egress calls per question. An asker waits no longer than the reader's share of that.
//! 3. **Signing outside the reader** ([`keys`]). The host signs each answer with the reader's statement key, which it
//!    resolves by name when it signs, through `T-014-01`'s resolver as the platform does ([`rutent_prov::keys`]), and
//!    never hands to the reader's process. It signs only an answer that names the question and the request it was
//!    asked and holds to its shape; anything else is signed as unreadable.
//! 4. **Egress, enforced by the host** ([`egress`], [`jail`]). A reader's process has no network route of its own:
//!    it runs in the jail `CMP-PROV` runs a provider's process in ([`rutent_prov::confine`]): on Linux new user,
//!    network, IPC, UTS, mount and PID namespaces from outside, and from inside the empty read-only root, every
//!    capability dropped, Landlock and the `seccomp` allow-list, which the host holds it to before it answers, as far
//!    as the kernel reports them to it ([`jail::check_confined`]). Its one
//!    channel to the host is its standard input and output. Every request or connection it
//!    makes is the host's to make, through `CMP-PROV`'s egress layer, only to a destination the reader declares, and
//!    each is recorded with destination, reader, version and request ([`records`]).
//! 5. **The trust rules.** For `rpc_pool`, the host asks the pool's endpoints and hands the reader an answer only
//!    where at least the quorum of them agree; disagreement or too few answers is unreadable. The quorum binds which
//!    endpoints agreed on what the host was asked to fetch, not what the reader then says they said: a reader's
//!    content is the operator's to trust (`DEC-28`), as its program is. For `light_client`, the
//!    reader checks the network's genesis and the metadata it relies on before it answers anything, and until it has
//!    reported them, and the host has held them to its configuration, every question is unreadable. So is any question
//!    to a reader whose start-up checks have not passed, whatever its rule.
//! 6. **Restarts.** A restarted host re-reads its configuration ([`HostConfig::read`]), and each reader is refusing
//!    until its checks pass again.
//!
//! # The clocks
//!
//! The deadline of an answer is Core's, carried in the question, and checked on Core's clock when the answer reaches
//! it, never on the host's. The host hands the question to the reader as Core sent it, and reads the deadline against
//! its own wall clock for one thing alone: a question already past it, when it arrives or when its turn comes, is
//! dropped and answered with nothing, never queued or handed over (`T-015-05`). Its own bounds, how long a reader may take over a question before the host takes it
//! as hung and how long an egress call may take, are [`config::ReaderLimits`]'s, on the operating system's monotonic
//! clock, which is also the clock the egress layer's resolver and connector read by default: none of them is the
//! answer's deadline.
//!
//! The wire between the host and a reader's process is [`wire`]'s.

pub mod config;
pub mod egress;
mod host;
pub mod jail;
pub mod keys;
pub mod link;
pub mod records;
mod time;
pub mod wire;
mod worker;

pub use config::{HostConfig, ReaderConfig};
pub use host::{Host, ReaderState, Services};
