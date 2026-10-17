//! Grid-based marker clustering (SPEC §6.14).

use crate::mercator::lonlat_to_world;
use lb_core::{GeoPoint, PhotoId};
use std::collections::BTreeMap;

/// A geotagged photo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    /// The photo.
    pub id: PhotoId,
    /// Where it was taken.
    pub point: GeoPoint,
}

/// Markers that fall in one grid cell at the current zoom.
#[derive(Debug, Clone, PartialEq)]
pub struct Cluster {
    /// Mean position of the members.
    pub center: GeoPoint,
    /// Members in input order.
    pub ids: Vec<PhotoId>,
}

impl Cluster {
    /// Number of photos.
    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

/// Groups `markers` into square cells of `cell_px` pixels at zoom `z`. Clusters come out ordered
/// by cell (north to south, west to east) so the result is deterministic.
pub fn cluster_markers(markers: &[Marker], z: u8, cell_px: f64) -> Vec<Cluster> {
    let cell_px = cell_px.max(1.0);
    let mut cells: BTreeMap<(i64, i64), Vec<&Marker>> = BTreeMap::new();
    for m in markers {
        let (x, y) = lonlat_to_world(m.point, z);
        cells
            .entry(((y / cell_px).floor() as i64, (x / cell_px).floor() as i64))
            .or_default()
            .push(m);
    }
    cells
        .into_values()
        .map(|members| {
            let n = members.len() as f64;
            let lat = members.iter().map(|m| m.point.lat).sum::<f64>() / n;
            let lon = members.iter().map(|m| m.point.lon).sum::<f64>() / n;
            Cluster {
                center: GeoPoint { lat, lon },
                ids: members.iter().map(|m| m.id).collect(),
            }
        })
        .collect()
}
