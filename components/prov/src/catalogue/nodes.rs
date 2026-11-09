//! The two lists of nodes, and the holding account behind each (`REQ-CAT-22`, `REQ-CAT-23`).

use std::collections::{BTreeMap, BTreeSet};

/// Why a node record is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeError {
    /// The node, or the reference, is blank.
    Blank,
    /// The node is declared private by a provider, so it can't also be on the operator's public list.
    DeclaredPrivate(String),
    /// The node isn't on the operator's public list.
    NotPublic(String),
    /// No provider declares this private node.
    NotDeclaredPrivate(String),
    /// A private node's holding account comes from its publisher, and this isn't the one on record.
    NotItsPublisher {
        /// The node.
        node: String,
        /// The publisher on record.
        recorded: String,
    },
}

/// Who gave the operator its way to a holding account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldingSource {
    /// The operator registered it: a public node's, by default a plain wallet, as a module outside the core.
    Operator,
    /// A private node's publisher gave the operator its interface.
    Publisher(String),
}

/// The way to a node's holding account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldingAccount {
    /// What the operator interacts with: a registered module, or the publisher's interface to its record.
    pub reference: String,
    /// Who gave it.
    pub source: HoldingSource,
}

/// The operator's public nodes and the holding account of every node.
#[derive(Debug, Clone, Default)]
pub struct NodeBook {
    public: BTreeSet<String>,
    holdings: BTreeMap<String, HoldingAccount>,
}

impl NodeBook {
    pub(super) fn add_public(&mut self, node: &str, private: bool) -> Result<(), NodeError> {
        if node.trim().is_empty() {
            return Err(NodeError::Blank);
        }
        if private {
            return Err(NodeError::DeclaredPrivate(node.to_owned()));
        }
        self.public.insert(node.to_owned());
        Ok(())
    }

    pub(super) fn hold_public(&mut self, node: &str, reference: &str) -> Result<(), NodeError> {
        if reference.trim().is_empty() {
            return Err(NodeError::Blank);
        }
        if !self.public.contains(node) {
            return Err(NodeError::NotPublic(node.to_owned()));
        }
        self.holdings.insert(
            node.to_owned(),
            HoldingAccount {
                reference: reference.to_owned(),
                source: HoldingSource::Operator,
            },
        );
        Ok(())
    }

    pub(super) fn hold_private(
        &mut self,
        node: &str,
        recorded: Option<&str>,
        publisher: &str,
        reference: &str,
    ) -> Result<(), NodeError> {
        if reference.trim().is_empty() {
            return Err(NodeError::Blank);
        }
        let Some(recorded) = recorded else {
            return Err(NodeError::NotDeclaredPrivate(node.to_owned()));
        };
        if recorded != publisher {
            return Err(NodeError::NotItsPublisher {
                node: node.to_owned(),
                recorded: recorded.to_owned(),
            });
        }
        self.holdings.insert(
            node.to_owned(),
            HoldingAccount {
                reference: reference.to_owned(),
                source: HoldingSource::Publisher(publisher.to_owned()),
            },
        );
        Ok(())
    }

    /// Whether the operator lists the node as public.
    pub fn is_public(&self, node: &str) -> bool {
        self.public.contains(node)
    }

    /// The operator's public nodes, in name order.
    pub fn public(&self) -> Vec<&str> {
        self.public.iter().map(String::as_str).collect()
    }

    /// The way to a node's holding account, if there is one.
    pub fn holding(&self, node: &str) -> Option<&HoldingAccount> {
        self.holdings.get(node)
    }
}
