//! Map view services: markers, clusters, tiles and geotag editing (SPEC §6.14).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use lb_core::{GeoPoint, PhotoId};
use lb_geo::{
    CACHE_TTL, Cluster, HttpTileProvider, Marker, PlacedTile, TileCache, TileId, TileProvider,
    Viewport, cluster_markers, tiles_for_viewport,
};
use parking_lot::Mutex;
use std::sync::Arc;

pub use lb_geo::{ATTRIBUTION, MAX_ZOOM, MIN_ZOOM};

/// Cell size of the marker grid in pixels.
pub const CLUSTER_CELL_PX: f64 = 60.0;

/// Shared state of the map services inside [`Lightbox`].
pub(crate) struct MapState {
    injected: Option<Arc<dyn TileProvider>>,
    cache: Mutex<Option<(String, Arc<TileCache>)>>,
}

impl MapState {
    pub(crate) fn new(injected: Option<Arc<dyn TileProvider>>) -> Self {
        Self {
            injected,
            cache: Mutex::new(None),
        }
    }
}

/// Map operations.
#[derive(Clone)]
pub struct MapService {
    lb: Lightbox,
}

impl Lightbox {
    /// Markers, tiles and geotag editing.
    pub fn map(&self) -> MapService {
        MapService { lb: self.clone() }
    }
}

impl MapService {
    /// Markers for the geotagged photos among `ids`, in the order of `ids`.
    pub fn markers(&self, ids: &[PhotoId]) -> Result<Vec<Marker>> {
        Ok(self
            .lb
            .photos()
            .get_many(ids)?
            .into_iter()
            .filter_map(|p| p.gps.map(|point| Marker { id: p.id, point }))
            .collect())
    }

    /// Clusters of the geotagged photos among `ids` at the viewport's zoom.
    pub fn clusters(&self, ids: &[PhotoId], viewport: &Viewport) -> Result<Vec<Cluster>> {
        Ok(cluster_markers(
            &self.markers(ids)?,
            viewport.zoom,
            CLUSTER_CELL_PX,
        ))
    }

    /// The tiles that cover `viewport`.
    pub fn tiles(&self, viewport: &Viewport) -> Vec<PlacedTile> {
        tiles_for_viewport(viewport)
    }

    fn cache(&self) -> Result<Arc<TileCache>> {
        let state = &self.lb.inner.map;
        let url = self.lb.settings().load()?.map.tile_url;
        let mut slot = state.cache.lock();
        if let Some((cached_url, cache)) = slot.as_ref()
            && *cached_url == url
        {
            return Ok(cache.clone());
        }
        let provider: Arc<dyn TileProvider> = match &state.injected {
            Some(p) => p.clone(),
            None => Arc::new(HttpTileProvider::new(url.clone())),
        };
        let digest = lb_media::quick_key_of_bytes(url.as_bytes());
        let key = digest.get(..12).unwrap_or(&digest);
        let cache = Arc::new(TileCache::new(
            self.lb.paths().tiles().join(key),
            provider,
            self.lb.clock().clone(),
            CACHE_TTL,
        ));
        *slot = Some((url, cache.clone()));
        Ok(cache)
    }

    /// PNG bytes of `tile`, from the disk cache when fresh. This is the only place the map
    /// touches a tile provider.
    pub fn tile(&self, tile: TileId) -> Result<Vec<u8>> {
        self.cache()?
            .get(tile)
            .map_err(|e| AppError::Invalid(e.to_string()))
    }

    /// Whether tiles come from an injected provider instead of the network.
    pub fn uses_injected_provider(&self) -> bool {
        self.lb.inner.map.injected.is_some()
    }

    /// Writes `point` as the position of `ids` in the catalog. Files are not touched.
    pub fn set_location(&self, ids: &[PhotoId], point: GeoPoint) -> Result<usize> {
        if !(-90.0..=90.0).contains(&point.lat) || !(-180.0..=180.0).contains(&point.lon) {
            return Err(AppError::Invalid("the position is out of range".into()));
        }
        self.write_location(ids, Some(point))
    }

    /// Removes the position of `ids` from the catalog.
    pub fn clear_location(&self, ids: &[PhotoId]) -> Result<usize> {
        self.write_location(ids, None)
    }

    fn write_location(&self, ids: &[PhotoId], point: Option<GeoPoint>) -> Result<usize> {
        let changed = self.lb.catalog().set_gps(ids, point)?;
        if changed > 0 {
            self.lb.emit(AppEvent::PhotosChanged { ids: ids.to_vec() });
        }
        Ok(changed)
    }
}
