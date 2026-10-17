//! Subcommands of `lightbox-cli` and their output.

use clap::{Args, Parser, Subcommand};
use lb_app::{
    AppError, ExportOptions, ExportSize, Lightbox, MetadataPolicy, ThumbSize, ViewQuery, ViewSource,
};
use lb_core::{AlbumId, EditOp, FolderId, GroupBy, PhotoId, RootId, RootMode};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::str::FromStr;

/// Command line of `lightbox-cli`.
#[derive(Debug, Parser)]
#[command(
    name = "lightbox-cli",
    version,
    about = "Headless access to the Lightbox library"
)]
pub struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage the folders that make up the library.
    Roots {
        #[command(subcommand)]
        action: RootsCmd,
    },
    /// Scan roots for new, changed and removed files.
    Scan(ScanArgs),
    /// Search the library.
    Query(QueryArgs),
    /// Star photos.
    Star(StarArgs),
    /// Set a photo's rating.
    Rate {
        /// Photo id.
        id: i64,
        /// Rating from 0 to 5.
        rating: u8,
    },
    /// Add or remove tags.
    Tag {
        #[command(subcommand)]
        action: TagCmd,
    },
    /// Manage albums.
    Album {
        #[command(subcommand)]
        action: AlbumCmd,
    },
    /// Inspect and change a photo's edit stack.
    Edit {
        #[command(subcommand)]
        action: EditCmd,
    },
    /// Render a photo with its edits to a PNG.
    Render(RenderArgs),
    /// Export photos to a folder.
    Export(ExportArgs),
    /// Find exact and similar photos.
    Dupes(DupesArgs),
    /// Build a collage JPEG from photos.
    Collage(CollageArgs),
    /// Make an MP4 slideshow movie from photos (needs ffmpeg).
    Movie(MovieArgs),
    /// Import star, caption, tags, albums, edits and faces from `.picasa.ini` files.
    PicasaImport(PicasaArgs),
    /// Thumbnail cache maintenance.
    Thumbs {
        #[command(subcommand)]
        action: ThumbsCmd,
    },
    /// Report on the environment and the catalog.
    Doctor,
}

#[derive(Debug, Subcommand)]
enum RootsCmd {
    /// Add a root and scan it.
    Add {
        /// Folder to add.
        path: PathBuf,
        /// `watch`, `once` or `exclude`.
        #[arg(long, default_value = "watch")]
        mode: String,
    },
    /// List roots.
    List,
}

#[derive(Debug, Args)]
struct ScanArgs {
    /// Only this root.
    #[arg(long)]
    root: Option<i64>,
    /// Accepted for compatibility: scans always finish before the command returns.
    #[arg(long)]
    wait: bool,
}

#[derive(Debug, Args)]
struct QueryArgs {
    /// Search text, e.g. `beach rating:>=3 year:2021`.
    #[arg(allow_hyphen_values = true)]
    query: String,
    /// `all`, `starred`, `recent`, `album:<id>` or `folder:<id>`.
    #[arg(long, default_value = "all")]
    source: String,
}

#[derive(Debug, Args)]
struct StarArgs {
    /// Photo ids.
    #[arg(required = true)]
    ids: Vec<i64>,
    /// Remove the star instead.
    #[arg(long)]
    off: bool,
}

#[derive(Debug, Subcommand)]
enum TagCmd {
    /// Add a tag to photos.
    Add {
        /// Tag name.
        tag: String,
        /// Photo ids.
        #[arg(required = true)]
        ids: Vec<i64>,
    },
    /// Remove a tag from photos.
    Remove {
        /// Tag name.
        tag: String,
        /// Photo ids.
        #[arg(required = true)]
        ids: Vec<i64>,
    },
}

#[derive(Debug, Subcommand)]
enum AlbumCmd {
    /// Create an album.
    Create {
        /// Album name.
        name: String,
    },
    /// Add photos to an album.
    Add {
        /// Album id.
        album: i64,
        /// Photo ids.
        #[arg(required = true)]
        ids: Vec<i64>,
    },
    /// List albums.
    List,
}

#[derive(Debug, Subcommand)]
enum EditCmd {
    /// Append an operation given as JSON, e.g. `{"op":"rotate_quarter","turns":1}`.
    Push {
        /// Photo id.
        id: i64,
        /// The operation.
        op: String,
    },
    /// Undo the last edit.
    Undo {
        /// Photo id.
        id: i64,
    },
    /// Remove all edits.
    Revert {
        /// Photo id.
        id: i64,
    },
    /// Print the edit stack.
    Show {
        /// Photo id.
        id: i64,
    },
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// Photo id.
    id: i64,
    /// Output PNG path.
    out: PathBuf,
    /// Longest edge in pixels.
    #[arg(long, default_value_t = 1280)]
    max_edge: u32,
}

#[derive(Debug, Args)]
struct ExportArgs {
    /// Photo ids.
    #[arg(required = true)]
    ids: Vec<i64>,
    /// Destination folder.
    #[arg(long)]
    to: PathBuf,
    /// Scale down to this long edge.
    #[arg(long)]
    long_edge: Option<u32>,
    /// JPEG quality 1 to 100.
    #[arg(long, default_value_t = 85)]
    quality: u8,
    /// Drop GPS data from the exported EXIF.
    #[arg(long)]
    strip_gps: bool,
}

#[derive(Debug, Args)]
struct CollageArgs {
    /// `grid`, `mosaic`, `pile` or `frame`.
    layout: String,
    /// Photo ids (2 to 200).
    #[arg(required = true)]
    ids: Vec<i64>,
    /// Output JPEG file.
    #[arg(long)]
    out: PathBuf,
    /// Output size as `WxH`.
    #[arg(long, default_value = "1920x1080")]
    size: String,
    /// Seed of the pile's randomness.
    #[arg(long, default_value_t = 0)]
    seed: u64,
}

#[derive(Debug, Args)]
struct MovieArgs {
    /// Photo ids, shown in this order.
    #[arg(required = true)]
    ids: Vec<i64>,
    /// Output MP4 file.
    #[arg(long)]
    out: PathBuf,
    /// Seconds each photo is shown.
    #[arg(long, default_value_t = 3.0)]
    seconds: f64,
    /// Frames per second.
    #[arg(long, default_value_t = 30)]
    fps: u32,
    /// Movie size as `WxH` (at most 1920x1080).
    #[arg(long, default_value = "1920x1080")]
    size: String,
    /// Audio file to play under the movie.
    #[arg(long)]
    audio: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct DupesArgs {
    /// Largest perceptual-hash distance (in bits, 0 to 64) that still counts as similar.
    #[arg(long, default_value_t = lb_app::DEFAULT_SIMILARITY)]
    threshold: u32,
}

#[derive(Debug, Args)]
struct PicasaArgs {
    /// Folder holding `.picasa.ini` files (searched recursively).
    folder: PathBuf,
    /// Replace existing captions, edits, folder captions and imported faces.
    #[arg(long)]
    overwrite: bool,
    /// Only report what would change.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Subcommand)]
enum ThumbsCmd {
    /// Generate missing thumbnails.
    Warm {
        /// `S`, `M` or `L`.
        #[arg(long, default_value = "M")]
        size: String,
    },
}

/// Text and JSON forms of a command's result.
pub struct Output {
    /// Printed with `--json`.
    pub json: Value,
    /// Printed otherwise.
    pub text: String,
}

/// Why a command failed.
#[derive(Debug)]
pub enum CliError {
    /// Bad input: unknown id, invalid value.
    User(String),
    /// A runtime failure: I/O, database.
    Runtime(String),
}

impl CliError {
    /// The message to print.
    pub fn message(&self) -> &str {
        match self {
            Self::User(m) | Self::Runtime(m) => m,
        }
    }

    /// Whether the user can fix this by changing the command.
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User(_))
    }
}

impl From<AppError> for CliError {
    fn from(e: AppError) -> Self {
        let message = e.user_message();
        match e {
            AppError::Invalid(_)
            | AppError::NotFound(_)
            | AppError::AlreadyExists(_)
            | AppError::Edit(_) => Self::User(message),
            _ => Self::Runtime(message),
        }
    }
}

type CliResult = Result<Output, CliError>;

fn user(msg: impl Into<String>) -> CliError {
    CliError::User(msg.into())
}

fn photo_ids(lb: &Lightbox, ids: &[i64]) -> Result<Vec<PhotoId>, CliError> {
    let ids: Vec<PhotoId> = ids.iter().map(|i| PhotoId::new(*i)).collect();
    for id in &ids {
        if lb.photos().get(*id)?.is_none() {
            return Err(user(format!("photo {id} not found")));
        }
    }
    Ok(ids)
}

fn out(json: Value, text: impl Into<String>) -> CliResult {
    Ok(Output {
        json,
        text: text.into(),
    })
}

fn photo_json(p: &lb_core::Photo) -> Value {
    json!({
        "id": p.id.get(),
        "path": p.path,
        "taken_at": p.taken_at.map(|t| t.to_string()),
        "starred": p.starred,
        "rating": p.rating,
        "width": p.width,
        "height": p.height,
        "edit_version": p.edit_version,
        "status": p.status.as_str(),
    })
}

fn parse_source(s: &str) -> Result<ViewSource, CliError> {
    let bad = || {
        user(format!(
            "unknown source \"{s}\"; use all, starred, recent, album:<id> or folder:<id>"
        ))
    };
    let id = |v: &str| v.parse::<i64>().map_err(|_| bad());
    Ok(match s {
        "all" => ViewSource::AllPhotos,
        "starred" => ViewSource::Starred,
        "recent" => ViewSource::RecentlyEdited,
        _ => match s.split_once(':') {
            Some(("album", v)) => ViewSource::Album(AlbumId::new(id(v)?)),
            Some(("folder", v)) => ViewSource::Folder(FolderId::new(id(v)?)),
            _ => return Err(bad()),
        },
    })
}

fn scan_json(s: &lb_app::ScanSummary) -> Value {
    json!({
        "added": s.added, "updated": s.updated, "unchanged": s.unchanged,
        "missing": s.missing, "errors": s.errors, "moved": s.moved,
        "cancelled": s.cancelled,
    })
}

/// Runs a parsed command line against the library in `LIGHTBOX_HOME` (or the platform default).
pub fn run(cli: Cli) -> CliResult {
    let lb = Lightbox::open_default(lb_app::JobQueue::inline())?;
    match cli.command {
        Command::Roots { action } => roots(&lb, action),
        Command::Scan(a) => scan(&lb, &a),
        Command::Query(a) => query(&lb, &a),
        Command::Star(a) => {
            let ids = photo_ids(&lb, &a.ids)?;
            let changed = lb.photos().set_starred(&ids, !a.off)?;
            out(
                json!({ "starred": !a.off, "changed": changed }),
                format!(
                    "{} {changed} photo(s)",
                    if a.off { "unstarred" } else { "starred" }
                ),
            )
        }
        Command::Rate { id, rating } => {
            let ids = photo_ids(&lb, &[id])?;
            let changed = lb.photos().set_rating(&ids, rating)?;
            out(
                json!({ "rating": rating, "changed": changed }),
                format!("rated photo {id} {rating}"),
            )
        }
        Command::Tag { action } => tag(&lb, action),
        Command::Album { action } => album(&lb, action),
        Command::Edit { action } => edit(&lb, action),
        Command::Render(a) => render(&lb, &a),
        Command::Export(a) => export(&lb, &a),
        Command::Dupes(a) => dupes(&lb, &a),
        Command::Collage(a) => collage(&lb, &a),
        Command::Movie(a) => movie(&lb, &a),
        Command::PicasaImport(a) => picasa_import(&lb, &a),
        Command::Thumbs { action } => {
            let ThumbsCmd::Warm { size } = action;
            thumbs_warm(&lb, &size)
        }
        Command::Doctor => doctor(&lb),
    }
}

fn roots(lb: &Lightbox, action: RootsCmd) -> CliResult {
    match action {
        RootsCmd::Add { path, mode } => {
            let mode = RootMode::from_str(&mode).map_err(|e| user(e.to_string()))?;
            let change = lb.library().add_root(&path, mode)?;
            let root = change
                .root
                .ok_or_else(|| CliError::Runtime("root was not created".into()))?;
            out(
                json!({
                    "id": root.id.get(), "path": root.path, "mode": root.mode.as_str(),
                    "scan": change.scan.as_ref().map(scan_json),
                }),
                format!("added root {} {}", root.id, root.path.display()),
            )
        }
        RootsCmd::List => {
            let roots = lb.library().roots()?;
            let text = roots
                .iter()
                .map(|r| format!("{}\t{}\t{}", r.id, r.mode.as_str(), r.path.display()))
                .collect::<Vec<_>>()
                .join("\n");
            let json = roots
                .iter()
                .map(|r| json!({ "id": r.id.get(), "path": r.path, "mode": r.mode.as_str() }))
                .collect::<Vec<_>>();
            out(Value::Array(json), text)
        }
    }
}

fn picasa_import(lb: &Lightbox, a: &PicasaArgs) -> CliResult {
    let options = lb_app::PicasaOptions {
        overwrite: a.overwrite,
    };
    let r = if a.dry_run {
        lb.picasa().preview(&a.folder, options)?
    } else {
        lb.picasa().import(&a.folder, options)?
    };
    out(
        json!({
            "dry_run": a.dry_run, "ini_files": r.ini_files,
            "photos_matched": r.photos_matched, "photos_missing": r.photos_missing,
            "starred": r.starred, "captions": r.captions, "tags_added": r.tags_added,
            "albums_created": r.albums_created, "album_links": r.album_links,
            "edits": r.edits, "faces": r.faces, "folder_captions": r.folder_captions,
            "skipped_filters": r.skipped_filters,
        }),
        format!(
            "{} {} ini file(s), {} photo(s): {} starred, {} captions, {} tags, {} albums, \
             {} edits, {} faces",
            if a.dry_run {
                "would import"
            } else {
                "imported"
            },
            r.ini_files,
            r.photos_matched,
            r.starred,
            r.captions,
            r.tags_added,
            r.albums_created,
            r.edits,
            r.faces
        ),
    )
}

fn scan(lb: &Lightbox, a: &ScanArgs) -> CliResult {
    let _ = a.wait;
    let results = match a.root {
        Some(id) => vec![(RootId::new(id), lb.library().rescan(RootId::new(id))?)],
        None => lb.library().rescan_all()?,
    };
    let text = results
        .iter()
        .map(|(id, s)| {
            format!(
                "root {id}: {} added, {} updated, {} missing, {} errors",
                s.added, s.updated, s.missing, s.errors
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let json = results
        .iter()
        .map(|(id, s)| json!({ "root": id.get(), "summary": scan_json(s) }))
        .collect::<Vec<_>>();
    out(Value::Array(json), text)
}

fn query(lb: &Lightbox, a: &QueryArgs) -> CliResult {
    let mut view = ViewQuery::new(parse_source(&a.source)?);
    view.search = a.query.clone();
    view.group_by = GroupBy::None;
    let plan = lb.search().parse(&view.effective_text());
    let result = lb.search().query(&view)?;
    let ids = result.ids();
    let mut photos = Vec::new();
    for chunk in ids.chunks(200) {
        photos.extend(lb.photos().get_many(chunk)?);
    }
    let text = photos
        .iter()
        .map(|p| format!("{}\t{}", p.id, p.path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let diagnostics: Vec<Value> = plan
        .diagnostics
        .iter()
        .map(|d| json!({ "start": d.span.start, "end": d.span.end, "message": d.message }))
        .collect();
    out(
        json!({
            "total": result.total,
            "photos": photos.iter().map(photo_json).collect::<Vec<_>>(),
            "diagnostics": diagnostics,
        }),
        text,
    )
}

fn tag(lb: &Lightbox, action: TagCmd) -> CliResult {
    let (add, tag, ids) = match action {
        TagCmd::Add { tag, ids } => (true, tag, ids),
        TagCmd::Remove { tag, ids } => (false, tag, ids),
    };
    let ids = photo_ids(lb, &ids)?;
    let names = [tag.clone()];
    let changed = if add {
        lb.tags().add(&ids, &names)?
    } else {
        lb.tags().remove(&ids, &names)?
    };
    out(
        json!({ "tag": tag, "added": add, "changed": changed }),
        format!(
            "{} tag \"{tag}\" on {changed} photo(s)",
            if add { "added" } else { "removed" }
        ),
    )
}

fn album(lb: &Lightbox, action: AlbumCmd) -> CliResult {
    match action {
        AlbumCmd::Create { name } => {
            let album = lb.albums().create(&name, None)?;
            out(
                json!({ "id": album.id.get(), "name": album.name }),
                format!("created album {} \"{}\"", album.id, album.name),
            )
        }
        AlbumCmd::Add { album, ids } => {
            let id = AlbumId::new(album);
            if lb.albums().get(id)?.is_none() {
                return Err(user(format!("album {album} not found")));
            }
            let ids = photo_ids(lb, &ids)?;
            let added = lb.albums().add_photos(id, &ids)?;
            out(
                json!({ "album": album, "added": added }),
                format!("added {added} photo(s)"),
            )
        }
        AlbumCmd::List => {
            let albums = lb.albums().list()?;
            let mut json = Vec::new();
            let mut lines = Vec::new();
            for a in &albums {
                let count = lb.albums().photo_ids(a.id)?.len();
                lines.push(format!("{}\t{}\t{count}", a.id, a.name));
                json.push(json!({ "id": a.id.get(), "name": a.name, "photos": count }));
            }
            out(Value::Array(json), lines.join("\n"))
        }
    }
}

fn stack_output(lb: &Lightbox, id: PhotoId) -> CliResult {
    let stack = lb.edits().stack(id)?;
    let text = stack
        .ops
        .iter()
        .map(|op| serde_json::to_string(op).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n");
    let json = serde_json::to_value(&stack).map_err(|e| CliError::Runtime(e.to_string()))?;
    out(json, text)
}

fn edit(lb: &Lightbox, action: EditCmd) -> CliResult {
    match action {
        EditCmd::Push { id, op } => {
            let id = photo_ids(lb, &[id])?[0];
            let op: EditOp =
                serde_json::from_str(&op).map_err(|e| user(format!("invalid edit JSON: {e}")))?;
            lb.edits().push(id, op)?;
            stack_output(lb, id)
        }
        EditCmd::Undo { id } => {
            let id = photo_ids(lb, &[id])?[0];
            lb.edits().undo(id)?;
            stack_output(lb, id)
        }
        EditCmd::Revert { id } => {
            let id = photo_ids(lb, &[id])?[0];
            lb.edits().revert(id)?;
            stack_output(lb, id)
        }
        EditCmd::Show { id } => stack_output(lb, photo_ids(lb, &[id])?[0]),
    }
}

fn render(lb: &Lightbox, a: &RenderArgs) -> CliResult {
    let id = photo_ids(lb, &[a.id])?[0];
    if a.max_edge == 0 {
        return Err(user("--max-edge must be at least 1"));
    }
    let img = lb.edits().render_preview(id, a.max_edge)?;
    img.save_with_format(&a.out, image::ImageFormat::Png)
        .map_err(|e| CliError::Runtime(format!("could not write {}: {e}", a.out.display())))?;
    out(
        json!({ "path": a.out, "width": img.width(), "height": img.height() }),
        format!(
            "wrote {} ({}x{})",
            a.out.display(),
            img.width(),
            img.height()
        ),
    )
}

fn collage(lb: &Lightbox, a: &CollageArgs) -> CliResult {
    let layout = lb_app::CollageLayout::parse(&a.layout)
        .ok_or_else(|| user("layout must be grid, mosaic, pile or frame"))?;
    let size =
        lb_app::CollageSize::parse(&a.size).ok_or_else(|| user("size must look like 1920x1080"))?;
    let ids = photo_ids(lb, &a.ids)?;
    let opts = lb_app::CollageOptions {
        layout,
        size,
        seed: a.seed,
        ..lb_app::CollageOptions::default()
    };
    let made = lb.collage().create(&ids, &opts, &a.out)?;
    let (width, height) = size.dimensions();
    out(
        json!({
            "path": made.path,
            "id": made.id.map(|i| i.get()),
            "width": width,
            "height": height,
        }),
        format!("saved {} ({width}x{height})", made.path.display()),
    )
}

fn movie(lb: &Lightbox, a: &MovieArgs) -> CliResult {
    let size =
        lb_app::CollageSize::parse(&a.size).ok_or_else(|| user("size must look like 1920x1080"))?;
    let (width, height) = size.dimensions();
    if !a.seconds.is_finite() {
        return Err(user("seconds must be a number"));
    }
    let ids = photo_ids(lb, &a.ids)?;
    let settings = lb_app::MovieSettings {
        fps: a.fps,
        width,
        height,
        seconds_per_photo: a.seconds,
        audio: a.audio.clone(),
    };
    let report = lb
        .movie()
        .render(&ids, &settings, &a.out, &|| false, &|_, _| {})?;
    let skipped: Vec<i64> = report.skipped.iter().map(|i| i.get()).collect();
    out(
        json!({
            "path": report.path,
            "photos": report.photos,
            "skipped": skipped,
            "duration_secs": report.duration_secs,
        }),
        format!(
            "saved {} ({} photos, {:.1} s)",
            a.out.display(),
            report.photos,
            report.duration_secs
        ),
    )
}

fn dupes(lb: &Lightbox, a: &DupesArgs) -> CliResult {
    if a.threshold > 64 {
        return Err(user("threshold must be between 0 and 64"));
    }
    let groups = lb.duplicates().find(a.threshold)?;
    let json_groups: Vec<Value> = groups
        .iter()
        .map(|g| {
            json!({
                "exact": g.exact,
                "ids": g.ids.iter().map(|i| i.get()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let text = if groups.is_empty() {
        "no duplicates found".to_owned()
    } else {
        groups
            .iter()
            .map(|g| {
                let ids: Vec<String> = g.ids.iter().map(ToString::to_string).collect();
                format!(
                    "{}\t{}",
                    if g.exact { "exact" } else { "similar" },
                    ids.join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    out(
        json!({ "threshold": a.threshold, "groups": json_groups }),
        text,
    )
}

fn export(lb: &Lightbox, a: &ExportArgs) -> CliResult {
    let ids = photo_ids(lb, &a.ids)?;
    let mut opts = ExportOptions::new(&a.to);
    opts.quality = a.quality;
    if let Some(n) = a.long_edge {
        opts.size = ExportSize::LongEdge(n);
    }
    opts.metadata = if a.strip_gps {
        MetadataPolicy::KeepExifNoGps
    } else {
        MetadataPolicy::KeepAll
    };
    let report = lb.export().export_now(&ids, &opts)?;
    let files: Vec<Value> = report
        .written
        .iter()
        .map(|f| json!({ "id": f.id.get(), "path": f.path }))
        .collect();
    let failed: Vec<Value> = report
        .failed
        .iter()
        .map(|(id, why)| json!({ "id": id.get(), "reason": why }))
        .collect();
    let text = format!(
        "exported {} file(s) to {}{}",
        report.written.len(),
        a.to.display(),
        if report.failed.is_empty() {
            String::new()
        } else {
            format!("; {} failed", report.failed.len())
        }
    );
    let result = out(
        json!({ "written": files, "skipped": report.skipped.len(), "failed": failed }),
        text,
    );
    if !report.failed.is_empty() && report.written.is_empty() {
        return Err(CliError::Runtime(format!(
            "nothing exported: {}",
            report.failed[0].1
        )));
    }
    result
}

fn thumbs_warm(lb: &Lightbox, size: &str) -> CliResult {
    let size = match size.to_ascii_uppercase().as_str() {
        "S" => ThumbSize::S,
        "M" => ThumbSize::M,
        "L" => ThumbSize::L,
        other => return Err(user(format!("unknown size \"{other}\"; use S, M or L"))),
    };
    let tickets = lb.thumbs().warm(size)?;
    let requested = tickets.len();
    let mut failed = 0;
    for t in tickets {
        if let Some(Err(_)) = t.wait(std::time::Duration::from_secs(600)) {
            failed += 1;
        }
    }
    out(
        json!({ "requested": requested, "failed": failed }),
        format!(
            "generated {} thumbnail(s), {failed} failed",
            requested - failed
        ),
    )
}

fn doctor(lb: &Lightbox) -> CliResult {
    let report = lb.doctor()?;
    let opt = |p: &Option<PathBuf>| {
        p.as_ref()
            .map_or_else(|| "not found".to_owned(), |p| p.display().to_string())
    };
    let text = format!(
        "data:      {}\ncache:     {}\ncatalog:   schema {}, {} photos, {} roots, integrity {}\nffmpeg:    {}\nffprobe:   {}\nmodels:    {}\ngraphics:  {}{}\nfeatures:  {}",
        report.data_dir.display(),
        report.cache_dir.display(),
        report.schema_version,
        report.photos,
        report.roots,
        if report.healthy() {
            "ok".to_owned()
        } else {
            report.integrity_problems.join("; ")
        },
        opt(&report.ffmpeg),
        opt(&report.ffprobe),
        report.face_models.as_ref().map_or_else(
            || "LIGHTBOX_FACE_MODELS not set".to_owned(),
            |(p, ok)| format!(
                "{} ({}, {})",
                p.display(),
                if *ok { "found" } else { "missing" },
                if report.face_models_ready {
                    "verified"
                } else {
                    "not ready"
                }
            )
        ),
        report.graphics.session,
        if report.graphics.render_nodes.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                report
                    .graphics
                    .render_nodes
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
        report.features.join(", "),
    );
    let json = serde_json::to_value(&report).map_err(|e| CliError::Runtime(e.to_string()))?;
    let result = out(json, text);
    if report.healthy() {
        result
    } else {
        Err(CliError::Runtime(format!(
            "catalog integrity check failed: {}",
            report.integrity_problems.join("; ")
        )))
    }
}
