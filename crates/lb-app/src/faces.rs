//! Face detection, clustering and people management (SPEC §6.15).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use image::{RgbImage, imageops::FilterType};
use lb_catalog::NewFace;
use lb_core::{
    Face, FaceId, FaceSource, NormRect, PeopleEntry, PeopleKey, Person, PersonId, PhotoId,
};
use lb_faces::{ClusterIndex, DEFAULT_THRESHOLD, DETECT_EDGE, FacePipeline, agglomerate};
use lb_jobs::{Job, JobCtx, JobError, JobHandle, JobResult, Priority};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Most unnamed faces a full re-clustering handles (it needs quadratic memory).
pub const MAX_RECLUSTER_FACES: usize = 5000;

/// Margin added around a face box when cropping it for display.
const CROP_MARGIN: f32 = 0.4;

/// Shared state of the face services inside [`Lightbox`].
pub(crate) struct FaceState {
    injected: Option<Arc<FacePipeline>>,
    resolved: Mutex<Option<Arc<FacePipeline>>>,
    index: Mutex<Option<ClusterIndex>>,
    run: Mutex<()>,
}

impl FaceState {
    pub(crate) fn new(injected: Option<Arc<FacePipeline>>) -> Self {
        Self {
            injected,
            resolved: Mutex::new(None),
            index: Mutex::new(None),
            run: Mutex::new(()),
        }
    }
}

/// What a detection run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FaceScanReport {
    /// Photos analysed.
    pub photos: usize,
    /// Faces stored.
    pub faces: usize,
    /// Photos that could not be decoded; they are marked as done and not retried.
    pub unreadable: usize,
}

/// Face detection and people operations.
#[derive(Clone)]
pub struct FaceService {
    lb: Lightbox,
}

impl Lightbox {
    /// Faces and people.
    pub fn faces(&self) -> FaceService {
        FaceService { lb: self.clone() }
    }
}

struct DetectJob {
    service: FaceService,
}

impl Job for DetectJob {
    fn kind(&self) -> &'static str {
        "faces"
    }

    fn coalesce_key(&self) -> Option<String> {
        Some("faces".into())
    }

    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        self.service
            .process_pending(Some(ctx))
            .map(|_| ())
            .map_err(|e| JobError::Failed(e.to_string()))
    }
}

impl FaceService {
    /// The detector and embedder in use: the injected pair, or the models in the models
    /// directory (`LIGHTBOX_FACE_MODELS` overrides it) in builds with the `onnx` feature.
    pub fn pipeline(&self) -> Option<Arc<FacePipeline>> {
        let state = &self.lb.inner.faces;
        if let Some(p) = &state.injected {
            return Some(p.clone());
        }
        let mut slot = state.resolved.lock();
        if slot.is_none() {
            *slot = self.load_models();
        }
        slot.clone()
    }

    #[cfg(feature = "onnx")]
    fn load_models(&self) -> Option<Arc<FacePipeline>> {
        let dir = std::env::var_os(crate::doctor::FACE_MODELS_ENV)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.lb.paths().models());
        match lb_faces::load_onnx_pipeline(&dir) {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::info!(error = %e, dir = %dir.display(), "face models are not available");
                None
            }
        }
    }

    #[cfg(not(feature = "onnx"))]
    fn load_models(&self) -> Option<Arc<FacePipeline>> {
        None
    }

    /// Whether detection can run: a pipeline is available.
    pub fn available(&self) -> bool {
        self.pipeline().is_some()
    }

    /// Whether detection is switched on in the settings and possible.
    pub fn enabled(&self) -> bool {
        self.lb.settings().load().is_ok_and(|s| s.faces.enabled) && self.available()
    }

    fn invalidate(&self) {
        *self.lb.inner.faces.index.lock() = None;
    }

    fn changed(&self) {
        self.invalidate();
        self.lb.emit(AppEvent::PeopleChanged);
    }

    fn build_index(&self) -> Result<ClusterIndex> {
        let embeddings = self.lb.catalog().face_embeddings()?;
        let max = self.lb.catalog().max_cluster_id()?;
        Ok(ClusterIndex::from_members(
            embeddings
                .iter()
                .filter_map(|e| e.cluster_id.map(|c| (c, e.embedding.as_slice()))),
            max,
        ))
    }

    /// Detects faces in every readable image that has not been analysed yet and clusters them
    /// incrementally. Runs on the calling thread; `ctx` allows cancellation and progress.
    pub fn process_pending(&self, ctx: Option<&JobCtx>) -> Result<FaceScanReport> {
        let pipeline = self
            .pipeline()
            .ok_or_else(|| AppError::Invalid("the face models are not installed".into()))?;
        let state = &self.lb.inner.faces;
        let _single = state.run.lock();
        let pending = self.lb.catalog().photos_without_face_scan()?;
        let mut report = FaceScanReport::default();
        if pending.is_empty() {
            return Ok(report);
        }
        let mut index = match state.index.lock().take() {
            Some(index) => index,
            None => self.build_index()?,
        };
        let total = pending.len();
        let mut outcome = Ok(());
        for (i, id) in pending.into_iter().enumerate() {
            if ctx.is_some_and(JobCtx::is_cancelled) {
                break;
            }
            match self.analyse_photo(&pipeline, &mut index, id) {
                Ok(Some(n)) => {
                    report.photos += 1;
                    report.faces += n;
                }
                Ok(None) => report.unreadable += 1,
                Err(e) => {
                    outcome = Err(e);
                    break;
                }
            }
            if let Some(ctx) = ctx {
                ctx.progress((i + 1) as f32 / total as f32);
            }
        }
        *state.index.lock() = Some(index);
        outcome?;
        if report.faces > 0 {
            self.lb.emit(AppEvent::PeopleChanged);
        }
        Ok(report)
    }

    fn analyse_photo(
        &self,
        pipeline: &FacePipeline,
        index: &mut ClusterIndex,
        id: PhotoId,
    ) -> Result<Option<usize>> {
        let catalog = self.lb.catalog();
        let now = self.lb.clock().now();
        let Some(photo) = catalog.get_photo(id)? else {
            return Ok(None);
        };
        let img = match lb_media::decode(&photo.path, Some(DETECT_EDGE)) {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!(path = %photo.path.display(), error = %e, "face detection skipped: unreadable");
                catalog.record_face_scan(id, Vec::new(), now)?;
                return Ok(None);
            }
        };
        let found = pipeline
            .analyze(&img)
            .map_err(|e| AppError::Invalid(format!("face detection failed: {e}")))?;
        let faces: Vec<NewFace> = found
            .into_iter()
            .map(|f| NewFace {
                photo_id: id,
                rect: f.rect,
                score: f.score,
                cluster_id: Some(index.assign(&f.embedding, DEFAULT_THRESHOLD)),
                embedding: Some(f.embedding),
                person_id: None,
                source: FaceSource::Detected,
            })
            .collect();
        let n = faces.len();
        catalog.record_face_scan(id, faces, now)?;
        Ok(Some(n))
    }

    /// Queues detection as a background job when faces are enabled and possible.
    pub fn spawn_pending(&self) -> Option<JobHandle> {
        if !self.enabled() {
            return None;
        }
        Some(self.lb.jobs().submit(
            DetectJob {
                service: self.clone(),
            },
            Priority::Background,
        ))
    }

    /// Forgets all detected faces so that every photo is analysed again. Named people stay.
    pub fn reset_detection(&self) -> Result<usize> {
        let _single = self.lb.inner.faces.run.lock();
        let n = self.lb.catalog().reset_face_detection()?;
        self.changed();
        Ok(n)
    }

    /// Re-clusters the unnamed faces with average linkage at the same threshold as incremental
    /// assignment. Clusters keep their ids where they can; named people are not touched.
    /// Returns the number of unnamed clusters.
    pub fn recluster(&self) -> Result<usize> {
        let _single = self.lb.inner.faces.run.lock();
        let catalog = self.lb.catalog();
        let unnamed: Vec<_> = catalog
            .face_embeddings()?
            .into_iter()
            .filter(|e| e.person_id.is_none() && e.cluster_id.is_some())
            .collect();
        if unnamed.len() > MAX_RECLUSTER_FACES {
            return Err(AppError::Invalid(
                "there are too many unnamed faces to group again at once".into(),
            ));
        }
        let vectors: Vec<Vec<f32>> = unnamed.iter().map(|e| e.embedding.clone()).collect();
        let labels = agglomerate(&vectors, DEFAULT_THRESHOLD);
        let groups = labels.iter().max().map_or(0, |m| m + 1);

        let mut overlap: Vec<BTreeMap<i64, usize>> = vec![BTreeMap::new(); groups];
        for (e, l) in unnamed.iter().zip(&labels) {
            if let Some(c) = e.cluster_id {
                *overlap[*l].entry(c).or_default() += 1;
            }
        }
        let mut order: Vec<usize> = (0..groups).collect();
        order.sort_by_key(|g| std::cmp::Reverse(overlap[*g].values().sum::<usize>()));
        let mut next = catalog.max_cluster_id()?;
        let mut taken = std::collections::BTreeSet::new();
        let mut target = vec![0i64; groups];
        for g in order {
            let best = overlap[g]
                .iter()
                .filter(|(c, _)| !taken.contains(*c))
                .max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c)))
                .map(|(c, _)| *c);
            target[g] = best.unwrap_or_else(|| {
                next += 1;
                next
            });
            taken.insert(target[g]);
        }
        let changes: Vec<(FaceId, i64)> = unnamed
            .iter()
            .zip(&labels)
            .filter(|(e, l)| e.cluster_id != Some(target[**l]))
            .map(|(e, l)| (e.id, target[*l]))
            .collect();
        if !changes.is_empty() {
            catalog.set_face_clusters(changes)?;
        }
        self.changed();
        Ok(groups)
    }

    /// The People list: unnamed groups first, then people by name.
    pub fn people(&self, include_hidden: bool) -> Result<Vec<PeopleEntry>> {
        Ok(self.lb.catalog().list_people(include_hidden)?)
    }

    /// Every named person, ordered by name (for autocomplete).
    pub fn persons(&self) -> Result<Vec<Person>> {
        Ok(self.lb.catalog().list_persons()?)
    }

    /// Names starting with `prefix`, case-insensitively, for autocomplete.
    pub fn suggest_names(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        let prefix = prefix.trim().to_lowercase();
        Ok(self
            .persons()?
            .into_iter()
            .filter(|p| p.name.to_lowercase().starts_with(&prefix))
            .map(|p| p.name)
            .take(limit)
            .collect())
    }

    /// Faces of a person or cluster, best score first.
    pub fn faces_of(&self, key: PeopleKey) -> Result<Vec<Face>> {
        Ok(self.lb.catalog().faces_of(key)?)
    }

    /// Photos in which a person or cluster appears.
    pub fn photos_of(&self, key: PeopleKey) -> Result<Vec<PhotoId>> {
        Ok(self.lb.catalog().photos_of(key)?)
    }

    /// Faces in one photo, left to right.
    pub fn faces_in_photo(&self, photo: PhotoId) -> Result<Vec<Face>> {
        Ok(self.lb.catalog().faces_of_photo(photo)?)
    }

    /// Names a person or cluster. Naming a cluster with an existing name merges it into that
    /// person.
    pub fn name(&self, key: PeopleKey, name: &str) -> Result<Person> {
        let person = match key {
            PeopleKey::Cluster(c) => self.lb.catalog().name_cluster(c, name)?,
            PeopleKey::Person(id) => self.lb.catalog().rename_person(id, name)?,
        };
        self.changed();
        Ok(person)
    }

    /// Merges `from` into `into`.
    pub fn merge(&self, from: PersonId, into: PersonId) -> Result<()> {
        self.lb.catalog().merge_people(from, into)?;
        self.changed();
        Ok(())
    }

    /// Takes one face away from its person ("not this person"); it becomes an unnamed cluster.
    pub fn remove_from_person(&self, face: FaceId) -> Result<()> {
        self.lb.catalog().remove_face_from_person(face)?;
        self.changed();
        Ok(())
    }

    /// Assigns one face to a person.
    pub fn assign(&self, face: FaceId, person: PersonId) -> Result<()> {
        self.lb.catalog().assign_face_to_person(face, person)?;
        self.changed();
        Ok(())
    }

    /// Deletes a face box that is not a face.
    pub fn delete_face(&self, face: FaceId) -> Result<()> {
        self.lb.catalog().delete_face(face)?;
        self.changed();
        Ok(())
    }

    /// Hides a person from the People list, or shows them again.
    pub fn set_hidden(&self, person: PersonId, hidden: bool) -> Result<()> {
        self.lb.catalog().set_person_hidden(person, hidden)?;
        self.changed();
        Ok(())
    }

    /// Deletes a person; their faces become unnamed.
    pub fn delete_person(&self, person: PersonId) -> Result<()> {
        self.lb.catalog().delete_person(person)?;
        self.changed();
        Ok(())
    }

    /// Adds a face drawn by the user, optionally already named.
    pub fn add_manual(&self, photo: PhotoId, rect: NormRect, name: Option<&str>) -> Result<FaceId> {
        let person = name
            .map(|n| self.lb.catalog().get_or_create_person(n))
            .transpose()?
            .map(|p| p.id);
        let rect = NormRect {
            x: rect.x.clamp(0.0, 1.0),
            y: rect.y.clamp(0.0, 1.0),
            w: rect.w.clamp(0.0, 1.0 - rect.x.clamp(0.0, 1.0)),
            h: rect.h.clamp(0.0, 1.0 - rect.y.clamp(0.0, 1.0)),
        };
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return Err(AppError::Invalid("the face box is empty".into()));
        }
        let id = self.lb.catalog().add_manual_face(photo, rect, person)?;
        self.changed();
        Ok(id)
    }

    /// A square crop of `face` with some margin, `edge` pixels wide.
    pub fn crop(&self, face: FaceId, edge: u32) -> Result<RgbImage> {
        let face = self
            .lb
            .catalog()
            .get_face(face)?
            .ok_or(AppError::NotFound("face"))?;
        let photo = self
            .lb
            .catalog()
            .get_photo(face.photo_id)?
            .ok_or(AppError::NotFound("photo"))?;
        let img = lb_media::decode(&photo.path, Some(DETECT_EDGE))?.to_rgb8();
        let (w, h) = (img.width() as f32, img.height() as f32);
        let r = face.rect;
        let side = (r.w * w).max(r.h * h) * (1.0 + CROP_MARGIN);
        let cx = (r.x + r.w / 2.0) * w;
        let cy = (r.y + r.h / 2.0) * h;
        let side = side.min(w.min(h)).max(1.0);
        let x = (cx - side / 2.0).clamp(0.0, (w - side).max(0.0));
        let y = (cy - side / 2.0).clamp(0.0, (h - side).max(0.0));
        let crop = image::imageops::crop_imm(
            &img,
            x as u32,
            y as u32,
            (side as u32).max(1),
            (side as u32).max(1),
        )
        .to_image();
        Ok(image::imageops::resize(
            &crop,
            edge.max(1),
            edge.max(1),
            FilterType::Triangle,
        ))
    }
}

/// Starts detection after a scan when it is switched on.
pub(crate) fn after_scan(lb: &Lightbox) {
    drop(lb.faces().spawn_pending());
}
