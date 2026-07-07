//! Property tests for the lossless tree CRDT (#lzlosstree): random structural +
//! text schedules must preserve the lossless render invariant and converge
//! regardless of delivery order.
//!
//! - `shuffled_delivery_converges`: a single author builds a random valid op
//!   sequence; a second replica receives those ops in a shuffled, one-at-a-time
//!   order (exercising apply-time causal buffering + idempotence) and must
//!   converge to identical rendered text and an identical dotted frontier.
//! - `split_then_merge_preserves_render`: splitting a leaf at any char boundary
//!   and merging it back is exactly render-preserving (independent oracle: the
//!   original text).

#![cfg(feature = "lossless-tree")]

use lazily::{LeafKind, LosslessTreeCrdt, NodeSeed, TreeNodeId, TreeUpdate, TreeVersionFrontier};
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Action {
    Create(String),
    Edit(usize, char),
    Split(usize),
    Reorder(usize, usize),
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        "[a-zé!]{1,4}".prop_map(Action::Create),
        (any::<usize>(), prop::char::range('a', 'z')).prop_map(|(i, c)| Action::Edit(i, c)),
        any::<usize>().prop_map(Action::Split),
        (any::<usize>(), any::<usize>()).prop_map(|(i, j)| Action::Reorder(i, j)),
    ]
}

fn raw(text: &str) -> NodeSeed {
    NodeSeed::Leaf {
        kind: LeafKind::Raw,
        text: text.to_string(),
    }
}

/// A byte offset of the `k`-th char boundary of `s` (clamped to `s.len()`).
fn char_boundary(s: &str, k: usize) -> usize {
    s.char_indices().map(|(b, _)| b).nth(k).unwrap_or(s.len())
}

/// Build a replica by interpreting `actions` on a single `para` element; every
/// choice is made valid against current live state, so all ops apply.
fn build(actions: &[Action]) -> LosslessTreeCrdt {
    let mut t = LosslessTreeCrdt::new(1);
    let para = t
        .create_node(
            TreeNodeId::ROOT,
            None,
            NodeSeed::Element { kind: "p".into() },
        )
        .unwrap();
    t.create_node(para, None, raw("seed")).unwrap();

    for act in actions {
        let leaves = t.children(para);
        if leaves.is_empty() {
            continue;
        }
        match act {
            Action::Create(text) => {
                let last = leaves.last().copied();
                t.create_node(para, last, raw(text)).unwrap();
            }
            Action::Edit(i, c) => {
                let node = leaves[i % leaves.len()];
                let mut buf = [0u8; 4];
                t.edit_leaf(node, 0, 0, c.encode_utf8(&mut buf)).unwrap();
            }
            Action::Split(i) => {
                let node = leaves[i % leaves.len()];
                let s = t.leaf_text(node).unwrap();
                let nchars = s.chars().count();
                let at = char_boundary(&s, i % (nchars + 1));
                t.split_leaf(node, at).unwrap();
            }
            Action::Reorder(i, j) => {
                if leaves.len() >= 2 {
                    let node = leaves[i % leaves.len()];
                    let anchor = leaves[j % leaves.len()];
                    if node != anchor {
                        t.reorder_child(node, Some(anchor)).unwrap();
                    }
                }
            }
        }
    }
    t
}

/// A deterministic Fisher–Yates shuffle driven by a small LCG (no `rand` dep).
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed ^ 0x9e37_79b9_7f4a_7c15;
    for i in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        items.swap(i, j);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn shuffled_delivery_converges(actions in prop::collection::vec(action(), 0..40), seed in any::<u64>()) {
        let a = build(&actions);
        let mut all = a.diff(&TreeVersionFrontier::default()).ops;
        shuffle(&mut all, seed);

        let mut b = LosslessTreeCrdt::new(2);
        for op in &all {
            b.apply_update(&TreeUpdate { ops: vec![op.clone()] });
        }
        prop_assert_eq!(b.render(), a.render(), "converge under shuffled delivery");
        prop_assert_eq!(b.frontier(), a.frontier(), "frontiers converge");

        // Re-delivering every op is a no-op (idempotence).
        for op in &all {
            b.apply_update(&TreeUpdate { ops: vec![op.clone()] });
        }
        prop_assert_eq!(b.render(), a.render(), "idempotent re-delivery");
    }

    #[test]
    fn split_then_merge_preserves_render(text in "[a-zé A-Z0-9]{1,20}", k in any::<usize>()) {
        let mut t = LosslessTreeCrdt::new(1);
        let para = t
            .create_node(TreeNodeId::ROOT, None, NodeSeed::Element { kind: "p".into() })
            .unwrap();
        let leaf = t.create_node(para, None, raw(&text)).unwrap();
        let original = t.render();

        let nchars = text.chars().count();
        let at = char_boundary(&text, k % (nchars + 1));
        let tail = t.split_leaf(leaf, at).unwrap();
        prop_assert_eq!(t.render(), original.clone(), "split preserves render");
        t.merge_adjacent_leaves(leaf, tail).unwrap();
        prop_assert_eq!(t.render(), original.clone(), "merge restores render");
    }
}
