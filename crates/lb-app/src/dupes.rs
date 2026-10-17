//! Finding exact and visually similar photos (SPEC §6.11).

use crate::error::Result;
use crate::lightbox::Lightbox;
use lb_core::{AlbumId, MediaType, Photo, PhotoId, PhotoStatus};
use lb_media::{BkTree, content_hash, phash_file};
use std::collections::{BTreeMap, HashMap};

/// Default Hamming distance up to which two photos count as similar.
pub const DEFAULT_SIMILARITY: u32 = 6;

/// Photos that are the same picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateGroup {
    /// Members, ascending by id.
    pub ids: Vec<PhotoId>,
    /// Every member has the same file content; otherwise they only look alike.
    pub exact: bool,
}

/// Duplicate detection and review actions.
#[derive(Clone)]
pub struct DuplicatesService {
    lb: Lightbox,
}

impl Lightbox {
    /// Duplicate detection.
    pub fn duplicates(&self) -> DuplicatesService {
        DuplicatesService { lb: self.clone() }
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb.max(ra)] = ra.min(rb);
        }
    }
}

impl DuplicatesService {
    /// Groups the library's readable images: same `content_hash`, or `phash` within `threshold`
    /// bits (union-find over both). Hashes missing from the catalog are computed and stored.
    /// Groups are ordered by their lowest id.
    pub fn find(&self, threshold: u32) -> Result<Vec<DuplicateGroup>> {
        let catalog = self.lb.catalog();
        let photos: Vec<Photo> = self
            .lb
            .photos()
            .all()?
            .into_iter()
            .filter(|p| p.media_type == MediaType::Image && p.status == PhotoStatus::Ok)
            .collect();
        let n = photos.len();
        let mut uf = UnionFind::new(n);

        let mut by_key: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, p) in photos.iter().enumerate() {
            by_key.entry(p.quick_key.as_str()).or_default().push(i);
        }
        let mut hashes: Vec<Option<String>> =
            photos.iter().map(|p| p.content_hash.clone()).collect();
        for members in by_key.values().filter(|m| m.len() > 1) {
            let mut by_hash: HashMap<String, usize> = HashMap::new();
            for &i in members {
                if hashes[i].is_none()
                    && let Ok(h) = content_hash(&photos[i].path)
                {
                    catalog.set_content_hash(photos[i].id, &h)?;
                    hashes[i] = Some(h);
                }
                if let Some(h) = &hashes[i] {
                    match by_hash.get(h) {
                        Some(&first) => uf.union(first, i),
                        None => {
                            by_hash.insert(h.clone(), i);
                        }
                    }
                }
            }
        }

        let stored: HashMap<PhotoId, Option<u64>> = catalog
            .photo_hashes()?
            .into_iter()
            .map(|h| (h.id, h.phash))
            .collect();
        let mut tree: BkTree<usize> = BkTree::new();
        let mut phashes: Vec<Option<u64>> = Vec::with_capacity(n);
        for p in &photos {
            let hash = match stored.get(&p.id).copied().flatten() {
                Some(h) => Some(h),
                None => match phash_file(&p.path) {
                    Ok(h) => {
                        catalog.set_phash(p.id, h)?;
                        Some(h)
                    }
                    Err(_) => None,
                },
            };
            phashes.push(hash);
        }
        for (i, hash) in phashes.iter().enumerate() {
            let Some(hash) = hash else { continue };
            for &j in tree.find(*hash, threshold) {
                uf.union(i, j);
            }
            tree.insert(*hash, i);
        }

        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for i in 0..n {
            let root = uf.find(i);
            groups.entry(root).or_default().push(i);
        }
        let mut out: Vec<DuplicateGroup> = groups
            .into_values()
            .filter(|m| m.len() > 1)
            .map(|members| {
                let first = hashes[members[0]].as_ref();
                let exact = first.is_some() && members.iter().all(|&i| hashes[i].as_ref() == first);
                DuplicateGroup {
                    ids: members.iter().map(|&i| photos[i].id).collect(),
                    exact,
                }
            })
            .collect();
        out.sort_by_key(|g| g.ids[0]);
        Ok(out)
    }

    /// Keeps `keep` and moves the other members of `group` to the trash.
    pub fn keep_one(&self, group: &DuplicateGroup, keep: PhotoId) -> Result<crate::DeleteReport> {
        let others: Vec<PhotoId> = group.ids.iter().copied().filter(|i| *i != keep).collect();
        self.lb.photos().delete(&others)
    }

    /// Stars `best` (the one worth keeping) and nothing else.
    pub fn star_best(&self, best: PhotoId) -> Result<()> {
        self.lb.photos().set_starred(&[best], true)?;
        Ok(())
    }

    /// Adds every member of `group` to `album`.
    pub fn add_group_to_album(&self, group: &DuplicateGroup, album: AlbumId) -> Result<usize> {
        self.lb.albums().add_photos(album, &group.ids)
    }
}
