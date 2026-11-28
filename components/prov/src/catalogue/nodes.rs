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
    /// A public node's holding account is a module the operator publishes, and this one isn't (`T-016-04`): a
    /// provider's module, or one no one has published, is never a node's holding account (`REQ-CAT-23`).
    NotOperatorModule(String),
    /// No provider declares this private node.
    NotDeclaredPrivate(String),
    /// A public network is taken onto the public list only from a decided listing naming its reader service
    /// (`DEC-28`, `T-020-60`), and this names none.
    NoListing(String),
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
    /// Each public network a decided listing names (`DEC-28`, `T-020-60`), and whether its reader passed Core's
    /// start-up check: one that didn't is held off every route.
    networks: BTreeMap<String, bool>,
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

    pub(super) fn hold_public(
        &mut self,
        node: &str,
        reference: &str,
        operators: bool,
    ) -> Result<(), NodeError> {
        if reference.trim().is_empty() {
            return Err(NodeError::Blank);
        }
        if !self.public.contains(node) {
            return Err(NodeError::NotPublic(node.to_owned()));
        }
        if !operators {
            return Err(NodeError::NotOperatorModule(reference.to_owned()));
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

    /// Takes `node` off the operator's public list; whether it was on it. Its holding account stays on record, so value
    /// that rests there is still accounted for (`INV-5`); no account can be registered there again until it is listed.
    pub(super) fn remove_public(&mut self, node: &str) -> bool {
        self.public.remove(node)
    }

    /// Whether `network` is listed by a decision naming its reader service and its reader passed Core's start-up check:
    /// on the public list of networks (`REQ-CAT-22`, `T-020-60`).
    pub fn is_started_network(&self, network: &str) -> bool {
        self.networks.get(network) == Some(&true)
    }

    /// Whether `network` is listed but its reader didn't pass Core's start-up check, or wasn't checked: held off every
    /// route (`T-020-60`).
    pub fn is_withheld_network(&self, network: &str) -> bool {
        self.networks.get(network) == Some(&false)
    }

    /// The public networks whose reader passed Core's start-up check, in name order.
    pub fn public_networks(&self) -> Vec<&str> {
        self.networks
            .iter()
            .filter(|(_, started)| **started)
            .map(|(network, _)| network.as_str())
            .collect()
    }

    /// Whether `network` is on the public list of networks from a listing, started or held off (`T-020-60`).
    pub fn is_listed_network(&self, network: &str) -> bool {
        self.networks.contains_key(network)
    }

    pub(super) fn take_network(&mut self, network: &str, started: bool) {
        self.networks.insert(network.to_owned(), started);
    }

    pub(super) fn drop_network(&mut self, network: &str) -> bool {
        self.networks.remove(network).is_some()
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

    /// Every node that has a holding account, with the way to it, in node order: what Core's schedule holds the
    /// deployment's register of holding accounts to (`T-020-63`).
    pub fn holders(&self) -> impl Iterator<Item = (&str, &HoldingAccount)> {
        self.holdings
            .iter()
            .map(|(node, account)| (node.as_str(), account))
    }
}
