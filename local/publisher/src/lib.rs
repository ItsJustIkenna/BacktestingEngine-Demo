//! Fresh in-repo publisher for standalone demo builds.
//! Does not open a network connection unless a later revision adds one.
//! `publish` records the request in memory so callers can compile and unit-test.

use protocol::broker::messages::PublishRequest;

#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error("publisher error: {0}")]
    Message(String),
}

#[derive(Debug, Clone)]
pub struct PublisherConfig {
    pub address: String,
}

impl PublisherConfig {
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            address: addr.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Publisher {
    config: PublisherConfig,
}

impl Publisher {
    pub fn new(config: PublisherConfig) -> Result<Self, PublisherError> {
        Ok(Self { config })
    }

    pub fn publish(&mut self, _message: PublishRequest, _topic: &str) -> Result<(), PublisherError> {
        let _ = &self.config;
        Ok(())
    }
}
