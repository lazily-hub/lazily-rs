//! Move-aware sequence CRDT for sibling order (#lzseqcrdt).
//!
//! [`SeqCrdt`] gives a **mergeable ordered sequence** of keyed elements without a
//! coordinator — the order layer above the per-cell value merge. It is the
//! sibling-order substrate for keyed reconciliation (`#lzkeyrecon`) of a document
//! tree under concurrent edits.
//!
//! # Design
//!
//! Each element carries a **fractional-index** [`Position`] (an orderable byte
//! key plus the originating peer as a tiebreak). Inserting between two neighbours
//! mints a key strictly between their keys, so concurrent inserts into the same
//! gap on different replicas both survive and converge to a deterministic order.
//!
//! Crucially it is **move-aware**: a move is a *single* [`LwwRegister`] reassign
//! of the element's position (highest [`HlcStamp`] wins), **not** a delete +
//! reinsert. So a reorder keeps the element's identity and value, and two
//! concurrent moves of the same element converge to the later one instead of
//! duplicating it (the failure mode of naive RGA delete+reinsert moves). Value,
//! position, and tombstone are independent LWW registers, so a concurrent
//! *move* and *value edit* of one element do not conflict.

use std::collections::HashMap;
use std::hash::Hash;

use crate::crdt::{CellCrdt, Hlc, LwwRegister};
use crate::distributed::PeerId;

/// A fractional-index position: an orderable byte key, tiebroken by the peer that
/// minted it so concurrent inserts into the same gap get a deterministic total
/// order. Compared lexicographically by `frac`, then `peer`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    frac: Vec<u8>,
    peer: PeerId,
}

impl Position {
    /// The raw fractional key bytes (for inspection/tests).
    pub fn frac(&self) -> &[u8] {
        &self.frac
    }
}

struct Entry<V> {
    value: LwwRegister<V>,
    position: LwwRegister<Position>,
    /// Tombstone as an LWW flag, so a remove converges and a concurrent
    /// resurrection is decided by stamp order.
    deleted: LwwRegister<bool>,
}

impl<V: Clone> Clone for Entry<V> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            position: self.position.clone(),
            deleted: self.deleted.clone(),
        }
    }
}

/// A move-aware, mergeable ordered sequence of `Id -> V`.
///
/// The clock is caller-driven: every mutator takes `now_micros` so behaviour is
/// deterministic and testable (the embedded [`Hlc`] never reads the system clock).
pub struct SeqCrdt<Id, V> {
    entries: HashMap<Id, Entry<V>>,
    hlc: Hlc,
    peer: PeerId,
}

impl<Id, V> SeqCrdt<Id, V>
where
    Id: Eq + Hash + Clone,
    V: Clone + PartialEq,
{
    /// Create an empty sequence owned by `peer`.
    pub fn new(peer: PeerId) -> Self {
        Self {
            entries: HashMap::new(),
            hlc: Hlc::new(peer),
            peer,
        }
    }

    fn frac_of(&self, id: &Id) -> Option<Vec<u8>> {
        self.entries.get(id).map(|e| e.position.value().frac)
    }

    /// Insert `id`/`value` between the live neighbours `left` and `right`
    /// (either `None` for an open end). If `id` already exists this is a no-op
    /// (use [`move_between`](Self::move_between) to relocate it).
    pub fn insert_between(
        &mut self,
        id: Id,
        value: V,
        left: Option<&Id>,
        right: Option<&Id>,
        now_micros: u64,
    ) {
        if self.entries.contains_key(&id) {
            return;
        }
        let lo = left.and_then(|l| self.frac_of(l));
        let hi = right.and_then(|r| self.frac_of(r));
        let frac = key_between(lo.as_deref(), hi.as_deref());
        let pos = Position {
            frac,
            peer: self.peer,
        };
        let stamp = self.hlc.send(now_micros);
        self.entries.insert(
            id,
            Entry {
                value: LwwRegister::new(value, stamp),
                position: LwwRegister::new(pos, stamp),
                deleted: LwwRegister::new(false, stamp),
            },
        );
    }

    /// Append `id`/`value` after the current last live element.
    pub fn insert_back(&mut self, id: Id, value: V, now_micros: u64) {
        let last = self.order().pop();
        self.insert_between(id, value, last.as_ref(), None, now_micros);
    }

    /// Prepend `id`/`value` before the current first live element.
    pub fn insert_front(&mut self, id: Id, value: V, now_micros: u64) {
        let first = self.order().into_iter().next();
        self.insert_between(id, value, None, first.as_ref(), now_micros);
    }

    /// Last-writer-wins update of `id`'s value. Returns whether it applied.
    pub fn set_value(&mut self, id: &Id, value: V, now_micros: u64) -> bool {
        let stamp = self.hlc.send(now_micros);
        match self.entries.get_mut(id) {
            Some(e) => e.value.set(value, stamp),
            None => false,
        }
    }

    /// Atomically move `id` between `left` and `right` (move-aware): a single
    /// LWW reassignment of its position, keeping identity and value. Returns
    /// whether it applied.
    pub fn move_between(
        &mut self,
        id: &Id,
        left: Option<&Id>,
        right: Option<&Id>,
        now_micros: u64,
    ) -> bool {
        if !self.entries.contains_key(id) {
            return false;
        }
        let lo = left.and_then(|l| self.frac_of(l));
        let hi = right.and_then(|r| self.frac_of(r));
        let frac = key_between(lo.as_deref(), hi.as_deref());
        let pos = Position {
            frac,
            peer: self.peer,
        };
        let stamp = self.hlc.send(now_micros);
        self.entries.get_mut(id).unwrap().position.set(pos, stamp)
    }

    /// Move `id` to just after `anchor`.
    pub fn move_after(&mut self, id: &Id, anchor: &Id, now_micros: u64) -> bool {
        let ord = self.order();
        let right = ord
            .iter()
            .position(|x| x == anchor)
            .and_then(|i| ord.get(i + 1))
            .cloned();
        self.move_between(id, Some(anchor), right.as_ref(), now_micros)
    }

    /// Move `id` to just before `anchor`.
    pub fn move_before(&mut self, id: &Id, anchor: &Id, now_micros: u64) -> bool {
        let ord = self.order();
        let left = ord
            .iter()
            .position(|x| x == anchor)
            .filter(|&i| i > 0)
            .map(|i| ord[i - 1].clone());
        self.move_between(id, left.as_ref(), Some(anchor), now_micros)
    }

    /// Tombstone `id` (LWW). Returns whether it applied.
    pub fn remove(&mut self, id: &Id, now_micros: u64) -> bool {
        let stamp = self.hlc.send(now_micros);
        match self.entries.get_mut(id) {
            Some(e) => e.deleted.set(true, stamp),
            None => false,
        }
    }

    /// Whether `id` is present and live (not tombstoned).
    pub fn contains(&self, id: &Id) -> bool {
        self.entries.get(id).is_some_and(|e| !e.deleted.value())
    }

    /// Read `id`'s value if it is live.
    pub fn get(&self, id: &Id) -> Option<V> {
        self.entries
            .get(id)
            .filter(|e| !e.deleted.value())
            .map(|e| e.value.value())
    }

    /// Live element ids in sequence order.
    pub fn order(&self) -> Vec<Id> {
        let mut live: Vec<(&Id, Position)> = self
            .entries
            .iter()
            .filter(|(_, e)| !e.deleted.value())
            .map(|(id, e)| (id, e.position.value()))
            .collect();
        live.sort_by(|a, b| a.1.cmp(&b.1));
        live.into_iter().map(|(id, _)| id.clone()).collect()
    }

    /// Live `(id, value)` pairs in sequence order.
    pub fn values(&self) -> Vec<(Id, V)> {
        self.order()
            .into_iter()
            .filter_map(|id| self.get(&id).map(|v| (id, v)))
            .collect()
    }

    /// Merge another replica's state in (commutative, associative, idempotent):
    /// per-element LWW of value, position, and tombstone; unknown elements are
    /// adopted. Advances the local clock past everything observed so later local
    /// writes still win against merged state. Returns whether anything changed.
    pub fn merge(&mut self, other: &SeqCrdt<Id, V>, now_micros: u64) -> bool {
        // Advance the clock past the highest stamp we are about to observe.
        let mut max_stamp = None;
        for e in other.entries.values() {
            for s in [e.value.stamp(), e.position.stamp(), e.deleted.stamp()] {
                max_stamp = Some(max_stamp.map_or(s, |m: crate::crdt::HlcStamp| m.max(s)));
            }
        }
        if let Some(s) = max_stamp {
            self.hlc.recv(s, now_micros);
        }

        let mut changed = false;
        for (id, oe) in &other.entries {
            match self.entries.get_mut(id) {
                Some(e) => {
                    changed |= e.value.merge_from(&oe.value);
                    changed |= e.position.merge_from(&oe.position);
                    changed |= e.deleted.merge_from(&oe.deleted);
                }
                None => {
                    self.entries.insert(id.clone(), oe.clone());
                    changed = true;
                }
            }
        }
        changed
    }
}

/// Generate a fractional key strictly between `lo` and `hi` (each `None` for an
/// open end), as a byte sequence compared lexicographically. Precondition:
/// `lo < hi` when both are present.
fn key_between(lo: Option<&[u8]>, hi: Option<&[u8]>) -> Vec<u8> {
    let mut result = Vec::new();
    let mut i = 0usize;
    // Safety bound: the shared prefix can be at most lo.len()+hi.len() long.
    let cap = lo.map_or(0, |l| l.len()) + hi.map_or(0, |h| h.len()) + 2;
    while i <= cap {
        let a: u16 = lo.and_then(|l| l.get(i)).map_or(0, |&d| d as u16);
        let b: u16 = match hi {
            Some(h) => h.get(i).map_or(0, |&d| d as u16),
            None => 256,
        };
        if a + 1 < b {
            // Gap of >= 2 at this digit: a midpoint digit lands strictly between.
            result.push(((a + b) / 2) as u8);
            return result;
        }
        // Gap < 2: commit the lower digit and descend.
        result.push(a as u8);
        i += 1;
        if a < b {
            // We dropped strictly below `hi` at this digit, so deeper digits are
            // bounded only by `lo`'s tail; recurse with an open top.
            let lo_tail: Vec<u8> = lo
                .map(|l| l.get(i..).unwrap_or(&[]).to_vec())
                .unwrap_or_default();
            result.extend(key_between(Some(&lo_tail), None));
            return result;
        }
        // a == b: shared prefix digit; continue.
    }
    // Degenerate (lo not < hi): append a midpoint and stop.
    result.push(128);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(n: u64) -> PeerId {
        PeerId(n)
    }

    #[test]
    fn key_between_produces_strict_total_order() {
        let a = key_between(None, None);
        let lo = key_between(None, Some(&a));
        let hi = key_between(Some(&a), None);
        assert!(lo < a && a < hi, "{lo:?} < {a:?} < {hi:?}");
        // Subdivide repeatedly between lo and a.
        let mut left = lo.clone();
        for _ in 0..50 {
            let mid = key_between(Some(&left), Some(&a));
            assert!(left < mid && mid < a, "{left:?} < {mid:?} < {a:?}");
            left = mid;
        }
    }

    #[test]
    fn insert_back_and_front_orders() {
        let mut s: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        s.insert_back("a", 1, 1);
        s.insert_back("b", 2, 2);
        s.insert_back("c", 3, 3);
        s.insert_front("z", 0, 4);
        assert_eq!(s.order(), vec!["z", "a", "b", "c"]);
        assert_eq!(s.get(&"b"), Some(2));
    }

    #[test]
    fn move_is_single_reassignment_no_duplication() {
        let mut s: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        for (i, k) in ["a", "b", "c", "d"].iter().enumerate() {
            s.insert_back(k, i as i32, i as u64 + 1);
        }
        assert_eq!(s.order(), vec!["a", "b", "c", "d"]);
        // Move "a" to the end.
        assert!(s.move_after(&"a", &"d", 10));
        assert_eq!(s.order(), vec!["b", "c", "d", "a"]);
        // Identity + value preserved, no duplicate element.
        assert_eq!(s.get(&"a"), Some(0));
        assert_eq!(s.order().len(), 4);
    }

    #[test]
    fn concurrent_inserts_same_gap_converge() {
        // Two replicas start from a shared single element, each insert after it.
        let mut a: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        a.insert_back("root", 0, 1);
        let mut b = SeqCrdt::new(peer(2));
        b.merge(&a, 2);

        a.insert_back("a1", 1, 10); // peer 1 inserts after root
        b.insert_back("b1", 2, 10); // peer 2 inserts after root (concurrent)

        // Merge both ways; order must converge identically and keep both.
        let mut a2 = a.clone_state();
        a2.merge(&b, 20);
        let mut b2 = b.clone_state();
        b2.merge(&a, 20);
        assert_eq!(a2.order(), b2.order());
        assert_eq!(a2.order().len(), 3);
        assert!(a2.contains(&"a1") && a2.contains(&"b1"));
    }

    #[test]
    fn concurrent_move_converges_to_later_stamp() {
        let mut a: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        for (i, k) in ["x", "y", "z"].iter().enumerate() {
            a.insert_back(k, i as i32, i as u64 + 1);
        }
        let mut b = a.clone_state_as(peer(2));

        // Both move "x"; peer 2's move has the later wall time -> it wins.
        a.move_after(&"x", &"y", 10); // -> y, x, z
        b.move_after(&"x", &"z", 20); // -> y, z, x  (later)

        let mut merged = a.clone_state();
        merged.merge(&b, 30);
        assert_eq!(merged.order(), vec!["y", "z", "x"]);
        // No duplication from the concurrent moves.
        assert_eq!(merged.order().len(), 3);
    }

    #[test]
    fn concurrent_move_and_value_edit_do_not_conflict() {
        let mut a: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        a.insert_back("a", 1, 1);
        a.insert_back("b", 2, 2);
        let mut b = a.clone_state_as(peer(2));

        a.move_after(&"a", &"b", 10); // peer 1 reorders
        b.set_value(&"a", 99, 10); // peer 2 edits value (concurrent)

        let mut merged = a.clone_state();
        merged.merge(&b, 20);
        assert_eq!(merged.order(), vec!["b", "a"]); // move applied
        assert_eq!(merged.get(&"a"), Some(99)); // value edit applied
    }

    #[test]
    fn remove_tombstone_converges_and_merge_is_commutative() {
        let mut a: SeqCrdt<&str, i32> = SeqCrdt::new(peer(1));
        for (i, k) in ["a", "b", "c"].iter().enumerate() {
            a.insert_back(k, i as i32, i as u64 + 1);
        }
        let mut b = a.clone_state_as(peer(2));
        a.remove(&"b", 10);
        b.move_after(&"a", &"c", 11);

        let mut ab = a.clone_state();
        ab.merge(&b, 20);
        let mut ba = b.clone_state();
        ba.merge(&a, 20);
        assert_eq!(ab.order(), ba.order(), "merge must be commutative");
        assert!(!ab.contains(&"b"), "tombstone converges");
    }

    // --- test helpers: cheap state clones for two-replica scenarios ---
    impl<Id, V> SeqCrdt<Id, V>
    where
        Id: Eq + Hash + Clone,
        V: Clone,
    {
        fn clone_state(&self) -> Self {
            self.clone_state_as(self.peer)
        }
        fn clone_state_as(&self, peer: PeerId) -> Self {
            let mut entries = HashMap::new();
            for (id, e) in &self.entries {
                entries.insert(id.clone(), e.clone());
            }
            SeqCrdt {
                entries,
                hlc: Hlc::new(peer),
                peer,
            }
        }
    }
}
