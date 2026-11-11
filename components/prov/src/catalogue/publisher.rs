//! The publisher behind each provider and node (`REQ-CAT-20`), and the modules the operator publishes itself
//! (`T-016-04`; `REQ-CAT-23`, `REQ-MDL-1`).
//!
//! A module the operator publishes, such as a public node's holding account, has no advertisement: the operator
//! records its name here, and is from then on its publisher. The two records exclude each other, so a provider
//! can't advertise under the name of a module the operator publishes, and the operator can't publish a module under
//! a provider's name. Neither can then release the other's module (`REQ-MDL-1`).

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::provider_role::Advertisement;

/// Why a publisher record is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublisherError {
    /// The provider is recorded under another publisher.
    ProviderChangedPublisher {
        /// The provider.
        provider: String,
        /// The publisher on record.
        recorded: String,
        /// The publisher the advertisement names.
        offered: String,
    },
    /// The provider is a module the operator publishes, so no advertisement can name it (`REQ-MDL-1`).
    ProviderIsTheOperators(String),
    /// The operator can't publish a module under the name of a provider a publisher stands behind
    /// (`REQ-MDL-1`).
    ProviderOfAPublisher {
        /// The provider.
        provider: String,
        /// The publisher on record.
        publisher: String,
    },
    /// The name of a module the operator publishes is blank.
    Blank,
    /// A private node is declared by a publisher other than the one on record for it.
    NodeDeclaredByAnotherPublisher {
        /// The node.
        node: String,
        /// The publisher on record.
        recorded: String,
    },
}

/// Who publishes a provider's module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Publisher {
    /// The operator itself: a module it publishes, with no advertisement, such as a public node's holding account.
    Operator,
    /// The publisher an accepted advertisement names.
    Party(String),
}

/// Which publisher stands behind each provider and each private node, and which modules the operator publishes.
#[derive(Debug, Clone, Default)]
pub struct PublisherIndex {
    providers: BTreeMap<String, String>,
    nodes: BTreeMap<String, String>,
    operator: BTreeSet<String>,
}

impl PublisherIndex {
    /// Records the publisher behind an advertisement's provider and its private nodes. A repeat changes nothing;
    /// a provider or node that names a different publisher is refused, and nothing is recorded.
    pub fn register(&mut self, ad: &Advertisement) -> Result<(), PublisherError> {
        if self.operator.contains(&ad.provider) {
            return Err(PublisherError::ProviderIsTheOperators(ad.provider.clone()));
        }
        if let Some(recorded) = self
            .providers
            .get(&ad.provider)
            .filter(|p| **p != ad.publisher)
        {
            return Err(PublisherError::ProviderChangedPublisher {
                provider: ad.provider.clone(),
                recorded: recorded.clone(),
                offered: ad.publisher.clone(),
            });
        }
        for node in &ad.private_nodes {
            if let Some(recorded) = self.nodes.get(node).filter(|p| **p != ad.publisher) {
                return Err(PublisherError::NodeDeclaredByAnotherPublisher {
                    node: node.clone(),
                    recorded: recorded.clone(),
                });
            }
        }
        self.providers
            .insert(ad.provider.clone(), ad.publisher.clone());
        for node in &ad.private_nodes {
            self.nodes.insert(node.clone(), ad.publisher.clone());
        }
        Ok(())
    }

    /// Records `provider` as a module the operator publishes (`REQ-CAT-23`). A repeat changes nothing; a blank
    /// name, or a provider a publisher's advertisement stands behind, is refused, and nothing is recorded.
    pub fn register_operator(&mut self, provider: &str) -> Result<(), PublisherError> {
        if provider.trim().is_empty() {
            return Err(PublisherError::Blank);
        }
        if let Some(publisher) = self.providers.get(provider) {
            return Err(PublisherError::ProviderOfAPublisher {
                provider: provider.to_owned(),
                publisher: publisher.clone(),
            });
        }
        self.operator.insert(provider.to_owned());
        Ok(())
    }

    /// Who publishes a provider's module: the operator, a publisher, or no one on record.
    pub fn publisher(&self, provider: &str) -> Option<Publisher> {
        if self.operator.contains(provider) {
            return Some(Publisher::Operator);
        }
        self.publisher_of_provider(provider)
            .map(|p| Publisher::Party(p.to_owned()))
    }

    /// Whether the operator publishes the provider's module.
    pub fn published_by_operator(&self, provider: &str) -> bool {
        self.operator.contains(provider)
    }

    /// The modules the operator publishes, in name order.
    pub fn operator_modules(&self) -> Vec<&str> {
        self.operator.iter().map(String::as_str).collect()
    }

    /// The publisher behind a provider, as its accepted advertisement names it. A module the operator publishes
    /// has none: see [`Self::publisher`].
    pub fn publisher_of_provider(&self, provider: &str) -> Option<&str> {
        self.providers.get(provider).map(String::as_str)
    }

    /// The publisher behind a private node.
    pub fn publisher_of_node(&self, node: &str) -> Option<&str> {
        self.nodes.get(node).map(String::as_str)
    }

    /// A publisher's providers, in name order.
    pub fn providers_of(&self, publisher: &str) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|(_, p)| *p == publisher)
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// A publisher's private nodes, in name order.
    pub fn nodes_of(&self, publisher: &str) -> Vec<&str> {
        let set: BTreeSet<&str> = self
            .nodes
            .iter()
            .filter(|(_, p)| *p == publisher)
            .map(|(k, _)| k.as_str())
            .collect();
        set.into_iter().collect()
    }
}
