//! The publisher behind each provider and node (`REQ-CAT-20`).

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
    /// A private node is declared by a publisher other than the one on record for it.
    NodeDeclaredByAnotherPublisher {
        /// The node.
        node: String,
        /// The publisher on record.
        recorded: String,
    },
}

/// Which publisher stands behind each provider and each private node.
#[derive(Debug, Clone, Default)]
pub struct PublisherIndex {
    providers: BTreeMap<String, String>,
    nodes: BTreeMap<String, String>,
}

impl PublisherIndex {
    /// Records the publisher behind an advertisement's provider and its private nodes. A repeat changes nothing;
    /// a provider or node that names a different publisher is refused, and nothing is recorded.
    pub fn register(&mut self, ad: &Advertisement) -> Result<(), PublisherError> {
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

    /// The publisher behind a provider.
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
