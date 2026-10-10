// SPDX-License-Identifier: AGPL-3.0-or-later
//! Independent randomized public-API verification of the indexed tree.
#![forbid(unsafe_code)]
use atelier_model::{IndexedTree, NodeId};
use std::collections::BTreeSet;

fn main() {
    for seed in 1..=12_u64 {
        let mut state = seed;
        let mut tree = IndexedTree::new();
        let mut oracle: Vec<(NodeId, u64)> = Vec::new();
        let mut issued = BTreeSet::new();
        for step in 0..20_000_u64 {
            state = state.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
            if oracle.is_empty() || state % 100 < 52 {
                let position = ((state >> 16) as usize) % (oracle.len() + 1);
                let id = tree.insert(position, step).unwrap();
                assert!(issued.insert(id), "reused identity");
                oracle.insert(position, (id, step));
            } else {
                let position = ((state >> 16) as usize) % oracle.len();
                let (id, value) = oracle.remove(position);
                assert_eq!(tree.remove(position), Ok(value));
                assert_eq!(tree.get_by_id(id), None);
                assert_eq!(tree.position_of(id), None);
            }
            assert_eq!(tree.len(), oracle.len());
            if step % 97 == 0 {
                for (position, &(id, value)) in oracle.iter().enumerate() {
                    assert_eq!(tree.get(position), Some(&value));
                    assert_eq!(tree.get_by_id(id), Some(&value));
                    assert_eq!(tree.position_of(id), Some(position));
                }
                assert_eq!(tree.get(usize::MAX), None);
                assert!(tree.remove(usize::MAX).is_err());
                assert!(tree.insert(usize::MAX, 0).is_err());
                assert_eq!(tree.len(), oracle.len());
            }
        }
        while let Some((id, value)) = oracle.pop() {
            assert_eq!(tree.remove(oracle.len()), Ok(value));
            assert_eq!(tree.get_by_id(id), None);
        }
        assert!(tree.is_empty());
        assert!(issued.insert(tree.insert(0, 0).unwrap()));
    }
    let mut strings = IndexedTree::new();
    let id = strings.insert(0, String::from("owned payload")).unwrap();
    assert_eq!(strings.get_by_id(id).map(String::as_str), Some("owned payload"));
    assert_eq!(strings.remove(0).unwrap(), "owned payload");
    println!("independent Vec oracle: 12 seeds, 240000 mixed operations passed");
}
