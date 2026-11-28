//! A stand-in for the evidence store's own read, [`KeptStatements`], until `T-022-03` builds the evidence store in
//! `receipts`. It keeps statements in memory, each under the salted commitment the contract defines, SHA-256 of the
//! salt followed by the bytes, and answers the log by that commitment. It is not the evidence store: it holds no
//! restatement index, makes no salt of its own and answers no party's read (`Evidence`), all of which are
//! `T-022-03`'s. The log holds whatever [`KeptStatements`] the assembly gives it, so `T-022-03`'s store replaces this
//! one without a change to the log.

use std::cell::RefCell;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{AppendFailure, KeptStatements};
use rutent_contracts::core_internal::{Encoded, KeptStatement, Salt};
use rutent_contracts::module_interface::Digest;

use super::digest;
use super::merkle::sha256;

const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `bytes` in base64url without padding, the contract's `Encoded` and `Salt` text.
pub fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(BASE64URL[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// The bytes base64url text without padding writes, or `None` where it isn't such text.
pub fn from_base64url(text: &str) -> Option<Vec<u8>> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes() {
        let v = BASE64URL.iter().position(|a| *a == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Statements kept in memory, shared by every clone: what one clone keeps, the log holding another reads.
#[derive(Debug, Clone, Default)]
pub struct KeptInMemory(Rc<RefCell<Vec<KeptStatement>>>);

impl KeptInMemory {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `received` beside `salt`, and answers with its commitment: SHA-256 of the salt followed by the bytes.
    /// The salt is the caller's, since this stand-in draws none. `received` holds at least one byte, as every
    /// statement does.
    pub fn keep(&self, received: &[u8], salt: [u8; 32]) -> Digest {
        let mut salted = salt.to_vec();
        salted.extend_from_slice(received);
        let commitment = digest(&sha256(&salted));
        let statement = KeptStatement {
            commitment: commitment.clone(),
            salt: Salt::new(base64url(&salt)).expect("32 bytes of base64url are a salt"),
            bytes: Encoded::new(base64url(received)).expect("base64url text is encoded bytes"),
        };
        self.0.borrow_mut().push(statement);
        commitment
    }

    /// Every statement kept, oldest first.
    pub fn statements(&self) -> Vec<KeptStatement> {
        self.0.borrow().clone()
    }
}

impl KeptStatements for KeptInMemory {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(self
            .0
            .borrow()
            .iter()
            .find(|k| &k.commitment == commitment)
            .cloned())
    }
}
