//! 512 px tile pyramid for images over 100 megapixels (SPEC-VIEWER F32, §V9).

use crate::viewer::RectF;

/// Decoded tile edge. Coarser pyramid levels cover `TILE_PX << level` source pixels.
pub const TILE_PX: u32 = 512;
/// Pixel count above which zoom uses tiles instead of one capped L3 image.
pub const HUGE_PIXELS: u64 = 100_000_000;

/// One tile of the pyramid, in full-resolution coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileRef {
    /// 0 is full resolution. Level `n` covers `TILE_PX << n` source pixels per tile.
    pub level: u32,
    /// Column in that level.
    pub col: u32,
    /// Row in that level.
    pub row: u32,
    /// Left edge in full-resolution pixels.
    pub x: u32,
    /// Top edge in full-resolution pixels.
    pub y: u32,
    /// Width in full-resolution pixels, clipped to the image.
    pub w: u32,
    /// Height in full-resolution pixels, clipped to the image.
    pub h: u32,
}

/// RGBA8 pixels for one tile. Never a full-image buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTile {
    /// Which pyramid cell this is.
    pub tile: TileRef,
    /// Decoded width. At most [`TILE_PX`].
    pub width: u32,
    /// Decoded height. At most [`TILE_PX`].
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// Reads a single pyramid tile. Implementations must not decode the rest of the image.
pub trait TileSource {
    /// Full-resolution size `(width, height)`.
    fn size(&self) -> (u32, u32);
    /// Decodes one tile to RGBA8 at most [`TILE_PX`] on a side.
    fn decode_tile(&mut self, tile: TileRef) -> crate::error::Result<Vec<u8>>;
}

/// Whether zoom of this image should use the tile pyramid (more than 100 megapixels).
#[must_use]
pub fn needs_tiles(width: u32, height: u32) -> bool {
    u64::from(width).saturating_mul(u64::from(height)) > HUGE_PIXELS
}

/// Tiles whose full-resolution rectangles cover `viewport` at `scale` screen pixels per source pixel.
#[must_use]
pub fn visible_tiles(size: (u32, u32), viewport: RectF, scale: f32) -> Vec<TileRef> {
    if size.0 == 0 || size.1 == 0 || viewport.w <= 0.0 || viewport.h <= 0.0 {
        return Vec::new();
    }
    let level = level_for_scale(scale, max_level(size));
    let span = TILE_PX << level.min(16);
    let (Some((col0, col1)), Some((row0, row1))) = (
        axis_range(viewport.x, viewport.w, size.0, span),
        axis_range(viewport.y, viewport.h, size.1, span),
    ) else {
        return Vec::new();
    };
    let mut tiles = Vec::new();
    for row in row0..=row1 {
        for col in col0..=col1 {
            tiles.push(tile_at(size, level, col, row, span));
        }
    }
    tiles
}

/// Decodes only the tiles that cover `viewport`.
pub fn decode_visible(
    source: &mut impl TileSource,
    viewport: RectF,
    scale: f32,
) -> crate::error::Result<Vec<DecodedTile>> {
    let size = source.size();
    let mut decoded = Vec::new();
    for tile in visible_tiles(size, viewport, scale) {
        let step = 1u32 << tile.level.min(16);
        let width = tile.w.div_ceil(step).min(TILE_PX);
        let height = tile.h.div_ceil(step).min(TILE_PX);
        let rgba = source.decode_tile(tile)?;
        decoded.push(DecodedTile {
            tile,
            width,
            height,
            rgba,
        });
    }
    Ok(decoded)
}

fn max_level(size: (u32, u32)) -> u32 {
    let long = size.0.max(size.1).max(1);
    let mut level = 0u32;
    let mut span = TILE_PX;
    while span < long && level < 16 {
        span = span.saturating_mul(2);
        level += 1;
    }
    level
}

fn level_for_scale(scale: f32, max_level: u32) -> u32 {
    if scale < 1.0 && scale > 0.0 {
        let level = (-scale.log2()).floor();
        if level > 0.0 {
            return (level as u32).min(max_level);
        }
    }
    0
}

fn axis_range(start: f32, length: f32, limit: u32, span: u32) -> Option<(u32, u32)> {
    if length <= 0.0 || limit == 0 || span == 0 {
        return None;
    }
    let end = start + length;
    let left = start.max(0.0);
    let right = end.min(limit as f32);
    if right <= left {
        return None;
    }
    let first = left.floor().max(0.0) as u32;
    let last = if right == right.floor() {
        (right as u32).saturating_sub(1)
    } else {
        right.floor() as u32
    };
    let first = first.min(limit - 1);
    let last = last.min(limit - 1);
    if last < first {
        return None;
    }
    Some((first / span, last / span))
}

fn tile_at(size: (u32, u32), level: u32, col: u32, row: u32, span: u32) -> TileRef {
    let x = col.saturating_mul(span);
    let y = row.saturating_mul(span);
    TileRef {
        level,
        col,
        row,
        x,
        y,
        w: size.0.saturating_sub(x).min(span),
        h: size.1.saturating_sub(y).min(span),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(tiles: &[TileRef], x: f32, y: f32) -> bool {
        tiles.iter().any(|tile| {
            x >= tile.x as f32
                && y >= tile.y as f32
                && x < (tile.x + tile.w) as f32
                && y < (tile.y + tile.h) as f32
        })
    }

    #[test]
    fn viewer_tiles_cover_viewport() {
        let size = (20_000, 15_000);
        assert!(needs_tiles(size.0, size.1));
        assert!(!needs_tiles(4_000, 3_000));
        let view = RectF {
            x: 100.0,
            y: 200.0,
            w: 900.0,
            h: 700.0,
        };
        let tiles = visible_tiles(size, view, 1.0);
        assert!(!tiles.is_empty());
        let mut y = view.y + 0.5;
        while y < view.y + view.h {
            let mut x = view.x + 0.5;
            while x < view.x + view.w {
                assert!(covers(&tiles, x, y), "gap at {x},{y}");
                x += 64.0;
            }
            y += 64.0;
        }
        for tile in &tiles {
            let step = 1u32 << tile.level;
            assert!(tile.w.div_ceil(step) <= TILE_PX);
            assert!(tile.h.div_ceil(step) <= TILE_PX);
            let overlaps = (tile.x as f32) < view.x + view.w
                && view.x < (tile.x + tile.w) as f32
                && (tile.y as f32) < view.y + view.h
                && view.y < (tile.y + tile.h) as f32;
            assert!(overlaps);
        }
        let coarse = visible_tiles(size, view, 0.25);
        assert!(coarse.len() <= tiles.len());
        let mut y = view.y + 0.5;
        while y < view.y + view.h {
            let mut x = view.x + 0.5;
            while x < view.x + view.w {
                assert!(covers(&coarse, x, y), "coarse gap at {x},{y}");
                x += 128.0;
            }
            y += 128.0;
        }
    }

    struct Counting {
        size: (u32, u32),
        calls: Vec<(u32, u32, u32)>,
    }

    impl TileSource for Counting {
        fn size(&self) -> (u32, u32) {
            self.size
        }

        fn decode_tile(&mut self, tile: TileRef) -> crate::error::Result<Vec<u8>> {
            self.calls.push((tile.level, tile.col, tile.row));
            let step = 1u32 << tile.level;
            let width = tile.w.div_ceil(step).min(TILE_PX);
            let height = tile.h.div_ceil(step).min(TILE_PX);
            let mut rgba = vec![0u8; usize::try_from(width * height * 4).unwrap()];
            // One source pixel's colour, so the test can see that this tile was really decoded.
            if width > 0 && height > 0 {
                rgba[0] = u8::try_from(tile.x & 255).unwrap_or(0);
                rgba[1] = u8::try_from(tile.y & 255).unwrap_or(0);
                rgba[2] = u8::try_from(tile.level).unwrap_or(0);
                rgba[3] = 255;
            }
            Ok(rgba)
        }
    }

    #[test]
    fn viewer_tiles_only_visible_decoded() {
        let size = (20_000, 15_000);
        let mut source = Counting {
            size,
            calls: Vec::new(),
        };
        let view = RectF {
            x: 600.0,
            y: 700.0,
            w: 100.0,
            h: 80.0,
        };
        let decoded = decode_visible(&mut source, view, 1.0).unwrap();
        assert_eq!(decoded.len(), 1, "a 100 px window is one 512 px tile");
        assert_eq!(source.calls, vec![(0, 1, 1)]);
        assert_eq!(decoded[0].rgba[0], u8::try_from(512u32 & 255).unwrap());
        assert_eq!(decoded[0].width, TILE_PX);
        let all = 20_000u32.div_ceil(TILE_PX) * 15_000u32.div_ceil(TILE_PX);
        assert!(source.calls.len() as u32 * 20 < all);
        let wide = RectF {
            x: 0.0,
            y: 0.0,
            w: 20_000.0,
            h: 15_000.0,
        };
        source.calls.clear();
        let zoomed_out = decode_visible(&mut source, wide, 0.05).unwrap();
        assert!(!zoomed_out.is_empty());
        assert!(source.calls.len() < all as usize);
        assert!(source.calls.iter().all(|(level, _, _)| *level > 0));
    }
}
