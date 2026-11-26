//! What the demo's two binaries share: `rutent-demo`, the narrated run in a terminal, and `rutent-demo-api`, the same
//! network served over HTTP with a page that draws it. Each includes this folder as a module of its own
//! (`#[path = …]`), so nothing here is part of `rutent-scenarios`'s library: `rutent-demo` is a binary of
//! `rutent-scenarios`, and `rutent-demo-api` a workspace of its own in `conformance/scenarios/demo-api/`.
//!
//! - [`network`]: the simulated providers and the graph they make.
//! - [`orders`]: the customer's requests, the operator's staff and the evidence a decision cites.
//! - [`show`]: amounts in their units, routes, and the log's entries as a timeline.
//! - [`audit`]: the folder an outside checker is handed, and `audit-check` run over it.
//! - [`relaunch`]: running again from cargo's copy under `deps/`, and the output folder.

pub mod audit;
pub mod network;
pub mod orders;
pub mod relaunch;
pub mod show;
