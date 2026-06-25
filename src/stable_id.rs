//! Manufactured identity for markdown text (#lzstableid).
//!
//! Plain markdown has no node ids, so keyed reconciliation (`#lzkeyrecon`) has
//! nothing to match on. This module *manufactures* stable identity from text, in
//! three layers of decreasing certainty:
//!
//! 1. **Anchored ids** — an in-band marker/id on a block (agent-doc already emits
//!    these). Exact, and survives an arbitrary rewrite of the block's body.
//! 2. **Content-derived keys** — a hash of the block's *normalized* text, so an
//!    unchanged block keeps its key across reflow/rewrap/reorder even with no
//!    anchor.
//! 3. **Alignment** — for a block whose content changed (no exact match), match
//!    it to a predecessor by **similarity** (word-LCS ratio, à la
//!    Myers/patience/histogram diffs) so an *edit* is distinguished from a real
//!    *insert*. A true rewrite legitimately reads as insert+remove — there is no
//!    identity left to preserve.
//!
//! [`assign_stable_keys`] is the bridge to `#lzkeyrecon`: it returns one stable
//! key per new block, reusing a matched/edited block's key so identity flows
//! through an edit (the reconciler then emits `Update`, not remove+insert).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A text block, optionally carrying an in-band anchor/id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// In-band stable id, if the source provides one (e.g. an agent-doc marker).
    pub anchor: Option<String>,
    /// The block's raw text.
    pub text: String,
}

impl Block {
    /// A block with no anchor.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            anchor: None,
            text: text.into(),
        }
    }

    /// A block with an in-band anchor id.
    pub fn anchored(anchor: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            anchor: Some(anchor.into()),
            text: text.into(),
        }
    }
}

/// A manufactured identity key for a block: an anchor id, or a content hash of
/// the normalized text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKey {
    /// From an in-band anchor — survives a full rewrite of the block body.
    Anchored(String),
    /// Hash of normalized content — survives reflow/reorder, changes on edit.
    Content(u64),
}

impl BlockKey {
    /// A stable string form usable as a reconciliation key (`#lzkeyrecon`).
    pub fn as_string(&self) -> String {
        match self {
            BlockKey::Anchored(a) => format!("a:{a}"),
            BlockKey::Content(h) => format!("c:{h:016x}"),
        }
    }
}

/// Normalize a block's text so reflow/rewrap/indent changes don't change its
/// content key: collapse all whitespace runs to single spaces and trim.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn content_hash(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    normalize(text).hash(&mut h);
    h.finish()
}

/// The identity key for a block: its anchor if present, else a content hash.
pub fn block_key(b: &Block) -> BlockKey {
    match &b.anchor {
        Some(a) => BlockKey::Anchored(a.clone()),
        None => BlockKey::Content(content_hash(&b.text)),
    }
}

/// How a new block relates to the old sequence.
#[derive(Debug, Clone, PartialEq)]
pub enum Match {
    /// Exact key match (anchor or content hash) — identity preserved. A position
    /// change makes it a move; the content is unchanged (or anchored).
    Same { old: usize },
    /// Matched to a predecessor by similarity; the content changed (an edit).
    Edited { old: usize, similarity: f32 },
    /// No match — a genuine insertion.
    Inserted,
}

/// The alignment of a new block sequence against an old one.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// One entry per new block, in order.
    pub new_matches: Vec<Match>,
    /// Old block indices that were not matched (removed).
    pub removed: Vec<usize>,
}

/// Word-LCS similarity ratio in `[0,1]`: `2·|LCS| / (|a|+|b|)` over whitespace
/// tokens (the difflib/Myers-style ratio). 1.0 = identical token sequence.
pub fn similarity(a: &str, b: &str) -> f32 {
    let aw: Vec<&str> = a.split_whitespace().collect();
    let bw: Vec<&str> = b.split_whitespace().collect();
    if aw.is_empty() && bw.is_empty() {
        return 1.0;
    }
    let lcs = lcs_len(&aw, &bw);
    (2 * lcs) as f32 / (aw.len() + bw.len()) as f32
}

fn lcs_len(a: &[&str], b: &[&str]) -> usize {
    let mut dp = vec![0usize; b.len() + 1];
    for &x in a {
        let mut prev = 0; // dp[j-1] from the previous row
        for (j, &y) in b.iter().enumerate() {
            let cur = dp[j + 1];
            dp[j + 1] = if x == y {
                prev + 1
            } else {
                dp[j + 1].max(dp[j])
            };
            prev = cur;
        }
    }
    dp[b.len()]
}

/// Minimum similarity for an unmatched block to be classified as an `Edited`
/// predecessor rather than a fresh `Inserted` block.
pub const EDIT_THRESHOLD: f32 = 0.5;

/// Align `new` against `old`, manufacturing identity. Exact key matches (anchor,
/// then content hash) carry identity directly; remaining new blocks are matched
/// to the most-similar unmatched old block above [`EDIT_THRESHOLD`] (nearest
/// index breaks ties) and classified `Edited`, else `Inserted`. Unmatched old
/// blocks are `removed`.
pub fn align(old: &[Block], new: &[Block]) -> Alignment {
    let old_keys: Vec<BlockKey> = old.iter().map(block_key).collect();
    let new_keys: Vec<BlockKey> = new.iter().map(block_key).collect();
    let mut old_used = vec![false; old.len()];
    let mut new_matches: Vec<Option<Match>> = vec![None; new.len()];

    // Pass 1: exact key match in order (anchor or content hash). Equal content
    // blocks are consumed left-to-right so duplicates pair up deterministically.
    for (ni, nk) in new_keys.iter().enumerate() {
        if let Some(oi) = (0..old.len()).find(|&oi| !old_used[oi] && &old_keys[oi] == nk) {
            old_used[oi] = true;
            new_matches[ni] = Some(Match::Same { old: oi });
        }
    }

    // Pass 2: similarity match for the still-unmatched new blocks.
    for (ni, slot) in new_matches.iter_mut().enumerate() {
        if slot.is_some() {
            continue;
        }
        let mut best: Option<(usize, f32)> = None;
        for (oi, used) in old_used.iter().enumerate() {
            if *used {
                continue;
            }
            let sim = similarity(&new[ni].text, &old[oi].text);
            let better = match best {
                None => true,
                Some((bi, bs)) => {
                    sim > bs
                        || (sim == bs
                            && (oi as isize - ni as isize).abs()
                                < (bi as isize - ni as isize).abs())
                }
            };
            if better {
                best = Some((oi, sim));
            }
        }
        match best {
            Some((oi, sim)) if sim >= EDIT_THRESHOLD => {
                old_used[oi] = true;
                *slot = Some(Match::Edited {
                    old: oi,
                    similarity: sim,
                });
            }
            _ => *slot = Some(Match::Inserted),
        }
    }

    let removed = (0..old.len()).filter(|&oi| !old_used[oi]).collect();
    Alignment {
        new_matches: new_matches.into_iter().map(|m| m.unwrap()).collect(),
        removed,
    }
}

/// One stable key per **new** block, suitable as the `#lzkeyrecon` key set.
///
/// A `Same`/`Edited` block reuses its matched old block's key so identity flows
/// through an edit (the reconciler emits `Update`, not remove+insert). An
/// `Inserted` block gets its own anchor/content key.
pub fn assign_stable_keys(old: &[Block], new: &[Block]) -> Vec<String> {
    let old_keys: Vec<String> = old.iter().map(|b| block_key(b).as_string()).collect();
    let alignment = align(old, new);
    alignment
        .new_matches
        .iter()
        .enumerate()
        .map(|(ni, m)| match m {
            Match::Same { old } | Match::Edited { old, .. } => old_keys[*old].clone(),
            Match::Inserted => block_key(&new[ni]).as_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_key_survives_reflow_but_not_edit() {
        let a = Block::text("the quick brown fox");
        let b = Block::text("the   quick\n  brown   fox\n"); // reflowed whitespace
        let c = Block::text("the quick red fox"); // edited word
        assert_eq!(block_key(&a), block_key(&b), "reflow keeps content key");
        assert_ne!(block_key(&a), block_key(&c), "edit changes content key");
    }

    #[test]
    fn anchored_key_survives_full_rewrite() {
        let a = Block::anchored("item-1", "original body");
        let b = Block::anchored("item-1", "completely different prose now");
        assert_eq!(block_key(&a), block_key(&b), "anchor survives rewrite");
    }

    #[test]
    fn pure_reorder_is_all_same_no_removed() {
        let old = [
            Block::text("alpha"),
            Block::text("beta"),
            Block::text("gamma"),
        ];
        let new = [
            Block::text("gamma"),
            Block::text("alpha"),
            Block::text("beta"),
        ];
        let al = align(&old, &new);
        assert!(
            al.new_matches
                .iter()
                .all(|m| matches!(m, Match::Same { .. }))
        );
        assert!(al.removed.is_empty());
    }

    #[test]
    fn small_edit_is_detected_as_edited_not_insert_remove() {
        let old = [Block::text("the quick brown fox jumps over the lazy dog")];
        let new = [Block::text("the quick brown fox jumps over the sleepy dog")];
        let al = align(&old, &new);
        match al.new_matches[0] {
            Match::Edited { old: 0, similarity } => assert!(similarity > 0.5),
            ref other => panic!("expected Edited, got {other:?}"),
        }
        assert!(al.removed.is_empty(), "edited block is not a removal");
    }

    #[test]
    fn genuine_insert_and_remove() {
        let old = [Block::text("keep me"), Block::text("delete me entirely")];
        let new = [
            Block::text("keep me"),
            Block::text("brand new unrelated content here"),
        ];
        let al = align(&old, &new);
        assert!(matches!(al.new_matches[0], Match::Same { old: 0 }));
        assert!(
            matches!(al.new_matches[1], Match::Inserted),
            "dissimilar block must be Inserted, got {:?}",
            al.new_matches[1]
        );
        assert_eq!(al.removed, vec![1]);
    }

    #[test]
    fn assign_stable_keys_flows_identity_through_edit() {
        let old = [
            Block::text("first paragraph stays the same"),
            Block::text("second paragraph will be tweaked a little"),
        ];
        let new = [
            // reordered + the second edited.
            Block::text("second paragraph will be tweaked a bit"),
            Block::text("first paragraph stays the same"),
        ];
        let old_keys: Vec<String> = old.iter().map(|b| block_key(b).as_string()).collect();
        let new_keys = assign_stable_keys(&old, &new);
        // The edited block reuses old block 1's key; the unchanged block reuses 0's.
        assert_eq!(new_keys[0], old_keys[1], "edited block keeps its identity");
        assert_eq!(new_keys[1], old_keys[0]);
    }

    #[test]
    fn anchored_blocks_align_by_id_through_rewrite() {
        let old = [
            Block::anchored("h1", "Heading one body"),
            Block::anchored("h2", "Heading two body"),
        ];
        let new = [
            Block::anchored("h2", "Heading two REWRITTEN body"),
            Block::anchored("h1", "Heading one body"),
        ];
        let al = align(&old, &new);
        // h2 moved + rewritten but still matches by anchor (Same, not Edited).
        assert!(matches!(al.new_matches[0], Match::Same { old: 1 }));
        assert!(matches!(al.new_matches[1], Match::Same { old: 0 }));
        assert!(al.removed.is_empty());
    }
}
