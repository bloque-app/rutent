//! The CPU a test runs on, as `/proc/cpuinfo` names it, and the file of machine-code digests the re-derivation reads and
//! writes (`B-11`, `G-04`, ADR-0014).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;

/// The value of the first `key : value` line of a `/proc/cpuinfo` text.
fn field(cpuinfo: &str, key: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

/// The vendor a `/proc/cpuinfo` text names (`vendor_id`, x86), if it names one.
pub fn vendor(cpuinfo: &str) -> Option<String> {
    field(cpuinfo, "vendor_id").filter(|v| !v.is_empty())
}

/// The model a `/proc/cpuinfo` text names (`model name`, x86).
pub fn model(cpuinfo: &str) -> Option<String> {
    field(cpuinfo, "model name")
}

/// Whether a vendor id is AMD's: its own, or the licensed Hygon parts that carry the same design.
pub fn is_amd(vendor: &str) -> bool {
    matches!(vendor, "AuthenticAMD" | "HygonGenuine")
}

/// Whether a re-derivation on `this` vendor, against a reference made on `reference`, is one on another vendor than
/// AMD's: this machine's can't be AMD's, and the reference's must be known and not this machine's, so the two runners
/// can't have both been the same non-AMD vendor (which would compare nothing across vendors).
pub fn crosses_vendors(this: &str, reference: &str) -> Result<(), String> {
    if is_amd(this) {
        return Err(format!("this machine's CPU is {this}: AMD's"));
    }
    if reference == "unknown" || reference.is_empty() {
        return Err("the reference records no CPU vendor".to_owned());
    }
    if reference == this {
        return Err(format!(
            "the reference was made on {reference} too: the same vendor, so nothing is compared across vendors"
        ));
    }
    Ok(())
}

/// This machine's `/proc/cpuinfo`, if it has one.
pub fn cpuinfo() -> Option<String> {
    std::fs::read_to_string("/proc/cpuinfo").ok()
}

/// The CPU a digest file records and the digests it holds, one per part: `{language}-{category}-{part}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Digests {
    /// The `vendor_id`, or `unknown` where the machine has no `/proc/cpuinfo`.
    pub vendor: String,
    /// The `model name`, or `unknown`.
    pub model: String,
    /// The platform version that made the machine code (`build.platform_version`).
    pub platform: String,
    /// Each part's name and the SHA-256 of its machine code.
    pub parts: BTreeMap<String, String>,
}

impl Digests {
    /// The file's text.
    pub fn render(&self) -> String {
        let mut text = format!(
            "# vendor_id: {}\n# model name: {}\n# platform: {}\n",
            self.vendor, self.model, self.platform
        );
        for (part, digest) in &self.parts {
            text.push_str(&format!("{part} {digest}\n"));
        }
        text
    }

    /// Reads the file's text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let header = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("# {key}: ")))
                .map(str::to_owned)
                .ok_or(format!("no `{key}` line"))
        };
        let mut parts = BTreeMap::new();
        for line in text
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (part, digest) = line
                .split_once(' ')
                .ok_or(format!("not a digest line: {line}"))?;
            parts.insert(part.to_owned(), digest.to_owned());
        }
        Ok(Self {
            vendor: header("vendor_id")?,
            model: header("model name")?,
            platform: header("platform")?,
            parts,
        })
    }

    /// Reads a digest file.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// The parts whose digests differ from `reference`'s, or that one of the two lacks.
    pub fn differences(&self, reference: &Self) -> Vec<String> {
        let mut found = Vec::new();
        if self.platform != reference.platform {
            found.push(format!(
                "platform version {} against {}",
                self.platform, reference.platform
            ));
        }
        for name in self.parts.keys().chain(reference.parts.keys()) {
            if self.parts.get(name) != reference.parts.get(name) && !found.contains(name) {
                found.push(name.clone());
            }
        }
        found
    }
}
