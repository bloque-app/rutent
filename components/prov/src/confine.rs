//! The provider's jail, for the reader host (`T-015-04`, `DEC-28`, `REQ-MDL-7`): the one path of `CMP-PROV`'s module
//! runtime that `rutent-reader-host` may name (`tools/boundary-check`'s admitted paths).
//!
//! A reader's process gets the confinement a provider's process gets: [`isolated`] starts it in new user, network,
//! IPC, UTS, mount and PID namespaces with its environment cleared, every inherited descriptor above the standard
//! three closed and its limits set, and the reader calls [`confine`] as [`Role::Provider`] before it reads any input:
//! `no_new_privs`, the empty read-only root, every capability dropped, Landlock with no rule, the `seccomp`
//! allow-list and its address space bounded ([`PROVIDER_ADDRESS_SPACE`]). These are re-exports of
//! [`crate::module::sandbox`]'s items, nothing more: the rest of the module runtime stays out of the host's reach.

pub use crate::module::sandbox::{Limits, PROVIDER_ADDRESS_SPACE, Role, confine, isolated};
