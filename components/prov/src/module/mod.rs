//! Module admission, runtime and lifecycle (`F-013`), on Wasm components and Wasmtime 48.0.8 (`DEC-27`), from
//! `T-008-02`'s binding (`REQ-MDL-15`, `REQ-MDL-18`, `AC-20`, `REQ-MDL-13`).
//!
//! A module is two components, one per part, written against the WIT world in `wit/binding.wit`, outside the frozen
//! contract (`DEC-4`). Its release assertion names the format [`engine::FORMAT`], `wasm-component`. Nothing in
//! this module imports a part's WASI interface: the component model's own imports are the only route a part has.
//!
//! - [`platform`]: the platform. It admits releases, runs one OS process per provider, and checks everything a
//!   module asks for against its running declaration, attributing each request to the provider whose process it
//!   came from (`DEC-9`).
//! - [`engine`]: the one Wasmtime configuration the compiler and every provider process share: an explicit compile
//!   target, only the WebAssembly features the modules need, fuel for CPU, and the memory limits per part.
//! - [`children`]: the platform's side of its children, the admitting compiler (one fresh process per admission)
//!   and each provider's process.
//! - [`host`]: the world's imports on the process side, each one a request to the platform.
//! - [`limits`]: the bounds on what a module's parts may ask of the platform, per operation and per provider, and
//!   the pure checks the platform makes against them (`REQ-MDL-11`).
//! - [`sandbox`]: the operating system's jail for a child, and the frames the platform and a child exchange.
//! - [`wire`]: the messages in those frames.
//! - [`types`]: what the platform answers with.
//!
//! The two executables, `rutent-wasm-compiler` and `rutent-wasm-provider`, are this package's binaries. The
//! guest packages the tests build their modules from are in `module-guests/`.
//!
//! This module admits modules, so a change to it is on a `DEC-20` path.

pub mod children;
pub mod engine;
pub mod host;
pub mod limits;
pub mod platform;
pub mod sandbox;
pub mod types;
pub mod wire;
