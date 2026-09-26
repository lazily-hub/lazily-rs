/// The highest durable capability compiled into this crate instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DurableCapabilityTier {
    Core,
    Client,
    DurableHost,
    DistributedHost,
    AcceleratedHost,
}

/// Report the highest compiled durable capability without implying that an
/// application has configured its external dependencies.
#[must_use]
pub const fn compiled_durable_capability_tier() -> DurableCapabilityTier {
    #[cfg(feature = "durable-jetstream")]
    return DurableCapabilityTier::DistributedHost;

    #[cfg(all(not(feature = "durable-jetstream"), feature = "durable-postgres"))]
    return DurableCapabilityTier::DurableHost;

    #[cfg(all(
        not(feature = "durable-jetstream"),
        not(feature = "durable-postgres"),
        feature = "durable-client"
    ))]
    return DurableCapabilityTier::Client;

    #[cfg(not(any(
        feature = "durable-jetstream",
        feature = "durable-postgres",
        feature = "durable-client"
    )))]
    return DurableCapabilityTier::Core;
}

#[cfg(test)]
mod tests {
    use super::{DurableCapabilityTier, compiled_durable_capability_tier};

    #[test]
    fn compiled_tier_matches_features() {
        let tier = compiled_durable_capability_tier();
        if cfg!(feature = "durable-jetstream") {
            assert_eq!(tier, DurableCapabilityTier::DistributedHost);
        } else if cfg!(feature = "durable-postgres") {
            assert_eq!(tier, DurableCapabilityTier::DurableHost);
        } else if cfg!(feature = "durable-client") {
            assert_eq!(tier, DurableCapabilityTier::Client);
        } else {
            assert_eq!(tier, DurableCapabilityTier::Core);
        }
        assert_ne!(tier, DurableCapabilityTier::AcceleratedHost);
    }
}
