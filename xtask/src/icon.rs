//! The application icon (`assets/icon.svg`) drawn without an SVG renderer, for packaging.

use image::{Rgba, RgbaImage};

const SAMPLES: u32 = 3;

fn inside_round_rect(x: f32, y: f32, left: f32, top: f32, size: (f32, f32), radius: f32) -> bool {
    let (w, h) = size;
    if x < left || x > left + w || y < top || y > top + h {
        return false;
    }
    let cx = x.clamp(left + radius, left + w - radius);
    let cy = y.clamp(top + radius, top + h - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

fn inside_polygon(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8
}

/// The colour at a point of the 512 x 512 design, `None` where it is transparent.
fn shade(x: f32, y: f32) -> Option<[u8; 3]> {
    if !inside_round_rect(x, y, 16.0, 16.0, (480.0, 480.0), 104.0) {
        return None;
    }
    let t = (y - 16.0) / 480.0;
    let mut color = [
        lerp(0x2f, 0x16, t),
        lerp(0x6f, 0x36, t),
        lerp(0xe4, 0x7c, t),
    ];
    if inside_round_rect(x, y, 96.0, 112.0, (320.0, 288.0), 32.0) {
        color = [0xf4, 0xf7, 0xff];
        if (x - 340.0).powi(2) + (y - 196.0).powi(2) <= 34.0 * 34.0 {
            color = [0xff, 0xc8, 0x3d];
        }
        let far = [
            (96.0, 400.0),
            (232.0, 236.0),
            (316.0, 332.0),
            (364.0, 284.0),
            (416.0, 344.0),
            (416.0, 400.0),
        ];
        if inside_polygon(x, y, &far) {
            color = [0x3b, 0x7d, 0xdd];
        }
        if inside_polygon(x, y, &[(96.0, 400.0), (188.0, 300.0), (268.0, 400.0)]) {
            color = [0x16, 0x36, 0x7c];
        }
    }
    Some(color)
}

/// Renders the icon at `size` x `size` pixels with anti-aliased edges.
pub fn render(size: u32) -> RgbaImage {
    let scale = 512.0 / size as f32;
    RgbaImage::from_fn(size, size, |px, py| {
        let mut sum = [0.0f32; 4];
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let x = (px as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * scale;
                let y = (py as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * scale;
                if let Some([r, g, b]) = shade(x, y) {
                    sum[0] += f32::from(r);
                    sum[1] += f32::from(g);
                    sum[2] += f32::from(b);
                    sum[3] += 1.0;
                }
            }
        }
        let n = (SAMPLES * SAMPLES) as f32;
        if sum[3] == 0.0 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([
                (sum[0] / sum[3]).round() as u8,
                (sum[1] / sum[3]).round() as u8,
                (sum[2] / sum[3]).round() as u8,
                (sum[3] / n * 255.0).round() as u8,
            ])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_transparent_corners_and_a_sun() {
        let icon = render(256);
        assert_eq!(icon.get_pixel(0, 0)[3], 0);
        assert_eq!(icon.get_pixel(255, 255)[3], 0);
        assert_eq!(icon.get_pixel(128, 20)[3], 255);
        assert_eq!(icon.get_pixel(170, 98).0, [0xff, 0xc8, 0x3d, 255]);
        assert_eq!(render(32).dimensions(), (32, 32));
    }
}
