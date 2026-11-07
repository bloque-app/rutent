//! VOS behind rutent's host contract (`T-007-03`, `EXP-1`): a [`Host`] whose durable store is a VOS node, run as
//! its own `vos node run` process, holding the journal actor (`../journal`).
//!
//! **Not mergeable until the owner rules on `DEC-6`.** This crate links VOS's AGPL-3.0-or-later crates
//! (`vos-node`, `vos-wire`, `vos-sdk`). It is kept out of rutent's workspace and lockfile; only the measurement
//! notes it produces may merge.
//!
//! # Where each part runs
//!
//! - **The state machine runs here, in the adapter**, not in VOS. rutent's state-machine contract needs `std` and
//!   `serde`; a VOS guest is `no_std` on RV64EM with no atomics. So [`Host::submit`] reads the order's latest
//!   state from VOS, calls [`StateMachine::open`] or [`StateMachine::transition`], and sends the result.
//! - **Every rule that needs state is VOS's**: the journal actor checks halted, the record id (byte-equal
//!   canonical JSON returns the record; anything else is refused and the refusal written), the order's next
//!   sequence number (a compare-and-set) and an open import, and writes, all in one handler step, which VOS
//!   commits whole or not at all. The adapter keeps nothing authoritative in memory: a kill drops it.
//! - **Reads** are linearizable queries of the actor.
//!
//! # Faults
//!
//! A kill is a `SIGKILL` of the node's process; reopening starts `vos node run` again on the same data
//! directory. The harness also implements [`FaultSeam`], reaching each crash point by a kill timed to it:
//!
//! - [`CrashPoint::AfterReceived`]: the adapter holds the input; nothing is computed or sent. The node is killed.
//! - [`CrashPoint::AfterComputed`]: the adapter has computed the transition. VOS's own fault file holds every
//!   commit (`hold_commits`, a `faults` build), the append is sent, and the node is killed 1 s later; held, VOS
//!   cannot have made it durable. That VOS had received it by then is inferred from a control pass that sends it
//!   without the hold, in which the record is durable before the kill.
//! - [`CrashPoint::AfterDurable`]: VOS has answered the append as run, so it is committed. The node is killed
//!   before the adapter acknowledges it.
//!
//! An import stopped part-way is a kill between two entries' calls.
//!
//! # The flag
//!
//! Everything here is behind the `vos` feature, the flag `PLAN.md` `T-007-03` asks for. Without it the library
//! is empty and nothing of VOS is compiled or linked.

#[cfg(feature = "vos")]
mod vos;
#[cfg(feature = "vos")]
pub use vos::*;
