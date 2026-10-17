//! Clone-heal retouching (SPEC §6.6.3, P1).

use super::geometry::sample_bilinear;
use crate::color::smoothstep;
use image::Rgba32FImage;
use lb_core::RetouchSpot;

/// Copies each spot's source disk onto its target disk with a feathered edge
/// (`radius · 0.2` wide). Spots apply in order, each reading the result of the previous ones.
/// Source positions outside the image are clamped to the nearest edge pixel, and target pixels
/// outside the image are simply not touched.
pub fn retouch(img: &mut Rgba32FImage, spots: &[RetouchSpot]) {
    for spot in spots {
        apply_spot(img, spot);
    }
}

fn apply_spot(img: &mut Rgba32FImage, spot: &RetouchSpot) {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let (wf, hf) = (f64::from(w), f64::from(h));
    let long = wf.max(hf);
    let radius = f64::from(spot.radius) * long;
    if radius <= 0.0 {
        return;
    }
    let feather = radius * 0.2;
    let dst = (f64::from(spot.dst_x) * wf, f64::from(spot.dst_y) * hf);
    let src = (f64::from(spot.src_x) * wf, f64::from(spot.src_y) * hf);

    let x0 = (dst.0 - radius).floor().max(0.0) as u32;
    let y0 = (dst.1 - radius).floor().max(0.0) as u32;
    let x1 = ((dst.0 + radius).ceil().max(0.0) as u32).min(w);
    let y1 = ((dst.1 + radius).ceil().max(0.0) as u32).min(h);

    let mut updates = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let (dx, dy) = (cx - dst.0, cy - dst.1);
            let d = (dx * dx + dy * dy).sqrt();
            if d >= radius {
                continue;
            }
            let alpha = 1.0 - smoothstep((radius - feather) as f32, radius as f32, d as f32);
            let from = sample_bilinear(img, src.0 + dx, src.1 + dy).0;
            let cur = img.get_pixel(x, y).0;
            let mut out = cur;
            for c in 0..3 {
                out[c] = cur[c] + (from[c] - cur[c]) * alpha;
            }
            updates.push((x, y, out));
        }
    }
    for (x, y, px) in updates {
        img.put_pixel(x, y, image::Rgba(px));
    }
}
