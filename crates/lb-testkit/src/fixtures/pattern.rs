//! Resolution-independent procedural image patterns (deterministic, no RNG crate).

use super::manifest::Pattern;
use image::RgbaImage;

/// SplitMix64: tiny, seedable, platform-independent PRNG.
#[derive(Debug, Clone)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    /// Creates a generator from `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Clone, Copy)]
struct Shape {
    cx: f32,
    cy: f32,
    r: f32,
    round: bool,
    color: [u8; 3],
}

fn channel(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

fn base_color(seed: u64, u: f32, v: f32) -> [u8; 3] {
    let mut rng = SplitMix64::new(seed);
    let (pa, pb, pc) = (rng.next_f32(), rng.next_f32(), rng.next_f32());
    let r = 0.15 + 0.7 * ((u * (0.6 + pa) + v * pb * 0.5).fract());
    let g = 0.15 + 0.7 * ((v * (0.6 + pb) + u * pc * 0.5).fract());
    let b = 0.15 + 0.7 * (((u + v) * 0.5 * (0.6 + pc) + pa).fract());
    [channel(r), channel(g), channel(b)]
}

fn shapes(seed: u64) -> Vec<Shape> {
    let mut rng = SplitMix64::new(seed ^ 0xA5A5_5A5A);
    (0..4)
        .map(|_| Shape {
            cx: 0.15 + 0.7 * rng.next_f32(),
            cy: 0.15 + 0.7 * rng.next_f32(),
            r: 0.08 + 0.12 * rng.next_f32(),
            round: rng.next_u64() & 1 == 0,
            color: [
                channel(rng.next_f32()),
                channel(rng.next_f32()),
                channel(rng.next_f32()),
            ],
        })
        .collect()
}

/// Renders `pattern` at `width`×`height`. Content depends only on normalised coordinates, so
/// resized variants of the same seed look alike.
pub fn render(pattern: Pattern, seed: u64, width: u32, height: u32) -> RgbaImage {
    let scene_shapes = if pattern == Pattern::Scene {
        shapes(seed)
    } else {
        Vec::new()
    };
    RgbaImage::from_fn(width, height, |x, y| {
        let u = (x as f32 + 0.5) / width as f32;
        let v = (y as f32 + 0.5) / height as f32;
        let rgb = match pattern {
            Pattern::Gradient => base_color(seed, u, v),
            Pattern::Checker => {
                let on = ((u * 8.0) as u32 + (v * 8.0) as u32).is_multiple_of(2);
                if on { [235, 235, 235] } else { [20, 20, 20] }
            }
            Pattern::Patches => {
                let (col, row) = ((u * 6.0) as u64, (v * 4.0) as u64);
                let mut rng = SplitMix64::new(seed.wrapping_add(row * 6 + col));
                [
                    channel(rng.next_f32()),
                    channel(rng.next_f32()),
                    channel(rng.next_f32()),
                ]
            }
            Pattern::Scene => {
                let mut c = base_color(seed, u, v);
                for s in &scene_shapes {
                    let (dx, dy) = (u - s.cx, v - s.cy);
                    let inside = if s.round {
                        dx * dx + dy * dy <= s.r * s.r
                    } else {
                        dx.abs() <= s.r && dy.abs() <= s.r
                    };
                    if inside {
                        c = s.color;
                    }
                }
                c
            }
        };
        image::Rgba([rgb[0], rgb[1], rgb[2], 255])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_deterministic_and_seed_sensitive() {
        for p in [
            Pattern::Gradient,
            Pattern::Patches,
            Pattern::Checker,
            Pattern::Scene,
        ] {
            assert_eq!(render(p, 7, 32, 24), render(p, 7, 32, 24), "{p:?}");
        }
        assert_ne!(
            render(Pattern::Scene, 7, 32, 24),
            render(Pattern::Scene, 8, 32, 24)
        );
    }

    #[test]
    fn splitmix_matches_reference_vector() {
        let mut r = SplitMix64::new(1234567);
        assert_eq!(r.next_u64(), 6457827717110365317);
        assert_eq!(r.next_u64(), 3203168211198807973);
    }
}
