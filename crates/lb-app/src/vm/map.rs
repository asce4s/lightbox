//! View-model of the map panel (SPEC §6.14).

use crate::error::Result;
use crate::lightbox::Lightbox;
use lb_core::{GeoPoint, PhotoId};
use lb_geo::{
    Cluster, MAX_ZOOM, MIN_ZOOM, Marker, PlacedTile, Viewport, lonlat_to_world, world_to_lonlat,
};

/// A cluster drawn at a window position.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenCluster {
    /// Pixel position of the marker centre in the panel.
    pub x: f64,
    /// Pixel position of the marker centre in the panel.
    pub y: f64,
    /// The photos it stands for.
    pub cluster: Cluster,
}

/// Pan, zoom and cluster state of the map panel over a set of photos.
pub struct MapVm {
    lb: Lightbox,
    ids: Vec<PhotoId>,
    viewport: Viewport,
}

impl MapVm {
    /// A panel `width` by `height` pixels showing the geotagged photos among `ids`, framed so all
    /// of them are visible.
    pub fn new(lb: &Lightbox, ids: Vec<PhotoId>, width: f64, height: f64) -> Result<Self> {
        let mut vm = Self {
            lb: lb.clone(),
            ids,
            viewport: Viewport {
                center: GeoPoint {
                    lat: 20.0,
                    lon: 0.0,
                },
                zoom: MIN_ZOOM,
                width: width.max(1.0),
                height: height.max(1.0),
            },
        };
        vm.fit()?;
        Ok(vm)
    }

    /// Frames every marker (or the whole world when there are none).
    pub fn fit(&mut self) -> Result<()> {
        let markers: Vec<Marker> = self.lb.map().markers(&self.ids)?;
        let (w, h) = (self.viewport.width, self.viewport.height);
        let Some(first) = markers.first() else {
            self.viewport.center = GeoPoint {
                lat: 20.0,
                lon: 0.0,
            };
            self.viewport.zoom = MIN_ZOOM;
            return Ok(());
        };
        let fits = |z: u8| {
            let pts: Vec<(f64, f64)> = markers
                .iter()
                .map(|m| lonlat_to_world(m.point, z))
                .collect();
            let span = |f: fn(&(f64, f64)) -> f64| {
                let lo = pts.iter().map(f).fold(f64::INFINITY, f64::min);
                let hi = pts.iter().map(f).fold(f64::NEG_INFINITY, f64::max);
                (lo, hi)
            };
            let (x0, x1) = span(|p| p.0);
            let (y0, y1) = span(|p| p.1);
            (x1 - x0 <= w * 0.8 && y1 - y0 <= h * 0.8)
                .then(|| world_to_lonlat((x0 + x1) / 2.0, (y0 + y1) / 2.0, z))
        };
        let mut best = (MIN_ZOOM, first.point);
        for z in MIN_ZOOM..=MAX_ZOOM.min(16) {
            match fits(z) {
                Some(center) => best = (z, center),
                None => break,
            }
        }
        self.viewport.zoom = best.0;
        self.viewport.center = best.1;
        Ok(())
    }

    /// Replaces the photos shown (for example after the grid's view changed).
    pub fn set_photos(&mut self, ids: Vec<PhotoId>) {
        self.ids = ids;
    }

    /// The current window.
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Changes the panel size.
    pub fn resize(&mut self, width: f64, height: f64) {
        self.viewport.width = width.max(1.0);
        self.viewport.height = height.max(1.0);
    }

    /// Drag by `(dx, dy)` pixels.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.viewport = self.viewport.panned(dx, dy);
    }

    /// Scroll wheel: `delta` zoom levels around pixel `(x, y)`.
    pub fn zoom_by(&mut self, delta: i8, x: f64, y: f64) {
        self.viewport = self.viewport.zoomed(delta, x, y);
    }

    /// Tiles covering the window.
    pub fn tiles(&self) -> Vec<PlacedTile> {
        self.lb.map().tiles(&self.viewport)
    }

    /// Clusters placed in window coordinates; those entirely outside the window are dropped.
    pub fn clusters(&self) -> Result<Vec<ScreenCluster>> {
        let vp = self.viewport;
        Ok(self
            .lb
            .map()
            .clusters(&self.ids, &vp)?
            .into_iter()
            .map(|cluster| {
                let (x, y) = vp.project(cluster.center);
                ScreenCluster { x, y, cluster }
            })
            .filter(|c| {
                c.x >= -30.0 && c.x <= vp.width + 30.0 && c.y >= -30.0 && c.y <= vp.height + 30.0
            })
            .collect())
    }

    /// Photos of the cluster at `index` of [`Self::clusters`]; the grid filters to these.
    pub fn cluster_photos(&self, index: usize) -> Result<Vec<PhotoId>> {
        Ok(self
            .clusters()?
            .get(index)
            .map(|c| c.cluster.ids.clone())
            .unwrap_or_default())
    }

    /// The map position under window pixel `(x, y)`; where "Set location" writes to.
    pub fn location_at(&self, x: f64, y: f64) -> GeoPoint {
        self.viewport.unproject(x, y)
    }

    /// Writes the position under `(x, y)` to `targets`.
    pub fn set_location_at(&self, targets: &[PhotoId], x: f64, y: f64) -> Result<usize> {
        self.lb.map().set_location(targets, self.location_at(x, y))
    }
}
