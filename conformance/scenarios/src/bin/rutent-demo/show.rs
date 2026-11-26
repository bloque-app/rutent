//! What the demo prints: headings, amounts in their units, routes, and the log's entries as a timeline.

use std::fmt::Write as _;

use rutent_contracts::core_internal::{Entry, Receipt};
use rutent_contracts::router_api::DecisionRecord;
use serde_json::Value;

/// Everything the demo printed, kept to be written beside the log as `timeline.txt`.
#[derive(Default)]
pub struct Transcript {
    text: String,
    /// The log's length when its entries were last shown.
    shown: usize,
}

impl Transcript {
    /// Prints `line` and keeps it.
    pub fn line(&mut self, line: impl AsRef<str>) {
        println!("{}", line.as_ref());
        self.text.push_str(line.as_ref());
        self.text.push('\n');
    }

    /// A blank line.
    pub fn gap(&mut self) {
        self.line("");
    }

    /// An act's heading.
    pub fn act(&mut self, number: &str, title: &str) {
        self.gap();
        let rule = "═".repeat(100);
        self.line(&rule);
        self.line(format!("  {number}  {title}"));
        self.line(&rule);
    }

    /// A step inside an act.
    pub fn step(&mut self, text: impl AsRef<str>) {
        self.line(format!("  ▸ {}", text.as_ref()));
    }

    /// A detail under a step.
    pub fn detail(&mut self, text: impl AsRef<str>) {
        self.line(format!("      {}", text.as_ref()));
    }

    /// What the presenter says, set apart.
    pub fn say(&mut self, text: impl AsRef<str>) {
        self.line(format!("  » {}", text.as_ref()));
    }

    /// The entries the log took since the last call, one line each: who stated or did what, and its receipt.
    pub fn entries_since(&mut self, entries: &[(Receipt, Entry)]) {
        let new = &entries[self.shown.min(entries.len())..];
        self.line(format!(
            "      ┌─ the log took {} entries (#{}..#{}) ─────────────────────────────────────────────",
            new.len(),
            self.shown,
            entries.len().saturating_sub(1)
        ));
        for (receipt, entry) in new {
            let line = entry_line(receipt, entry);
            self.line(format!("      │ {line}"));
        }
        self.line("      └──────────────────────────────────────────────────────────────────────────────────");
        self.shown = entries.len();
    }

    /// Everything printed so far.
    pub fn text(&self) -> &str {
        &self.text
    }
}

thread_local! {
    static PUBLIC: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Names the inner nodes on the operator's list of public nodes; every other inner node shows as private.
pub fn public_nodes(nodes: &[&str]) {
    PUBLIC.with(|p| *p.borrow_mut() = nodes.iter().map(|n| (*n).to_owned()).collect());
}

/// An amount in an asset's smallest unit, shown in the asset with two decimals: `1000000` of COP is `10,000.00 COP`.
pub fn money(units: &str, asset: &str) -> String {
    let Ok(n) = units.parse::<u128>() else {
        return format!("{units} {asset}");
    };
    let whole = (n / 100).to_string();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{grouped}.{:02} {asset}", n % 100)
}

/// A digest or a key cut to its first characters, enough to tell two apart on a slide.
pub fn short(text: &str) -> String {
    let tail = text.rsplit(':').next().unwrap_or(text);
    format!("{}…", &tail[..tail.len().min(12)])
}

/// A receipt as the customer would quote it: the entry's place in the log and its digest.
pub fn receipt(receipt: &Receipt) -> String {
    let value = serde_json::to_value(receipt).unwrap_or(Value::Null);
    format!(
        "receipt #{} sha256:{}",
        value["index"],
        short(value["entry"]["value"].as_str().unwrap_or("?"))
    )
}

/// The name an endpoint's node goes by, with its asset: an outer node's network, or an inner node's identifier.
pub fn endpoint(value: &Value) -> String {
    let node = &value["node"];
    let name = node["network"]
        .as_str()
        .map(|n| format!("{n} (outer)"))
        .or_else(|| {
            node.as_str().map(|n| {
                let public = PUBLIC.with(|p| p.borrow().iter().any(|x| x == n));
                format!("{n} ({})", if public { "public" } else { "private" })
            })
        })
        .unwrap_or_else(|| node.to_string());
    format!("{name} in {}", value["asset"]["id"].as_str().unwrap_or("?"))
}

/// Each operation of the route a decision record chose, one line each: the provider, the kind, where it starts and
/// where it ends.
pub fn route(record: &DecisionRecord) -> Vec<String> {
    let Some(chosen) = record.chosen() else {
        return vec!["no route chosen".to_owned()];
    };
    let mut lines = Vec::new();
    for (p, part) in chosen.parts.iter().enumerate() {
        for (n, operation) in part.operations.iter().enumerate() {
            let op = serde_json::to_value(operation).unwrap_or(Value::Null);
            lines.push(format!(
                "{}.{} {:<22} {:<11} {}  →  {}",
                p + 1,
                n + 1,
                operation.provider.as_str(),
                op["kind"].as_str().unwrap_or("?"),
                endpoint(&op["from"]),
                endpoint(&op["to"]),
            ));
        }
    }
    lines
}

/// The asset each operation of a decision record's chosen route gives out, in order.
pub fn assets_out(record: &DecisionRecord) -> Vec<String> {
    record
        .chosen()
        .map(|c| {
            c.parts
                .iter()
                .flat_map(|p| p.operations.iter())
                .map(|o| {
                    serde_json::to_value(o)
                        .ok()
                        .and_then(|v| v["to"]["asset"]["id"].as_str().map(str::to_owned))
                        .unwrap_or_else(|| "?".to_owned())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The providers a decision record's chosen route runs through, in order.
pub fn providers(record: &DecisionRecord) -> Vec<String> {
    record
        .chosen()
        .map(|c| {
            c.parts
                .iter()
                .flat_map(|p| p.operations.iter())
                .map(|o| o.provider.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// One entry of the log on one line: its place, whether Core verified it or a party stated it, what it is, who, and
/// the order and provider it names.
fn entry_line(receipt: &Receipt, entry: &Entry) -> String {
    let value = serde_json::to_value(entry).unwrap_or(Value::Null);
    let index = serde_json::to_value(receipt).map_or(Value::Null, |r| r["index"].clone());
    let mut line = format!("#{index:<4} ");
    match entry {
        Entry::Verified(_) => {
            let what = &value["what"];
            let call = what["call"].as_str().unwrap_or("?");
            let mut label = call.to_owned();
            if let Some(kind) = what["action"]["kind"].as_str() {
                let _ = write!(label, "/{kind}");
            } else if let Some(kind) = what["observed"]["kind"].as_str() {
                let _ = write!(label, "/{kind}");
            }
            let who = value["who"]["acts"].as_str().unwrap_or("?");
            let _ = write!(line, "verified  {label:<34} by {who:<22}");
            push_named(&mut line, what);
            push_named(&mut line, &what["action"]);
        }
        Entry::Stated(_) => {
            let statement = value["what"]["statement"].as_str().unwrap_or("?");
            let shape = statement.rsplit(':').next().unwrap_or(statement);
            let attribution = &value["attribution"];
            let (who, verified) = match attribution["signature"].as_str() {
                Some("verified") => (
                    attribution["who"]["acts"].as_str().unwrap_or("?"),
                    "signed, verified",
                ),
                // The log takes a provider's keys from the advertisement itself, so it can't verify that one by a key
                // it already holds: it names the provider by claim.
                _ if shape == "advertisement" => (
                    attribution["claimed_by"]["acts"].as_str().unwrap_or("?"),
                    "its keys are learnt from it",
                ),
                _ => (
                    attribution["claimed_by"]["acts"].as_str().unwrap_or("?"),
                    "UNVERIFIED",
                ),
            };
            let _ = write!(
                line,
                "stated    {shape:<34} by {who:<22} ({verified}, kept as {})",
                short(value["commitment"]["value"].as_str().unwrap_or("?"))
            );
            push_named(&mut line, &value["about"]);
        }
    }
    line
}

/// Adds the order, segment and provider `what` names, where it names them.
fn push_named(line: &mut String, what: &Value) {
    for key in ["order", "provider"] {
        if let Some(v) = what[key].as_str() {
            let _ = write!(line, " {key}={v}");
        }
    }
}
