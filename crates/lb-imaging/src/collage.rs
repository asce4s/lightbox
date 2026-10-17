//! Collage layouts and rendering (SPEC §6.13).
//!
//! [`layout`] is pure geometry over photo aspect ratios; [`render`] paints the placements.
//! Grid, mosaic and frame layouts produce integer, axis-aligned rectangles; the picture pile
//! places rotated photos whose rotated bounding boxes stay inside the canvas.

use image::{DynamicImage, Rgba, RgbaImage};
use lb_media::resize_exact;
use rand_core::{Rng, SeedableRng};
use rand_pcg::Pcg32;

/// Fewest and most photos in a collage.
pub const PHOTO_LIMITS: (usize, usize) = (2, 200);
/// Largest rotation of a pile photo in degrees.
pub const PILE_MAX_ANGLE: f32 = 15.0;
const PILE_TRIES: usize = 60;

/// How photos are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollageLayout {
    /// Equal cells, centre-cropped.
    #[default]
    Grid,
    /// Justified rows of equal height per row (Flickr style).
    Mosaic,
    /// Photos with white borders at random angles.
    Pile,
    /// One large centre photo surrounded by a ring of others.
    FrameMosaic,
}

impl CollageLayout {
    /// Parses `grid`, `mosaic`, `pile` or `frame`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "grid" => Some(Self::Grid),
            "mosaic" => Some(Self::Mosaic),
            "pile" => Some(Self::Pile),
            "frame" | "frame-mosaic" | "frame_mosaic" => Some(Self::FrameMosaic),
            _ => None,
        }
    }

    /// The name accepted by [`Self::parse`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Mosaic => "mosaic",
            Self::Pile => "pile",
            Self::FrameMosaic => "frame",
        }
    }
}

/// Canvas and layout parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct CollageSpec {
    /// Arrangement.
    pub layout: CollageLayout,
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Gap between photos and to the canvas edge (grid, mosaic, frame).
    pub spacing: u32,
    /// Background colour.
    pub background: [u8; 3],
    /// Seed of the pile's random angles and positions.
    pub seed: u64,
    /// Which photo is the large one in the frame layout.
    pub frame_index: usize,
}

impl CollageSpec {
    /// A grid on a white canvas with 8 px spacing.
    pub fn new(layout: CollageLayout, width: u32, height: u32) -> Self {
        Self {
            layout,
            width: width.max(1),
            height: height.max(1),
            spacing: 8,
            background: [255, 255, 255],
            seed: 0,
            frame_index: 0,
        }
    }
}

/// Where one photo goes. `w` and `h` include the pile's border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Index of the photo in the input list.
    pub index: usize,
    /// Centre x in canvas pixels.
    pub cx: f32,
    /// Centre y in canvas pixels.
    pub cy: f32,
    /// Width before rotation.
    pub w: f32,
    /// Height before rotation.
    pub h: f32,
    /// Clockwise rotation in degrees.
    pub angle: f32,
    /// White border width (pile only).
    pub border: f32,
}

impl Placement {
    /// Left, top, right and bottom of the rotated rectangle's bounding box.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let hw = (self.w * cos.abs() + self.h * sin.abs()) / 2.0;
        let hh = (self.w * sin.abs() + self.h * cos.abs()) / 2.0;
        (self.cx - hw, self.cy - hh, self.cx + hw, self.cy + hh)
    }

    fn rect(index: usize, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            index,
            cx: x as f32 + w as f32 / 2.0,
            cy: y as f32 + h as f32 / 2.0,
            w: w as f32,
            h: h as f32,
            angle: 0.0,
            border: 0.0,
        }
    }
}

/// Placements for photos with the given width/height ratios.
pub fn layout(spec: &CollageSpec, aspects: &[f32]) -> Vec<Placement> {
    let n = aspects.len();
    if n == 0 {
        return Vec::new();
    }
    match spec.layout {
        CollageLayout::Grid => grid(spec, n),
        CollageLayout::Mosaic => mosaic(spec, aspects),
        CollageLayout::Pile => pile(spec, aspects),
        CollageLayout::FrameMosaic => frame(spec, n),
    }
}

fn grid(spec: &CollageSpec, n: usize) -> Vec<Placement> {
    let (cw, ch) = (spec.width as f32, spec.height as f32);
    let canvas_aspect = cw / ch;
    let cols = ((n as f32 * canvas_aspect).sqrt().ceil() as usize)
        .clamp(1, n)
        .min((spec.width as usize / 2).max(1));
    let rows = n.div_ceil(cols);
    let gap = spec
        .spacing
        .min(spec.width / (2 * (cols as u32 + 1)))
        .min(spec.height / (2 * (rows as u32 + 1))) as i32;
    let cell_w = ((spec.width as i32 - gap * (cols as i32 + 1)) / cols as i32).max(1);
    let cell_h = ((spec.height as i32 - gap * (rows as i32 + 1)) / rows as i32).max(1);
    let total_h = rows as i32 * cell_h + gap * (rows as i32 - 1);
    let y0 = (spec.height as i32 - total_h) / 2;
    let mut out = Vec::with_capacity(n);
    for r in 0..rows {
        let count = cols.min(n - r * cols);
        let row_w = count as i32 * cell_w + gap * (count as i32 - 1);
        let x0 = (spec.width as i32 - row_w) / 2;
        for c in 0..count {
            out.push(Placement::rect(
                r * cols + c,
                x0 + c as i32 * (cell_w + gap),
                y0 + r as i32 * (cell_h + gap),
                cell_w,
                cell_h,
            ));
        }
    }
    out
}

struct MosaicRow {
    items: Vec<usize>,
    height: f32,
    full: bool,
}

fn mosaic_rows(
    aspects: &[f32],
    avail: f32,
    gap: f32,
    target: f32,
    max_items: usize,
) -> Vec<MosaicRow> {
    let mut rows = Vec::new();
    let mut items: Vec<usize> = Vec::new();
    let mut sum = 0.0_f32;
    for (i, a) in aspects.iter().enumerate() {
        items.push(i);
        sum += a;
        let gaps = gap * (items.len() as f32 - 1.0);
        if sum * target + gaps >= avail || items.len() >= max_items {
            rows.push(MosaicRow {
                height: ((avail - gaps) / sum).max(1.0),
                items: std::mem::take(&mut items),
                full: true,
            });
            sum = 0.0;
        }
    }
    if !items.is_empty() {
        let gaps = gap * (items.len() as f32 - 1.0);
        let justified = ((avail - gaps) / sum).max(1.0);
        let mut height = justified.min(target);
        if height * sum + gaps > avail {
            height = justified;
        }
        rows.push(MosaicRow {
            items,
            height,
            full: false,
        });
    }
    rows
}

fn mosaic_height(rows: &[MosaicRow], gap: f32) -> f32 {
    rows.iter().map(|r| r.height).sum::<f32>() + gap * (rows.len() as f32 + 1.0)
}

/// Rounding each row on its own can add almost half a pixel per row. Return
/// those pixels from the tallest rows so the stack still fits `canvas`.
fn fit_row_heights(heights: &mut [i32], gap: i32, canvas: i32) {
    if heights.is_empty() {
        return;
    }
    let between = gap.saturating_mul((heights.len() as i32 - 1).max(0));
    let budget = canvas.saturating_sub(between);
    let mut sum: i32 = heights.iter().sum();
    while sum > budget {
        let Some(ix) = heights
            .iter()
            .enumerate()
            .filter(|(_, height)| **height > 1)
            .max_by_key(|(_, height)| *height)
            .map(|(ix, _)| ix)
        else {
            break;
        };
        heights[ix] -= 1;
        sum -= 1;
    }
}

/// Rows for the largest target height whose stack still fits `ch`; `None` when even the
/// smallest rows do not fit.
fn fit_mosaic_rows(aspects: &[f32], cw: f32, ch: f32, gap: f32) -> Option<Vec<MosaicRow>> {
    let avail = cw - 2.0 * gap;
    // Every photo needs one pixel, so a row holds a bounded number of them.
    let max_items = (((avail + gap) / (1.0 + gap)).floor() as usize).max(1);
    let (mut lo, mut hi) = (1.0_f32, ch);
    let mut best = mosaic_rows(aspects, avail, gap, lo, max_items);
    if mosaic_height(&best, gap) > ch {
        return None;
    }
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        let rows = mosaic_rows(aspects, avail, gap, mid, max_items);
        if mosaic_height(&rows, gap) <= ch {
            lo = mid;
            best = rows;
        } else {
            hi = mid;
        }
    }
    Some(best)
}

fn mosaic(spec: &CollageSpec, aspects: &[f32]) -> Vec<Placement> {
    let (cw, ch) = (spec.width as f32, spec.height as f32);
    let mut gap = (spec.spacing as f32).min(cw / 8.0).min(ch / 8.0).floor();
    let aspects: Vec<f32> = aspects.iter().map(|a| a.max(0.05)).collect();
    // Dense collages shrink the spacing instead of overflowing the canvas.
    let rows = loop {
        if let Some(rows) = fit_mosaic_rows(&aspects, cw, ch, gap) {
            break rows;
        }
        if gap <= 0.0 {
            let max_items = (cw as usize).max(1);
            break mosaic_rows(&aspects, cw, 0.0, 1.0, max_items);
        }
        gap = (gap / 2.0).floor();
    };
    let mut scale = 1.0_f32;
    let total = mosaic_height(&rows, gap);
    if total > ch {
        let heights: f32 = rows.iter().map(|r| r.height).sum();
        scale = ((ch - gap * (rows.len() as f32 + 1.0)) / heights).max(0.0);
    }
    let gap_px = gap as i32;
    let mut heights: Vec<i32> = rows
        .iter()
        .map(|r| ((r.height * scale).round() as i32).max(1))
        .collect();
    fit_row_heights(&mut heights, gap_px, spec.height as i32);
    let used = heights.iter().sum::<i32>() + gap_px * (rows.len() as i32 - 1).max(0);
    let mut y = (spec.height as i32 - used).max(0) / 2;
    let mut out = Vec::with_capacity(aspects.len());
    for (row, &h) in rows.iter().zip(&heights) {
        let widths: Vec<f32> = row.items.iter().map(|&i| aspects[i] * h as f32).collect();
        let row_w = widths.iter().sum::<f32>() + gap * (row.items.len() as f32 - 1.0);
        let x0 = if row.full && scale >= 1.0 {
            gap
        } else {
            ((cw - row_w) / 2.0).max(0.0)
        };
        let mut spans: Vec<(i32, i32)> = Vec::with_capacity(row.items.len());
        let mut left = x0;
        let mut prev_right = i32::MIN;
        for (k, _) in row.items.iter().enumerate() {
            let l = (left.round() as i32).max(prev_right.saturating_add(gap_px));
            let mut r = (left + widths[k]).round() as i32;
            if row.full && scale >= 1.0 && k + 1 == row.items.len() {
                r = (cw - gap) as i32;
            }
            let r = r.max(l + 1);
            spans.push((l, r));
            prev_right = r;
            left += widths[k] + gap;
        }
        // One-pixel minimums can push a dense row past the edge; slide items back inside.
        let mut limit = spec.width as i32;
        for span in spans.iter_mut().rev() {
            span.1 = span.1.min(limit);
            span.0 = span.0.min(span.1 - 1);
            limit = span.0 - gap_px;
        }
        let mut floor = 0;
        for span in &mut spans {
            span.0 = span.0.max(floor);
            span.1 = span.1.max(span.0 + 1);
            floor = span.1 + gap_px;
        }
        for (&i, (l, r)) in row.items.iter().zip(spans) {
            out.push(Placement::rect(i, l, y, r - l, h));
        }
        y += h + gap_px;
    }
    out.sort_by_key(|p| p.index);
    out
}

fn frame(spec: &CollageSpec, n: usize) -> Vec<Placement> {
    let center = spec.frame_index.min(n - 1);
    let (w, h) = (spec.width as i32, spec.height as i32);
    let gap = (spec.spacing as i32).min(w / 16).min(h / 16);
    let (bw, bh) = (w / 4, h / 4);
    let others: Vec<usize> = (0..n).filter(|i| *i != center).collect();
    let m = others.len();
    // Bands in ring order: top, right, bottom, left. With fewer than four other photos the
    // bands fill in the order top, bottom, right, left and the centre photo grows into the rest.
    let lengths = [w, h - 2 * bh, w, h - 2 * bh];
    let mut counts = [0_usize; 4];
    if m >= 4 {
        counts = [1; 4];
        let extra = m - 4;
        let total: i64 = lengths.iter().map(|l| i64::from(*l)).sum();
        let mut remainders = [(0_i64, 0_usize); 4];
        for (k, len) in lengths.iter().enumerate() {
            let exact = extra as i64 * i64::from(*len);
            counts[k] += (exact / total) as usize;
            remainders[k] = (exact % total, k);
        }
        let left = m - counts.iter().sum::<usize>();
        remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        for &(_, k) in remainders.iter().cycle().take(left) {
            counts[k] += 1;
        }
    } else {
        for &band in [0_usize, 2, 1, 3].iter().take(m) {
            counts[band] = 1;
        }
    }
    let present = |band: usize| counts[band] > 0;
    let (top, right, bottom, left) = (
        if present(0) { bh } else { 0 },
        if present(1) { bw } else { 0 },
        if present(2) { bh } else { 0 },
        if present(3) { bw } else { 0 },
    );
    let mut out = vec![Placement::rect(
        center,
        left + gap / 2,
        top + gap / 2,
        (w - left - right - gap).max(1),
        (h - top - bottom - gap).max(1),
    )];
    let span = h - top - bottom;
    let mut next = others.into_iter();
    for (band, &count) in counts.iter().enumerate() {
        for c in 0..count {
            let Some(i) = next.next() else { break };
            let (x, y, cw, ch) = match band {
                0 | 2 => {
                    let (x0, x1) = split(w, count, c);
                    let y = if band == 0 { 0 } else { h - bh };
                    (x0, y, x1 - x0, bh)
                }
                _ => {
                    let (y0, y1) = split(span, count, c);
                    let x = if band == 3 { 0 } else { w - bw };
                    (x, top + y0, bw, y1 - y0)
                }
            };
            out.push(inset(Placement::rect(i, x, y, cw, ch), gap));
        }
    }
    out.sort_by_key(|p| p.index);
    out
}

fn split(total: i32, count: usize, k: usize) -> (i32, i32) {
    let c = count as i64;
    let a = i64::from(total) * k as i64 / c;
    let b = i64::from(total) * (k as i64 + 1) / c;
    (a as i32, b as i32)
}

fn inset(p: Placement, gap: i32) -> Placement {
    let w = (p.w - gap as f32).max(1.0);
    let h = (p.h - gap as f32).max(1.0);
    Placement { w, h, ..p }
}

fn unit(rng: &mut Pcg32) -> f32 {
    rng.next_u32() as f32 / 4_294_967_296.0
}

fn pile(spec: &CollageSpec, aspects: &[f32]) -> Vec<Placement> {
    let n = aspects.len();
    let (cw, ch) = (spec.width as f32, spec.height as f32);
    let base = ((cw * ch / n as f32).sqrt() * 1.5)
        .min(0.8 * cw.min(ch))
        .max(8.0);
    let mut rng = Pcg32::seed_from_u64(spec.seed);
    let mut out: Vec<Placement> = Vec::with_capacity(n);
    for (index, &aspect) in aspects.iter().enumerate() {
        let a = aspect.max(0.05);
        let (mut w, mut h) = if a >= 1.0 {
            (base, base / a)
        } else {
            (base * a, base)
        };
        let angle = (unit(&mut rng) * 2.0 - 1.0) * PILE_MAX_ANGLE;
        let (sin, cos) = angle.to_radians().sin_cos();
        let (mut hw, mut hh) = (
            (w * cos.abs() + h * sin.abs()) / 2.0,
            (w * sin.abs() + h * cos.abs()) / 2.0,
        );
        let fit = (cw / 2.0 / hw).min(ch / 2.0 / hh).min(1.0);
        if fit < 1.0 {
            w *= fit;
            h *= fit;
            hw *= fit;
            hh *= fit;
        }
        let border = (0.02 * w.min(h)).max(2.0).min(w.min(h) / 4.0);
        let long = w.max(h);
        let mut best: Option<(f32, f32, f32)> = None;
        for _ in 0..PILE_TRIES {
            let cx = hw + unit(&mut rng) * (cw - 2.0 * hw);
            let cy = hh + unit(&mut rng) * (ch - 2.0 * hh);
            let clearance = out
                .iter()
                .map(|p| {
                    let d = ((p.cx - cx).powi(2) + (p.cy - cy).powi(2)).sqrt();
                    d / (0.4 * (long + p.w.max(p.h)) / 2.0)
                })
                .fold(f32::INFINITY, f32::min);
            if best.is_none_or(|(_, _, c)| clearance > c) {
                best = Some((cx, cy, clearance));
            }
            if clearance >= 1.0 {
                break;
            }
        }
        let (cx, cy, _) = best.unwrap_or((cw / 2.0, ch / 2.0, 0.0));
        out.push(Placement {
            index,
            cx,
            cy,
            w,
            h,
            angle,
            border,
        });
    }
    out
}

/// Paints `images` (in input order) onto a new canvas.
pub fn render(spec: &CollageSpec, images: &[RgbaImage]) -> RgbaImage {
    let aspects: Vec<f32> = images
        .iter()
        .map(|i| i.width().max(1) as f32 / i.height().max(1) as f32)
        .collect();
    let mut canvas = RgbaImage::from_pixel(
        spec.width,
        spec.height,
        Rgba([
            spec.background[0],
            spec.background[1],
            spec.background[2],
            255,
        ]),
    );
    for p in layout(spec, &aspects) {
        let Some(img) = images.get(p.index) else {
            continue;
        };
        if spec.layout == CollageLayout::Pile {
            draw_pile_photo(&mut canvas, img, &p);
        } else {
            let (w, h) = (p.w as u32, p.h as u32);
            let cell = cover(img, w, h);
            let x = (p.cx - p.w / 2.0).round() as i64;
            let y = (p.cy - p.h / 2.0).round() as i64;
            image::imageops::replace(&mut canvas, &cell, x, y);
        }
    }
    canvas
}

/// Scales `img` to cover `w × h` and crops the centre.
fn cover(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (w, h) = (w.max(1), h.max(1));
    let (iw, ih) = (img.width().max(1), img.height().max(1));
    let target = w as f32 / h as f32;
    let (cw, ch) = if iw as f32 / ih as f32 > target {
        (((ih as f32 * target).round() as u32).clamp(1, iw), ih)
    } else {
        (iw, ((iw as f32 / target).round() as u32).clamp(1, ih))
    };
    let (x, y) = ((iw - cw) / 2, (ih - ch) / 2);
    let cropped = DynamicImage::ImageRgba8(image::imageops::crop_imm(img, x, y, cw, ch).to_image());
    resize_exact(&cropped, w, h).to_rgba8()
}

fn blend(canvas: &mut RgbaImage, x: u32, y: u32, rgb: [f32; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let px = canvas.get_pixel_mut(x, y);
    for (c, v) in px.0.iter_mut().zip(rgb) {
        *c = (f32::from(*c) * (1.0 - alpha) + v * alpha)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
}

fn draw_pile_photo(canvas: &mut RgbaImage, img: &RgbaImage, p: &Placement) {
    let inner_w = (p.w - 2.0 * p.border).max(1.0);
    let inner_h = (p.h - 2.0 * p.border).max(1.0);
    let photo = cover(img, inner_w.round() as u32, inner_h.round() as u32);
    let (sin, cos) = p.angle.to_radians().sin_cos();
    let soft = (0.01 * canvas.width().min(canvas.height()) as f32).max(2.0);
    let offset = (soft * 0.6, soft);
    let (l, t, r, b) = p.bounds();
    let pad = soft * 2.0 + offset.0.max(offset.1) + 2.0;
    let x0 = ((l - pad).floor().max(0.0)) as u32;
    let y0 = ((t - pad).floor().max(0.0)) as u32;
    let x1 = ((r + pad).ceil().min(canvas.width() as f32)) as u32;
    let y1 = ((b + pad).ceil().min(canvas.height() as f32)) as u32;
    let local = |px: f32, py: f32, cx: f32, cy: f32| {
        let (dx, dy) = (px - cx, py - cy);
        (dx * cos + dy * sin, -dx * sin + dy * cos)
    };
    let inside = |u: f32, v: f32| (p.w / 2.0 - u.abs()).min(p.h / 2.0 - v.abs());
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (su, sv) = local(px, py, p.cx + offset.0, p.cy + offset.1);
            let shadow = ((inside(su, sv) + soft) / (2.0 * soft)).clamp(0.0, 1.0);
            blend(canvas, x, y, [0.0, 0.0, 0.0], 0.35 * shadow);
        }
    }
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (u, v) = local(px, py, p.cx, p.cy);
            let coverage = (inside(u, v) + 0.5).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let (iu, iv) = (u + inner_w / 2.0, v + inner_h / 2.0);
            let rgb = if iu >= 0.0 && iv >= 0.0 && iu < inner_w && iv < inner_h {
                sample(
                    &photo,
                    iu * photo.width() as f32 / inner_w,
                    iv * photo.height() as f32 / inner_h,
                )
            } else {
                [255.0, 255.0, 255.0]
            };
            blend(canvas, x, y, rgb, coverage);
        }
    }
}

fn sample(img: &RgbaImage, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = (img.width(), img.height());
    let fx = (x - 0.5).clamp(0.0, (w - 1) as f32);
    let fy = (y - 0.5).clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let mut out = [0.0_f32; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = f32::from(img.get_pixel(x0, y0).0[c]) * (1.0 - tx)
            + f32::from(img.get_pixel(x1, y0).0[c]) * tx;
        let bottom = f32::from(img.get_pixel(x0, y1).0[c]) * (1.0 - tx)
            + f32::from(img.get_pixel(x1, y1).0[c]) * tx;
        *o = top * (1.0 - ty) + bottom * ty;
    }
    out
}
