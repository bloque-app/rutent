//! A suite `ModuleSpec` as the stand-ins read it, and writing it into a compiled stand-in's spec slot.
//!
//! The text form is the one `guests/rust/src/lib.rs` documents. Every word is one the suite writes without spaces:
//! provider names, versions, operation and secret names, state keys and host names.

use rutent_conformance::boundary::{ModuleSpec, Output, Step};
use rutent_contracts::module_interface::{OperationKind, PartName, Scheme};

/// The slot's marker, which opens the 4096-byte slot in each compiled stand-in.
pub const MAGIC: &[u8; 16] = b"RUTENT-SPEC-SLOT";
/// The slot's size, marker and length included.
pub const SLOT_SIZE: usize = 4096;
const TEXT: usize = 20;

/// A provider-role operation kind as the contract writes it.
pub fn kind_name(kind: OperationKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The spec's text, for a stand-in of `category`.
pub fn encode(spec: &ModuleSpec, category: &str) -> String {
    let d = &spec.declaration;
    let operation = d
        .outside
        .operations
        .first()
        .map(|o| o.name.as_str().to_owned())
        .unwrap_or_else(|| "none".to_owned());
    let mut text = format!(
        "k {category}\np {}\nv {}\ns {}\no {operation}\n",
        d.provider.as_str(),
        d.version.as_str(),
        u8::from(spec.starts)
    );
    let output = match spec.output {
        Output::Honest => "honest".to_owned(),
        Output::PaddedTo(n) => format!("pad {n}"),
        Output::Unparseable => "unparseable".to_owned(),
        Output::AnotherKind => "kind".to_owned(),
        Output::AnotherTransaction => "tx".to_owned(),
        Output::UnknownField => "field".to_owned(),
    };
    text.push_str(&format!("out {output}\n"));
    let part = |p: &PartName| match p {
        PartName::Sandboxed => "s",
        PartName::Outside => "o",
    };
    for step in &spec.steps {
        let line = match step {
            Step::LookUpSecret {
                part: p,
                operation,
                name,
            } => format!("step L {} {operation} {name}", part(p)),
            Step::ReadState { part: p, key } => format!("step R {} {key}", part(p)),
            Step::Call {
                operation,
                endpoint,
            } => {
                let scheme = match endpoint.scheme {
                    Scheme::Https => "https",
                    Scheme::Wss => "wss",
                };
                format!(
                    "step C {operation} {scheme}://{}:{}",
                    endpoint.host, endpoint.port
                )
            }
            Step::CallModule { callee, kind } => format!("step M {callee} {}", kind_name(*kind)),
        };
        text.push_str(&line);
        text.push('\n');
    }
    text
}

/// Why a stand-in couldn't take a spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotError {
    /// The compiled stand-in holds no slot, or holds it twice.
    NoSlot,
    /// The spec doesn't fit the slot.
    TooLong(usize),
}

/// The compiled stand-in `template` with `text` written into its slot.
pub fn patch(template: &[u8], text: &str) -> Result<Vec<u8>, SlotError> {
    let mut at = template
        .windows(MAGIC.len())
        .enumerate()
        .filter(|(_, w)| w == MAGIC);
    let (Some((at, _)), None) = (at.next(), at.next()) else {
        return Err(SlotError::NoSlot);
    };
    if template.len() < at + SLOT_SIZE {
        return Err(SlotError::NoSlot);
    }
    let bytes = text.as_bytes();
    if bytes.len() > SLOT_SIZE - TEXT {
        return Err(SlotError::TooLong(bytes.len()));
    }
    let mut blob = template.to_vec();
    let length = u32::try_from(bytes.len()).map_err(|_| SlotError::TooLong(bytes.len()))?;
    blob[at + 16..at + TEXT].copy_from_slice(&length.to_le_bytes());
    blob[at + TEXT..at + TEXT + bytes.len()].copy_from_slice(bytes);
    Ok(blob)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template() -> Vec<u8> {
        let mut t = b"PVM\0head".to_vec();
        t.extend_from_slice(MAGIC);
        t.extend(std::iter::repeat_n(b'.', SLOT_SIZE - MAGIC.len()));
        t.extend_from_slice(b"tail");
        t
    }

    // verifies: REQ-MDL-1, REQ-MDL-15
    #[test]
    fn a_spec_is_written_into_the_slot_with_its_length_and_nothing_else_changes() {
        let t = template();
        let blob = patch(&t, "k reserve\n").expect("it fits");
        assert_eq!(blob.len(), t.len());
        assert_eq!(&blob[24..28], &10u32.to_le_bytes());
        assert_eq!(&blob[28..38], b"k reserve\n");
        assert_eq!(&blob[38..], &t[38..]);
    }

    // verifies: REQ-MDL-15
    #[test]
    fn a_spec_longer_than_the_slot_is_refused() {
        let long = "x".repeat(SLOT_SIZE);
        assert_eq!(
            patch(&template(), &long),
            Err(SlotError::TooLong(SLOT_SIZE))
        );
        assert_eq!(patch(b"no slot here", "k"), Err(SlotError::NoSlot));
    }
}
