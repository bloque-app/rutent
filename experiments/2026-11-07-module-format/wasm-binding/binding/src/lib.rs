//! A Wasm-component binding of rutent's module-boundary suite (`EXP-2`, `T-008-02`, `REQ-MDL-15`, `REQ-MDL-18`,
//! `AC-20`, `REQ-MDL-13`), on Wasmtime 48.0.8.

pub mod children;
pub mod engine;
pub mod host;
pub mod platform;
pub mod program;
pub mod wire;
