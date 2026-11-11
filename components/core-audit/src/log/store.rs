//! The log's own store (ADR-0005): its records, in order, apart from the state machine's store and the provider
//! system's. A record is one entry's RFC 8785 canonical JSON.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Why the store took or gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// It can't be read or written now; the log may try again.
    Unavailable(String),
    /// A record before the last isn't whole, so the history can't be read back: the log refuses to open on it.
    Corrupt(usize),
}

/// An append-only store of records. A record is durable once [`Store::append`] answers `Ok`. An `Err` says nothing
/// either way: the record may or may not have been written, so the log reads the store back before it appends again.
pub trait Store {
    /// Appends `record` durably.
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError>;

    /// Every record, oldest first.
    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError>;
}

/// A store in memory: no durability across processes, for a run that keeps none.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    records: Vec<Vec<u8>>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for MemoryStore {
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        self.records.push(record.to_vec());
        Ok(())
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError> {
        Ok(self.records.clone())
    }
}

/// A store in one append-only file: each record on its own line, synced to the disk before an append answers.
/// RFC 8785 escapes every control character inside a string, so a record holds no line break of its own. A last
/// line cut short by a crash was never answered as appended, and is dropped when the file is read back.
#[derive(Debug)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// The store in the file at `path`, created empty where there is none.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(unavailable)?;
        Ok(FileStore { path })
    }
}

fn unavailable(e: std::io::Error) -> StoreError {
    StoreError::Unavailable(e.to_string())
}

impl Store for FileStore {
    fn append(&mut self, record: &[u8]) -> Result<(), StoreError> {
        if record.contains(&b'\n') {
            return Err(StoreError::Unavailable(
                "a record holds a line break".into(),
            ));
        }
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(unavailable)?;
        let mut line = Vec::with_capacity(record.len() + 1);
        line.extend_from_slice(record);
        line.push(b'\n');
        file.write_all(&line).map_err(unavailable)?;
        file.sync_data().map_err(unavailable)
    }

    fn records(&mut self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut bytes = Vec::new();
        File::open(&self.path)
            .and_then(|mut f| f.read_to_end(&mut bytes))
            .map_err(unavailable)?;
        let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        if whole < bytes.len() {
            // A torn last line: cut it off, so the next append starts on a line of its own.
            let file = OpenOptions::new()
                .write(true)
                .open(&self.path)
                .map_err(unavailable)?;
            file.set_len(whole as u64).map_err(unavailable)?;
            file.sync_data().map_err(unavailable)?;
        }
        let records: Vec<Vec<u8>> = bytes[..whole]
            .split(|b| *b == b'\n')
            .map(<[u8]>::to_vec)
            .collect();
        // `split` leaves an empty piece after the last line break.
        let records = &records[..records.len().saturating_sub(1)];
        if let Some(i) = records.iter().position(Vec::is_empty) {
            return Err(StoreError::Corrupt(i));
        }
        Ok(records.to_vec())
    }
}
