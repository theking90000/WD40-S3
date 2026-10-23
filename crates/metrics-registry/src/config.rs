//! Shared history resolutions and retention policies.

use std::{collections::BTreeSet, time::Duration};

use crate::Error;

/// History policies applied to every key and metric in a local registry.
///
/// Configuration is supplied at initialization and is not persisted. Increasing
/// retention does not restore expired history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Nonempty list of policies with distinct resolutions.
    pub policies: Vec<RetentionPolicy>,
}

/// One history resolution and how long its windows are retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetentionPolicy {
    /// Nonzero window duration, expressed in whole seconds.
    pub resolution: Duration,
    /// Retention duration, at least as long as the resolution.
    pub retention: Duration,
}

impl Config {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.policies.is_empty() {
            return Err(Error::InvalidConfig(
                "at least one retention policy is required",
            ));
        }

        let mut resolutions = BTreeSet::new();
        for policy in &self.policies {
            if policy.resolution.is_zero() || policy.resolution.subsec_nanos() != 0 {
                return Err(Error::InvalidConfig(
                    "resolutions must be nonzero whole seconds",
                ));
            }
            if policy.retention < policy.resolution {
                return Err(Error::InvalidConfig(
                    "retention must be at least as long as its resolution",
                ));
            }
            if !resolutions.insert(policy.resolution) {
                return Err(Error::InvalidConfig("resolutions must be distinct"));
            }
        }
        Ok(())
    }
}
