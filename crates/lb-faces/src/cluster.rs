//! Incremental and agglomerative clustering of face embeddings (SPEC §6.15).

use std::collections::BTreeMap;

/// Cosine similarity at or above which two faces count as the same person.
pub const DEFAULT_THRESHOLD: f32 = 0.55;

/// Dot product over the common length.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cosine similarity; 0 when either vector is zero.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (na, nb) = (dot(a, a).sqrt(), dot(b, b).sqrt());
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

/// Scales `v` to unit length. Returns `false` (leaving `v` untouched) for zero or non-finite
/// vectors.
pub fn l2_normalize(v: &mut [f32]) -> bool {
    let norm = dot(v, v).sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return false;
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
    true
}

struct Accumulator {
    sum: Vec<f32>,
    count: usize,
}

impl Accumulator {
    fn add(&mut self, emb: &[f32]) {
        if self.sum.len() < emb.len() {
            self.sum.resize(emb.len(), 0.0);
        }
        for (s, e) in self.sum.iter_mut().zip(emb) {
            *s += e;
        }
        self.count += 1;
    }
}

/// Cluster centroids that new faces are matched against.
pub struct ClusterIndex {
    clusters: BTreeMap<i64, Accumulator>,
    next_id: i64,
}

impl ClusterIndex {
    /// An index without clusters; the first new cluster gets id 1.
    pub fn new() -> Self {
        Self {
            clusters: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Builds the index from stored faces: `(cluster id, unit embedding)`. New clusters get ids
    /// above `max_used_id`.
    pub fn from_members<'a>(
        members: impl IntoIterator<Item = (i64, &'a [f32])>,
        max_used_id: i64,
    ) -> Self {
        let mut index = Self::new();
        for (id, emb) in members {
            index
                .clusters
                .entry(id)
                .or_insert_with(|| Accumulator {
                    sum: Vec::new(),
                    count: 0,
                })
                .add(emb);
        }
        let highest = index.clusters.keys().next_back().copied().unwrap_or(0);
        index.next_id = highest.max(max_used_id) + 1;
        index
    }

    /// Number of clusters.
    pub fn len(&self) -> usize {
        self.clusters.len()
    }

    /// Whether there are no clusters.
    pub fn is_empty(&self) -> bool {
        self.clusters.is_empty()
    }

    /// The cluster whose centroid is most similar to `emb` (a unit vector), with that similarity.
    /// Ties go to the lowest id.
    pub fn nearest(&self, emb: &[f32]) -> Option<(i64, f32)> {
        let mut best: Option<(i64, f32)> = None;
        for (id, acc) in &self.clusters {
            let sim = cosine(&acc.sum, emb);
            if best.is_none_or(|(_, b)| sim > b) {
                best = Some((*id, sim));
            }
        }
        best
    }

    /// Adds a face: it joins the nearest cluster when the similarity reaches `threshold`,
    /// otherwise it starts a new one. Returns the cluster id.
    pub fn assign(&mut self, emb: &[f32], threshold: f32) -> i64 {
        let id = match self.nearest(emb) {
            Some((id, sim)) if sim >= threshold => id,
            _ => {
                let id = self.next_id;
                self.next_id += 1;
                id
            }
        };
        self.clusters
            .entry(id)
            .or_insert_with(|| Accumulator {
                sum: Vec::new(),
                count: 0,
            })
            .add(emb);
        id
    }

    /// Reserves a fresh cluster id.
    pub fn fresh_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Number of faces in a cluster.
    pub fn size(&self, id: i64) -> usize {
        self.clusters.get(&id).map_or(0, |a| a.count)
    }
}

impl Default for ClusterIndex {
    fn default() -> Self {
        Self::new()
    }
}

/// Renumbers arbitrary labels to `0..k` in order of first appearance.
pub fn stable_labels(labels: &[usize]) -> Vec<usize> {
    let mut map = BTreeMap::new();
    labels
        .iter()
        .map(|l| {
            let next = map.len();
            *map.entry(*l).or_insert(next)
        })
        .collect()
}

/// Average-linkage agglomerative clustering: repeatedly merges the two clusters with the highest
/// mean pairwise cosine similarity while it is at least `threshold`. Returns one label per input,
/// numbered `0..k` by first appearance. Memory and time are quadratic in `embeddings.len()`.
pub fn agglomerate(embeddings: &[Vec<f32>], threshold: f32) -> Vec<usize> {
    let n = embeddings.len();
    let mut sim = vec![0.0f32; n * n];
    for i in 0..n {
        for j in (i + 1)..n {
            let s = cosine(&embeddings[i], &embeddings[j]);
            sim[i * n + j] = s;
            sim[j * n + i] = s;
        }
    }
    let mut active = vec![true; n];
    let mut size = vec![1usize; n];
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let row_best = |sim: &[f32], active: &[bool], i: usize| -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for j in 0..n {
            if j != i && active[j] && best.is_none_or(|(_, b)| sim[i * n + j] > b) {
                best = Some((j, sim[i * n + j]));
            }
        }
        best
    };
    let mut best: Vec<Option<(usize, f32)>> = (0..n).map(|i| row_best(&sim, &active, i)).collect();
    loop {
        let mut pick: Option<(usize, usize, f32)> = None;
        for i in 0..n {
            if let (true, Some((j, s))) = (active[i], best[i])
                && pick.is_none_or(|(_, _, b)| s > b)
            {
                pick = Some((i, j, s));
            }
        }
        let Some((a, b, s)) = pick else { break };
        if s < threshold {
            break;
        }
        let (i, j) = (a.min(b), a.max(b));
        let (ni, nj) = (size[i] as f32, size[j] as f32);
        for k in 0..n {
            if k != i && k != j && active[k] {
                let merged = (ni * sim[i * n + k] + nj * sim[j * n + k]) / (ni + nj);
                sim[i * n + k] = merged;
                sim[k * n + i] = merged;
            }
        }
        active[j] = false;
        size[i] += size[j];
        let moved = std::mem::take(&mut members[j]);
        members[i].extend(moved);
        best[i] = row_best(&sim, &active, i);
        for k in 0..n {
            if !active[k] || k == i {
                continue;
            }
            match best[k] {
                Some((t, _)) if t == i || t == j => best[k] = row_best(&sim, &active, k),
                Some((_, bs)) if sim[k * n + i] > bs => best[k] = Some((i, sim[k * n + i])),
                None => best[k] = row_best(&sim, &active, k),
                _ => {}
            }
        }
    }
    let mut labels = vec![0usize; n];
    for (rep, group) in members.iter().enumerate() {
        for m in group {
            labels[*m] = rep;
        }
    }
    stable_labels(&labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(angle: f32) -> Vec<f32> {
        vec![angle.cos(), angle.sin()]
    }

    #[test]
    fn cosine_and_normalize() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 1.0]) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
        let mut v = vec![3.0, 4.0];
        assert!(l2_normalize(&mut v));
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
        let mut z = vec![0.0, 0.0];
        assert!(!l2_normalize(&mut z));
    }

    #[test]
    fn incremental_assignment_joins_or_creates() {
        let mut index = ClusterIndex::new();
        let a = index.assign(&unit(0.0), DEFAULT_THRESHOLD);
        let b = index.assign(&unit(0.2), DEFAULT_THRESHOLD);
        let c = index.assign(&unit(1.5), DEFAULT_THRESHOLD);
        assert_eq!(a, b, "close embeddings share a cluster");
        assert_ne!(a, c, "distant embeddings start a new cluster");
        assert_eq!(index.len(), 2);
        assert_eq!(index.size(a), 2);
        let existing = ClusterIndex::from_members([(7, unit(0.0).as_slice())], 9);
        assert_eq!(existing.nearest(&unit(0.1)).map(|n| n.0), Some(7));
        let mut existing = existing;
        assert_eq!(existing.fresh_id(), 10);
    }

    #[test]
    fn agglomerate_groups_by_average_linkage() {
        let items = vec![unit(0.0), unit(1.6), unit(0.1), unit(1.7), unit(3.2)];
        assert_eq!(agglomerate(&items, DEFAULT_THRESHOLD), vec![0, 1, 0, 1, 2]);
        assert!(agglomerate(&[], DEFAULT_THRESHOLD).is_empty());
        assert_eq!(agglomerate(&[unit(0.0)], DEFAULT_THRESHOLD), vec![0]);
    }

    #[test]
    fn average_linkage_does_not_chain() {
        let chain: Vec<Vec<f32>> = (0..6).map(|i| unit(i as f32 * 0.5)).collect();
        let labels = agglomerate(&chain, DEFAULT_THRESHOLD);
        let distinct = labels.iter().max().copied().unwrap_or(0) + 1;
        assert!(distinct >= 2, "a long chain is split: {labels:?}");
        assert_eq!(stable_labels(&[5, 5, 2, 5, 2, 9]), vec![0, 0, 1, 0, 1, 2]);
    }
}
