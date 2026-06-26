//! Free-text character sequence CRDT + re-parse (#lztextcrdt).
//!
//! The anchored-skeleton layers ([`reconcile`](crate::reconcile),
//! [`stable_id`](crate::stable_id)) buy identity for *controlled* structure. For
//! arbitrary prose with no anchors and **concurrent** edits, the merge unit drops
//! to characters: [`TextCrdt`] merges keystrokes, then you **re-parse** the merged
//! text and re-derive the structural tree. The tree is a *projection* of
//! CRDT-merged text, not the merge unit itself. Honest floor: a true rewrite *is*
//! a replace — there is no character identity to preserve through it.
//!
//! # Algorithm
//!
//! A Fugue/RGA-style tree CRDT. Each inserted character is an element with a
//! unique [`OpId`] and a **left origin** (the element it was typed after). The
//! sequence is the in-order traversal of the origin tree, with same-origin
//! siblings ordered by `OpId` descending (newest-after-origin first — the RGA
//! tiebreak). Deletes are tombstones. `order` is therefore a pure, deterministic
//! function of the element set, so [`merge`](TextCrdt::merge) (a union of
//! elements, tombstones sticky) is commutative, associative, and idempotent.
//!
//! ```
//! use lazily::TextCrdt;
//!
//! // Two replicas fork from "hi" and edit concurrently.
//! let mut a = TextCrdt::from_str(1, "hi");
//! let mut b = a.fork(2);
//! a.insert(2, '!');          // "hi!"
//! b.insert(0, 'O');          // "Ohi"
//! a.merge(&b);
//! b.merge(&a);
//! assert_eq!(a.text(), b.text()); // converged, both edits preserved
//! ```

use std::collections::HashMap;

use crate::stable_id::Block;

/// A globally-unique, totally-ordered id for one inserted character.
///
/// Ordered by `(counter, peer)`; the counter is Lamport-style (advances past
/// everything observed on merge), so a causally-later insert sorts higher and a
/// concurrent insert tiebreaks deterministically by peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId {
    counter: u64,
    peer: u64,
}

#[derive(Debug, Clone)]
struct Elem {
    ch: char,
    /// The element this character was inserted *after* (None = document start).
    origin: Option<OpId>,
    /// `None` while live; `Some(delete_op)` once tombstoned. Carrying the
    /// *delete's* own [`OpId`] (not a bare flag) is what lets GC test whether the
    /// *deletion* — not merely the insertion — is causally stable (#lztombgc).
    /// Tombstones are sticky; concurrent deletes converge to the smaller `OpId`.
    deleted: Option<OpId>,
}

/// A character-granular, mergeable text buffer for concurrent free-text edits.
#[derive(Debug, Clone)]
pub struct TextCrdt {
    elems: HashMap<OpId, Elem>,
    peer: u64,
    counter: u64,
}

impl TextCrdt {
    /// An empty buffer owned by `peer`.
    pub fn new(peer: u64) -> Self {
        Self {
            elems: HashMap::new(),
            peer,
            counter: 0,
        }
    }

    /// A buffer owned by `peer` seeded with `s` (a linear chain of characters).
    pub fn from_str(peer: u64, s: &str) -> Self {
        let mut t = Self::new(peer);
        t.insert_str(0, s);
        t
    }

    /// Fork this buffer's state to a new replica `peer` (deep copy, new identity).
    /// Used to model two replicas diverging from a shared base.
    pub fn fork(&self, peer: u64) -> Self {
        Self {
            elems: self.elems.clone(),
            peer,
            counter: self.counter,
        }
    }

    fn next_id(&mut self) -> OpId {
        self.counter += 1;
        OpId {
            counter: self.counter,
            peer: self.peer,
        }
    }

    /// Insert `ch` at visible index `index` (0 = start, `len` = end).
    pub fn insert(&mut self, index: usize, ch: char) {
        let visible = self.ordered_ids(false);
        let origin = if index == 0 {
            None
        } else {
            visible.get(index - 1).copied()
        };
        let id = self.next_id();
        self.elems.insert(
            id,
            Elem {
                ch,
                origin,
                deleted: None,
            },
        );
    }

    /// Insert all of `s` starting at visible index `index`.
    pub fn insert_str(&mut self, index: usize, s: &str) {
        for (i, ch) in s.chars().enumerate() {
            self.insert(index + i, ch);
        }
    }

    /// Tombstone the visible character at `index`. No-op if out of range.
    pub fn delete(&mut self, index: usize) {
        let visible = self.ordered_ids(false);
        if let Some(id) = visible.get(index).copied() {
            // Mint a distinct OpId for the deletion so GC can later test whether
            // the *delete* is causally stable. No-op if already tombstoned.
            let del = self.next_id();
            if let Some(e) = self.elems.get_mut(&id)
                && e.deleted.is_none()
            {
                e.deleted = Some(del);
            }
        }
    }

    /// The current visible text in sequence order.
    pub fn text(&self) -> String {
        self.ordered_ids(false)
            .into_iter()
            .filter_map(|id| self.elems.get(&id).map(|e| e.ch))
            .collect()
    }

    /// Number of visible characters.
    pub fn len(&self) -> usize {
        self.elems.values().filter(|e| e.deleted.is_none()).count()
    }

    /// Number of tombstoned-but-not-yet-collected characters — the GC-pressure
    /// gauge behind the "memory bloat" critique.
    pub fn tombstone_count(&self) -> usize {
        self.elems.values().filter(|e| e.deleted.is_some()).count()
    }

    /// Whether there is any visible text.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ordered element ids via the origin tree (in-order traversal; same-origin
    /// siblings newest-first). `include_deleted` keeps tombstones in the order
    /// (needed so later origins still resolve), else they are filtered.
    fn ordered_ids(&self, include_deleted: bool) -> Vec<OpId> {
        // children[origin] = ids inserted directly after `origin`.
        let mut children: HashMap<Option<OpId>, Vec<OpId>> = HashMap::new();
        for (id, e) in &self.elems {
            children.entry(e.origin).or_default().push(*id);
        }
        for list in children.values_mut() {
            // Descending OpId: the most recent insert-after-origin comes first.
            list.sort_unstable_by(|a, b| b.cmp(a));
        }
        let mut out = Vec::with_capacity(self.elems.len());
        // Iterative pre-order DFS; stack holds ids to visit (reversed so the
        // first child is processed first).
        let mut stack: Vec<OpId> = children
            .get(&None)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .rev()
            .collect();
        while let Some(id) = stack.pop() {
            let e = &self.elems[&id];
            if include_deleted || e.deleted.is_none() {
                out.push(id);
            }
            if let Some(kids) = children.get(&Some(id)) {
                // Push reversed so the first (highest-OpId) child pops first.
                for &k in kids.iter().rev() {
                    stack.push(k);
                }
            }
        }
        out
    }

    /// Merge another replica's edits (commutative, associative, idempotent):
    /// union of elements by id, with tombstones sticky (a delete on either side
    /// wins). Advances the local counter past everything observed. Returns
    /// whether the visible text changed.
    pub fn merge(&mut self, other: &TextCrdt) -> bool {
        let before = self.text();
        for (id, oe) in &other.elems {
            self.counter = self.counter.max(id.counter);
            // Delete OpIds advance the clock too, so a local insert after a
            // merge can never collide with an observed deletion's id.
            if let Some(d) = oe.deleted {
                self.counter = self.counter.max(d.counter);
            }
            match self.elems.get_mut(id) {
                Some(e) => {
                    // Tombstone is sticky and order-independent: keep whichever
                    // delete id is smaller so concurrent deletes converge
                    // (commutative/associative) instead of depending on merge order.
                    e.deleted = match (e.deleted, oe.deleted) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                }
                None => {
                    self.elems.insert(*id, oe.clone());
                }
            }
        }
        self.text() != before
    }

    /// Garbage-collect causally-stable deletion tombstones (#lztombgc).
    ///
    /// `is_stable(delete_op_id)` is the caller-supplied "every replica has
    /// observed this deletion" policy — the distributed plane (`#lzcrdtplane`)
    /// derives it from its anti-entropy version vectors. Mechanism only, and
    /// deliberately conservative: a tombstoned element is collected only when it
    /// is **not referenced as any element's left origin**, so removing it can
    /// never orphan a surviving character. Interior tombstones are reclaimed
    /// bottom-up as their descendants are themselves collected (contiguous-run
    /// compaction with origin-rewrite is the heavier follow-up). Returns the
    /// number of elements collected.
    pub fn gc_with(&mut self, is_stable: impl Fn(OpId) -> bool) -> usize {
        let mut removed = 0;
        loop {
            let referenced: std::collections::HashSet<OpId> =
                self.elems.values().filter_map(|e| e.origin).collect();
            let collectable: Vec<OpId> = self
                .elems
                .iter()
                .filter(|(id, e)| e.deleted.is_some_and(&is_stable) && !referenced.contains(id))
                .map(|(id, _)| *id)
                .collect();
            if collectable.is_empty() {
                break;
            }
            for id in collectable {
                self.elems.remove(&id);
                removed += 1;
            }
        }
        removed
    }
}

/// Re-parse merged text into paragraph [`Block`]s (split on blank lines). This is
/// the "re-derive the tree from CRDT-merged text" step: feed the result through
/// [`assign_stable_keys`](crate::stable_id::assign_stable_keys) +
/// [`reconcile`](crate::reconcile) to project the merged text onto the keyed tree.
pub fn parse_blocks(text: &str) -> Vec<Block> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(Block::text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_insert_and_delete() {
        let mut t = TextCrdt::from_str(1, "helo");
        t.insert(3, 'l'); // "hello"
        assert_eq!(t.text(), "hello");
        t.insert(5, '!'); // append
        assert_eq!(t.text(), "hello!");
        t.delete(0); // drop 'h'
        assert_eq!(t.text(), "ello!");
        assert_eq!(t.len(), 5);
    }

    #[test]
    fn concurrent_inserts_converge_keeping_both() {
        let mut a = TextCrdt::from_str(1, "hi");
        let mut b = a.fork(2);
        a.insert(2, '!'); // "hi!"
        b.insert(0, 'O'); // "Ohi"
        let changed = a.merge(&b);
        b.merge(&a);
        assert!(changed);
        assert_eq!(a.text(), b.text(), "replicas converge");
        // Both edits survive.
        assert!(a.text().contains('!') && a.text().contains('O'));
        assert_eq!(a.text().len(), 4);
    }

    #[test]
    fn concurrent_inserts_at_same_spot_converge_deterministically() {
        let mut a = TextCrdt::from_str(1, "XY");
        let mut b = a.fork(2);
        a.insert(1, 'a'); // between X and Y on replica 1
        b.insert(1, 'b'); // between X and Y on replica 2 (concurrent)
        a.merge(&b);
        b.merge(&a);
        assert_eq!(a.text(), b.text());
        // Deterministic order, both present, anchored between X and Y.
        assert_eq!(a.text().len(), 4);
        assert!(a.text().starts_with('X') && a.text().ends_with('Y'));
    }

    #[test]
    fn concurrent_insert_and_delete_merge() {
        let mut a = TextCrdt::from_str(1, "abc");
        let mut b = a.fork(2);
        a.delete(1); // delete 'b' -> "ac"
        b.insert(3, 'd'); // append 'd' -> "abcd"
        a.merge(&b);
        b.merge(&a);
        assert_eq!(a.text(), b.text());
        assert_eq!(a.text(), "acd"); // delete + insert both applied
    }

    #[test]
    fn merge_is_idempotent_and_commutative() {
        let mut a = TextCrdt::from_str(1, "one");
        let mut b = a.fork(2);
        a.insert(3, 'X');
        b.insert(0, 'Y');
        let mut ab = a.clone();
        ab.merge(&b);
        ab.merge(&b); // idempotent
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab.text(), ba.text(), "commutative");
        let once = {
            let mut x = a.clone();
            x.merge(&b);
            x.text()
        };
        assert_eq!(ab.text(), once, "idempotent");
    }

    #[test]
    fn reparse_projects_merged_text_onto_keyed_blocks() {
        use crate::stable_id::assign_stable_keys;

        // Old doc: two paragraphs.
        let old_text = "first paragraph\n\nsecond paragraph";
        let old_blocks = parse_blocks(old_text);
        assert_eq!(old_blocks.len(), 2);

        // Concurrent edits: replica A appends a third paragraph; replica B tweaks
        // the first. Merge the *text*, then re-parse + re-key.
        let mut a = TextCrdt::from_str(1, old_text);
        let mut b = a.fork(2);
        a.insert_str(a.len(), "\n\nthird paragraph");
        b.insert_str(5, " EDITED"); // into "first"
        a.merge(&b);

        let new_blocks = parse_blocks(&a.text());
        assert_eq!(new_blocks.len(), 3, "merged text re-parses to 3 paragraphs");

        // The keyed projection: the unchanged second paragraph keeps its key
        // (identity through the merge); edited/new blocks are edits/inserts.
        let keys = assign_stable_keys(&old_blocks, &new_blocks);
        let old_keys: Vec<String> = old_blocks
            .iter()
            .map(|bl| crate::stable_id::block_key(bl).as_string())
            .collect();
        assert!(
            keys.contains(&old_keys[1]),
            "unchanged paragraph keeps identity across the text-CRDT merge"
        );
    }

    #[test]
    fn gc_collects_a_stable_deleted_leaf() {
        let mut t = TextCrdt::from_str(1, "abc");
        t.delete(2); // tombstone the trailing 'c' (a leaf: nothing follows it)
        assert_eq!(t.text(), "ab");
        assert_eq!(t.tombstone_count(), 1);
        // Nothing stable -> nothing collected.
        assert_eq!(t.gc_with(|_| false), 0);
        assert_eq!(t.tombstone_count(), 1);
        // Stable -> the leaf tombstone is reclaimed; visible text is unchanged.
        assert_eq!(t.gc_with(|_| true), 1);
        assert_eq!(t.tombstone_count(), 0);
        assert_eq!(t.text(), "ab");
    }

    #[test]
    fn gc_keeps_a_referenced_tombstone_then_collects_bottom_up() {
        // Delete the MIDDLE char: 'b' is the left-origin of 'c', so collecting it
        // would orphan 'c'. GC must keep it until 'c' is gone too.
        let mut t = TextCrdt::from_str(1, "abc");
        t.delete(1); // tombstone 'b'; 'c' still references it as origin
        assert_eq!(t.text(), "ac");
        assert_eq!(
            t.gc_with(|_| true),
            0,
            "referenced tombstone is not collected"
        );
        assert_eq!(t.tombstone_count(), 1);
        assert_eq!(
            t.text(),
            "ac",
            "live text intact while tombstone is retained"
        );

        // Now delete 'c' too. One GC pass collects 'c' (leaf), which un-references
        // 'b', so the same pass then collects 'b' bottom-up.
        t.delete(1); // visible index of 'c' is now 1
        assert_eq!(t.text(), "a");
        assert_eq!(
            t.gc_with(|_| true),
            2,
            "both tombstones collected bottom-up"
        );
        assert_eq!(t.tombstone_count(), 0);
        assert_eq!(t.text(), "a");
    }

    #[test]
    fn concurrent_deletes_of_same_char_converge() {
        // Both replicas delete the same character; the sticky tombstone must
        // converge regardless of merge order (commutative).
        let mut a = TextCrdt::from_str(1, "abc");
        let mut b = a.fork(2);
        a.delete(1); // 'b' on replica 1
        b.delete(1); // 'b' on replica 2 (concurrent, distinct delete OpIds)
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab.text(), "ac");
        assert_eq!(ba.text(), "ac");
        assert_eq!(ab.tombstone_count(), ba.tombstone_count());
        // The converged delete id is the same on both (min of the two) -> GC
        // stability is order-independent.
        ab.merge(&ba);
        ba.merge(&ab);
        assert_eq!(ab.text(), ba.text());
    }
}
