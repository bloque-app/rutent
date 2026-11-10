//! The credential proxy's tests (`T-014-02`). They are the crate's own, not integration tests, because a
//! [`Caller`](super::Caller) is built only inside the crate: no code outside it can claim to be any provider's
//! process. The HTTPS transport's tests, which need no caller, are `tests/proxy_https.rs`.

#![allow(dead_code)]

mod calls;
mod properties;
mod shared;
mod support;
mod world;
