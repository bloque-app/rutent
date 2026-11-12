//! What survives a crash of the assembled Core: the log's store and the evidence store's shelf (`NFR-5`).
//!
//! A [`Disk`] holds both. The log and the evidence store each take a handle on it ([`LogDisk`], [`ShelfDisk`]), and
//! when the Core holding them is dropped mid-step (the crash), the disk keeps every record and every statement that
//! was durable, and nothing else: a record is durable once the log's store took it, as on a real disk. A Core
//! opened again on the same disk reads both back, as the log and the store do after a restart (`T-022-01`,
//! `T-022-03`).
//!
//! A [`Crash`] planted on the disk stops the process right after the store took a record of a given kind: the
//! record is durable, and its writer never hears so. That is the crash between a record and its effect (`INV-7`):
//! the effect waits on the log's answer, which never comes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rutent_contracts::module_interface::Digest;
use rutent_core_audit::log::{Store, StoreError};
use rutent_core_audit::receipts::store::{MemoryShelf, Shelf, ShelfContents, ShelfError, Shelved};

/// Where the crash falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crash {
    /// Right after the log's store takes the `nth` record (from 0) whose `call` is `call`, before the log answers
    /// the append.
    AfterRecord {
        /// The verified entry's `call`, such as `effect`.
        call: &'static str,
        /// Which one, counted from 0.
        nth: usize,
    },
}

/// The message a planted crash panics with, so a test tells it from any other panic.
pub const CRASHED: &str = "the assembled core crashed (a planted fault)";

/// The log's records and the evidence store's statements, as a disk holds them across a crash.
#[derive(Clone, Default)]
pub struct Disk {
    records: Rc<RefCell<Vec<Vec<u8>>>>,
    shelf: Rc<RefCell<MemoryShelf>>,
    crash: Rc<Cell<Option<Crash>>>,
    seen: Rc<RefCell<Vec<(String, usize)>>>,
}

impl std::fmt::Debug for Disk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Disk")
            .field("records", &self.records.borrow().len())
            .finish_non_exhaustive()
    }
}

impl Disk {
    /// An empty disk.
    pub fn new() -> Self {
        Self::default()
    }

    /// Plants `crash`: the process stops there, once.
    pub fn plant(&self, crash: Crash) {
        self.crash.set(Some(crash));
    }

    /// Every record the log's store holds, oldest first.
    pub fn records(&self) -> Vec<Vec<u8>> {
        self.records.borrow().clone()
    }

    /// The log's store on this disk.
    pub(crate) fn log(&self) -> LogDisk {
        LogDisk(self.clone())
    }

    /// The evidence store's shelf on this disk.
    pub(crate) fn shelf(&self) -> ShelfDisk {
        ShelfDisk(self.clone())
    }

    fn took(&self, record: &[u8]) {
        let Some(Crash::AfterRecord { call, nth }) = self.crash.get() else {
            return;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(record) else {
            return;
        };
        if value["what"]["call"].as_str() != Some(call) {
            return;
        }
        let mut seen = self.seen.borrow_mut();
        let count = seen.iter().find(|(c, _)| c == call).map_or(0, |(_, n)| *n);
        seen.retain(|(c, _)| c != call);
        seen.push((call.to_owned(), count + 1));
        if count == nth {
            self.crash.set(None);
            drop(seen);
            panic!("{CRASHED}");
        }
    }
}

/// The log's store, on a [`Disk`].
pub struct LogDisk(Disk);

impl Store for LogDisk {
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        self.0.records.borrow_mut().push(record.to_vec());
        self.0.took(record);
        Ok(())
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError> {
        Ok(self.0.records.borrow().clone())
    }
}

/// The evidence store's shelf, on a [`Disk`].
pub struct ShelfDisk(Disk);

impl Shelf for ShelfDisk {
    fn put(&mut self, shelved: &Shelved) -> Result<(), ShelfError> {
        self.0.shelf.borrow_mut().put(shelved)
    }

    fn erase(&mut self, commitment: &Digest) -> Result<(), ShelfError> {
        self.0.shelf.borrow_mut().erase(commitment)
    }

    fn load(&mut self) -> Result<ShelfContents, ShelfError> {
        self.0.shelf.borrow_mut().load()
    }
}
