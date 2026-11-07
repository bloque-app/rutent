//! rutent's stand-in for the `dirs` crate, for EXP-2's PolkaVM binding (`T-008-03`).
//!
//! `polkavm-linker` 0.38.0 calls `dirs::cache_dir()` in one place, `target_json_path`, to choose where it writes
//! its target JSON. Upstream `dirs` 5.0.1 brings `dirs-sys` 0.4.1 and, through it, `option-ext` 0.2.0, which is
//! MPL-2.0. This stub replaces it through the binding workspace's `[patch.crates-io]`: it has no dependency, never
//! reads the environment or the platform's directories, and returns only a directory the binding set with
//! [`set_cache_dir`], or `None`.

use std::path::PathBuf;
use std::sync::OnceLock;

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets the directory [`cache_dir`] returns, once. False if one was already set.
pub fn set_cache_dir(path: PathBuf) -> bool {
    CACHE_DIR.set(path).is_ok()
}

/// The directory set with [`set_cache_dir`], or `None` if none was.
pub fn cache_dir() -> Option<PathBuf> {
    CACHE_DIR.get().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-MOD-5
    #[test]
    fn the_cache_dir_is_only_the_one_set_explicitly() {
        assert_eq!(cache_dir(), None);
        assert!(set_cache_dir(PathBuf::from("/explicit")));
        assert!(!set_cache_dir(PathBuf::from("/again")));
        assert_eq!(cache_dir(), Some(PathBuf::from("/explicit")));
    }
}
