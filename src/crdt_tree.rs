//! Lossless mergeable document-tree contract (`#lzcrdttree`).

use crate::{TextCrdt, TextOp, TextVersionVector};

/// A lossless document CRDT with a compact distributed frontier and transport delta.
///
/// Implementations must make [`merge_from`](Self::merge_from) and delta application
/// commutative, associative, and idempotent. A snapshot is exactly
/// `delta_since(&Default::default())`, so snapshot and incremental replication share one
/// representation and preserve operation identity.
pub trait CrdtTree {
    type VersionVector: Clone + Default + PartialEq + Eq;
    type Delta: Clone + PartialEq + Eq;
    type Value: Clone + PartialEq + Eq;

    fn version_vector(&self) -> Self::VersionVector;
    fn delta_since(&self, version: &Self::VersionVector) -> Self::Delta;
    fn apply_delta(&mut self, delta: &Self::Delta) -> bool;
    fn text(&self) -> String;
    fn value(&self) -> Self::Value;
    fn merge_from(&mut self, other: &Self) -> bool;
}

impl CrdtTree for TextCrdt {
    type VersionVector = TextVersionVector;
    type Delta = Vec<TextOp>;
    type Value = String;

    fn version_vector(&self) -> Self::VersionVector {
        TextCrdt::version_vector(self)
    }

    fn delta_since(&self, version: &Self::VersionVector) -> Self::Delta {
        TextCrdt::delta_since(self, version)
    }

    fn apply_delta(&mut self, delta: &Self::Delta) -> bool {
        TextCrdt::apply_delta(self, delta)
    }

    fn text(&self) -> String {
        TextCrdt::text(self)
    }

    fn value(&self) -> Self::Value {
        TextCrdt::text(self)
    }

    fn merge_from(&mut self, other: &Self) -> bool {
        TextCrdt::merge(self, other)
    }
}
