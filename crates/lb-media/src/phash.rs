//! Perceptual hashing and Hamming-distance search (SPEC §6.11).

use crate::{Result, decode, resize_exact};
use image::DynamicImage;
use std::path::Path;

/// 64-bit gradient hash: the image is reduced to 9×8 grey pixels and each bit says whether a
/// pixel is brighter than its right neighbour. Resized and re-compressed copies differ in only a
/// few bits, unrelated images in about half of them.
pub fn phash(img: &DynamicImage) -> u64 {
    let small = resize_exact(img, 9, 8).to_luma8();
    let mut hash = 0_u64;
    for y in 0..8 {
        for x in 0..8 {
            hash <<= 1;
            if small.get_pixel(x, y).0[0] > small.get_pixel(x + 1, y).0[0] {
                hash |= 1;
            }
        }
    }
    hash
}

/// [`phash`] of the file's upright image, decoded at reduced size.
///
/// # Errors
/// Fails exactly when the file cannot be decoded.
pub fn phash_file(path: &Path) -> Result<u64> {
    Ok(phash(&decode(path, Some(256))?))
}

/// Number of differing bits.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// A BK-tree over 64-bit hashes with Hamming distance (SPEC §6.11).
#[derive(Debug, Default, Clone)]
pub struct BkTree<T> {
    nodes: Vec<Node<T>>,
}

#[derive(Debug, Clone)]
struct Node<T> {
    hash: u64,
    value: T,
    children: Vec<(u32, usize)>,
}

impl<T> BkTree<T> {
    /// An empty tree.
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree holds nothing.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Adds an entry; equal hashes may repeat.
    pub fn insert(&mut self, hash: u64, value: T) {
        let new = self.nodes.len();
        if self.nodes.is_empty() {
            self.nodes.push(Node {
                hash,
                value,
                children: Vec::new(),
            });
            return;
        }
        let mut at = 0;
        loop {
            let d = hamming(hash, self.nodes[at].hash);
            match self.nodes[at].children.iter().find(|(cd, _)| *cd == d) {
                Some(&(_, child)) => at = child,
                None => {
                    self.nodes[at].children.push((d, new));
                    self.nodes.push(Node {
                        hash,
                        value,
                        children: Vec::new(),
                    });
                    return;
                }
            }
        }
    }

    /// Every entry within `radius` bits of `hash`, in no particular order.
    pub fn find(&self, hash: u64, radius: u32) -> Vec<&T> {
        let mut out = Vec::new();
        if self.nodes.is_empty() {
            return out;
        }
        let mut stack = vec![0_usize];
        while let Some(at) = stack.pop() {
            let node = &self.nodes[at];
            let d = hamming(hash, node.hash);
            if d <= radius {
                out.push(&node.value);
            }
            let (lo, hi) = (d.saturating_sub(radius), d + radius);
            stack.extend(
                node.children
                    .iter()
                    .filter(|(cd, _)| (lo..=hi).contains(cd))
                    .map(|&(_, child)| child),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn bk_tree_matches_bruteforce(
            hashes in proptest::collection::vec(any::<u64>(), 0..120),
            queries in proptest::collection::vec(any::<u64>(), 1..8),
            radius in 0_u32..=64,
        ) {
            let mut tree = BkTree::new();
            for (i, h) in hashes.iter().enumerate() {
                tree.insert(*h, i);
            }
            for q in queries {
                let mut got: Vec<usize> = tree.find(q, radius).into_iter().copied().collect();
                got.sort_unstable();
                let want: Vec<usize> = hashes
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| hamming(q, **h) <= radius)
                    .map(|(i, _)| i)
                    .collect();
                prop_assert_eq!(got, want);
            }
        }

        #[test]
        fn bk_tree_clustered_hashes(
            base in any::<u64>(),
            flips in proptest::collection::vec(proptest::collection::vec(0_u32..64, 0..8), 1..60),
            radius in 0_u32..=10,
        ) {
            let hashes: Vec<u64> = flips
                .iter()
                .map(|f| f.iter().fold(base, |h, bit| h ^ (1 << bit)))
                .collect();
            let mut tree = BkTree::new();
            for (i, h) in hashes.iter().enumerate() {
                tree.insert(*h, i);
            }
            let mut got: Vec<usize> = tree.find(base, radius).into_iter().copied().collect();
            got.sort_unstable();
            let want: Vec<usize> = hashes
                .iter()
                .enumerate()
                .filter(|(_, h)| hamming(base, **h) <= radius)
                .map(|(i, _)| i)
                .collect();
            prop_assert_eq!(got, want);
        }
    }
}
