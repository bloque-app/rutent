//! No writer of a stated entry in Core sets `relayed_by` on its own (`T-020-28`; `REQ-AUD-2`): the one helper,
//! `src/relay.rs`, does, so a site that builds a stated entry can't write that nothing relayed it.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

// verifies: REQ-AUD-2
#[test]
fn every_stated_entry_core_builds_takes_its_relay_from_the_one_helper() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    let mut builders = 0;
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        if file.ends_with("relay.rs") {
            continue;
        }
        assert!(
            !code.contains("relayed_by: None"),
            "{} writes `relayed_by: None` itself",
            file.display()
        );
        let built = code.matches("StatedEntry {").count();
        if built > 0 {
            builders += built;
            // Each builder takes its `relayed_by` by shorthand from a binding the helper makes, and the file has
            // no other binding or field of that name.
            let shorthand = code.matches("relayed_by,").count();
            let bound = code
                .lines()
                .filter(|l| l.trim_start().starts_with("let relayed_by"))
                .collect::<Vec<_>>();
            assert_eq!(
                shorthand,
                built,
                "{}: each entry takes its relay",
                file.display()
            );
            assert_eq!(
                bound.len(),
                built,
                "{}: one binding per entry",
                file.display()
            );
            assert!(
                bound.iter().all(|l| l.contains("relay::relayed_by(")),
                "{}: a relay not taken from the helper",
                file.display()
            );
        }
        assert!(
            !code.contains("relayed_by:"),
            "{} sets a `relayed_by` field itself",
            file.display()
        );
    }
    assert!(builders >= 5, "the five writers were found: {builders}");
}
