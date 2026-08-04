//! Rendering and decoding for the proposed color/icon matrix.
//!
//! This is deliberately not a production barcode detector. The simulator
//! supplies the nominal quadrilateral, as a tracker would after detecting
//! fiducials. A configurable registration offset measures sensitivity to an
//! imperfect tracker. That separation lets this lab answer the first question:
//! is enough information optically distinguishable at all?

use image::{Rgb, RgbImage};
use rayon::prelude::*;

use crate::code::{DATA_CELLS, DATA_ROWS, GRID_COLS, GRID_ROWS};

pub const BACKGROUND: [u8; 3] = [246, 246, 242];
pub const PALETTE: [[u8; 3]; 4] = [[205, 44, 57], [34, 151, 82], [46, 86, 211], [174, 48, 185]];

#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub canvas_px: u32,
    pub pitch_px: u32,
    /// Fraction of grid width removed from each top corner. The lower edge is
    /// left at full width, producing a controlled keystone trapezoid.
    pub perspective: f32,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            canvas_px: 2160,
            pitch_px: 9,
            perspective: 0.04,
        }
    }
}

impl Geometry {
    pub fn grid_px(self) -> f32 {
        (GRID_COLS as u32 * self.pitch_px) as f32
    }

    fn top(self) -> f32 {
        ((self.canvas_px as f32 - self.grid_px()) * 0.5).floor()
    }

    /// Map a point in the canonical square grid into the simulated capture.
    pub fn map(self, canonical_x: f32, canonical_y: f32) -> (f32, f32) {
        let grid = self.grid_px();
        let v = canonical_y / grid;
        let base_left = ((self.canvas_px as f32 - grid) * 0.5).floor();
        let inset = grid * self.perspective * (1.0 - v);
        let left = base_left + inset;
        let width = grid - 2.0 * inset;
        (left + canonical_x / grid * width, self.top() + canonical_y)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DecodeThresholds {
    pub max_shape_errors: u8,
    /// The runner-up analog shape cost must exceed the winner by this factor.
    pub min_shape_ratio: f32,
    /// The runner-up color distance must exceed the winner by this factor.
    pub min_color_ratio: f32,
    /// Simulated tracker error in captured pixels.
    pub registration_x: f32,
    pub registration_y: f32,
}

impl Default for DecodeThresholds {
    fn default() -> Self {
        Self {
            max_shape_errors: 13,
            min_shape_ratio: 1.08,
            min_color_ratio: 1.16,
            registration_x: 0.0,
            registration_y: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VisualDecode {
    pub symbols: Vec<Option<u8>>,
    pub calibrated_background: [f32; 3],
    pub calibrated_palette: [[f32; 3]; 4],
}

/// A 16-word, 64-bit first-order code. Every pair differs in 32 positions.
/// Coordinate bits are ordered from broad half-planes toward fine stripes so
/// the most common masks retain useful low-frequency structure under blur.
pub fn shape_bit(shape: u8, x: usize, y: usize) -> bool {
    let coordinate = ((x >= 4) as u8)
        | (((y >= 4) as u8) << 1)
        | (((x % 4 >= 2) as u8) << 2)
        | (((y % 4 >= 2) as u8) << 3)
        | (((x % 2 == 1) as u8) << 4)
        | (((y % 2 == 1) as u8) << 5);
    let mask = shape + 1;
    (coordinate & mask).count_ones() & 1 == 1
}

pub fn render(symbols: &[u8], geometry: Geometry) -> RgbImage {
    assert_eq!(symbols.len(), GRID_COLS * GRID_ROWS);
    let pitch = geometry.pitch_px as usize;
    let glyph = pitch.saturating_sub(1).max(1);
    let grid_px = GRID_COLS * pitch;
    let mut canonical = RgbImage::from_pixel(grid_px as u32, grid_px as u32, Rgb(BACKGROUND));

    for row in 0..GRID_ROWS {
        for col in 0..GRID_COLS {
            let symbol = symbols[row * GRID_COLS + col];
            let color = PALETTE[(symbol >> 4) as usize];
            let shape = symbol & 0x0f;
            for py in 0..glyph {
                let logical_y = py * 8 / glyph;
                for px in 0..glyph {
                    let logical_x = px * 8 / glyph;
                    if shape_bit(shape, logical_x, logical_y) {
                        canonical.put_pixel(
                            (col * pitch + px) as u32,
                            (row * pitch + py) as u32,
                            Rgb(color),
                        );
                    }
                }
            }
        }
    }

    warp_to_capture(&canonical, geometry)
}

fn warp_to_capture(source: &RgbImage, geometry: Geometry) -> RgbImage {
    let width = geometry.canvas_px as usize;
    let height = geometry.canvas_px as usize;
    let top = geometry.top();
    let grid = geometry.grid_px();
    let source_width = source.width() as usize;
    let source_bytes = source.as_raw();
    let mut output = vec![0_u8; width * height * 3];

    output
        .par_chunks_mut(width * 3)
        .enumerate()
        .for_each(|(y, row)| {
            for pixel in row.chunks_exact_mut(3) {
                pixel.copy_from_slice(&BACKGROUND);
            }
            let v = (y as f32 + 0.5 - top) / grid;
            if !(0.0..1.0).contains(&v) {
                return;
            }
            let inset = grid * geometry.perspective * (1.0 - v);
            let left = ((width as f32 - grid) * 0.5).floor() + inset;
            let span = grid - 2.0 * inset;
            let x_start = left.max(0.0).floor() as usize;
            let x_end = (left + span).min(width as f32).ceil() as usize;
            let source_y = ((v * source_width as f32) as usize).min(source_width - 1);
            for x in x_start..x_end {
                let u = (x as f32 + 0.5 - left) / span;
                let source_x = ((u * source_width as f32) as usize).min(source_width - 1);
                let src = (source_y * source_width + source_x) * 3;
                row[x * 3..x * 3 + 3].copy_from_slice(&source_bytes[src..src + 3]);
            }
        });

    RgbImage::from_raw(geometry.canvas_px, geometry.canvas_px, output).unwrap()
}

pub fn decode(image: &RgbImage, geometry: Geometry, thresholds: DecodeThresholds) -> VisualDecode {
    let (background, palette) = calibrate(image, geometry, thresholds);
    let symbols: Vec<Option<u8>> = (0..DATA_CELLS)
        .into_par_iter()
        .map(|index| {
            let row = index / GRID_COLS;
            let col = index % GRID_COLS;
            classify_cell(image, geometry, thresholds, row, col, background, palette)
        })
        .collect();
    VisualDecode {
        symbols,
        calibrated_background: background,
        calibrated_palette: palette,
    }
}

fn calibrate(
    image: &RgbImage,
    geometry: Geometry,
    thresholds: DecodeThresholds,
) -> ([f32; 3], [[f32; 3]; 4]) {
    let mut background_sum = [0_f64; 3];
    let mut background_count = 0_u64;
    let mut palette_sum = [[0_f64; 3]; 4];
    let mut palette_count = [0_u64; 4];
    let row = DATA_ROWS;
    for col in 0..GRID_COLS {
        let symbol = (col % 64) as u8;
        let shape = symbol & 0x0f;
        let color = (symbol >> 4) as usize;
        for y in 0..8 {
            for x in 0..8 {
                let sample = sample_cell_pixel(image, geometry, thresholds, row, col, x, y);
                if shape_bit(shape, x, y) {
                    for channel in 0..3 {
                        palette_sum[color][channel] += sample[channel] as f64;
                    }
                    palette_count[color] += 1;
                } else {
                    for channel in 0..3 {
                        background_sum[channel] += sample[channel] as f64;
                    }
                    background_count += 1;
                }
            }
        }
    }

    let background = std::array::from_fn(|channel| {
        (background_sum[channel] / background_count.max(1) as f64) as f32
    });
    let palette = std::array::from_fn(|color| {
        std::array::from_fn(|channel| {
            (palette_sum[color][channel] / palette_count[color].max(1) as f64) as f32
        })
    });
    (background, palette)
}

#[allow(clippy::too_many_arguments)]
fn classify_cell(
    image: &RgbImage,
    geometry: Geometry,
    thresholds: DecodeThresholds,
    row: usize,
    col: usize,
    background: [f32; 3],
    palette: [[f32; 3]; 4],
) -> Option<u8> {
    let mut samples = [[0_f32; 3]; 64];
    let mut sample_mean = [0_f32; 3];
    for y in 0..8 {
        for x in 0..8 {
            let index = y * 8 + x;
            let pixel = sample_cell_pixel(image, geometry, thresholds, row, col, x, y);
            samples[index] = pixel.map(|value| value as f32);
            for channel in 0..3 {
                sample_mean[channel] += samples[index][channel] / 64.0;
            }
        }
    }

    // Every shape has exactly 32 foreground samples, so the foreground color
    // can be estimated before knowing the shape: mean=(foreground+background)/2.
    let foreground_estimate = std::array::from_fn(|channel| {
        (2.0 * sample_mean[channel] - background[channel]).clamp(0.0, 255.0)
    });
    let mut ranked_colors = [(usize::MAX, f32::INFINITY); 2];
    for (color, candidate) in palette.iter().enumerate() {
        let distance = color_distance(foreground_estimate, *candidate);
        if distance < ranked_colors[0].1 {
            ranked_colors[1] = ranked_colors[0];
            ranked_colors[0] = (color, distance);
        } else if distance < ranked_colors[1].1 {
            ranked_colors[1] = (color, distance);
        }
    }
    let color_ratio = ranked_colors[1].1 / ranked_colors[0].1.max(1.0);
    if color_ratio < thresholds.min_color_ratio {
        return None;
    }
    let foreground_color = palette[ranked_colors[0].0];

    let mut ranked_shapes = [(u8::MAX, f32::INFINITY); 2];
    for shape in 0..16_u8 {
        let mut cost = 0_f32;
        for y in 0..8 {
            for x in 0..8 {
                let expected = if shape_bit(shape, x, y) {
                    foreground_color
                } else {
                    background
                };
                cost += color_distance(samples[y * 8 + x], expected);
            }
        }
        if cost < ranked_shapes[0].1 {
            ranked_shapes[1] = ranked_shapes[0];
            ranked_shapes[0] = (shape, cost);
        } else if cost < ranked_shapes[1].1 {
            ranked_shapes[1] = (shape, cost);
        }
    }
    let shape = ranked_shapes[0].0;
    let shape_ratio = ranked_shapes[1].1 / ranked_shapes[0].1.max(1.0);
    if shape_ratio < thresholds.min_shape_ratio {
        return None;
    }
    let mut shape_errors = 0_u8;
    for y in 0..8 {
        for x in 0..8 {
            let background_distance = color_distance(samples[y * 8 + x], background);
            let foreground_distance = color_distance(samples[y * 8 + x], foreground_color);
            let observed_foreground = foreground_distance < background_distance;
            shape_errors += (observed_foreground != shape_bit(shape, x, y)) as u8;
        }
    }
    if shape_errors > thresholds.max_shape_errors {
        return None;
    }
    Some(((ranked_colors[0].0 as u8) << 4) | shape)
}

fn sample_cell_pixel(
    image: &RgbImage,
    geometry: Geometry,
    thresholds: DecodeThresholds,
    row: usize,
    col: usize,
    logical_x: usize,
    logical_y: usize,
) -> [u8; 3] {
    let pitch = geometry.pitch_px as f32;
    let glyph = geometry.pitch_px.saturating_sub(1).max(1) as usize;
    let sample_position = |logical: usize| {
        let start = (logical * glyph).div_ceil(8);
        let end = ((logical + 1) * glyph).div_ceil(8);
        (start + end) as f32 * 0.5
    };
    let canonical_x = col as f32 * pitch + sample_position(logical_x);
    let canonical_y = row as f32 * pitch + sample_position(logical_y);
    let (x, y) = geometry.map(canonical_x, canonical_y);
    bilinear(
        image,
        // Geometry maps continuous pixel centers; image coordinates address
        // integer pixel centers, hence the half-pixel conversion here.
        x - 0.5 + thresholds.registration_x,
        y - 0.5 + thresholds.registration_y,
    )
}

fn bilinear(image: &RgbImage, x: f32, y: f32) -> [u8; 3] {
    let x = x.clamp(0.0, image.width().saturating_sub(1) as f32);
    let y = y.clamp(0.0, image.height().saturating_sub(1) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = image.get_pixel(x0, y0).0;
    let p10 = image.get_pixel(x1, y0).0;
    let p01 = image.get_pixel(x0, y1).0;
    let p11 = image.get_pixel(x1, y1).0;
    std::array::from_fn(|channel| {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8
    })
}

fn color_distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    // Green carries more luminance detail in a Bayer camera; weighting it a
    // little more also prevents blue-channel noise from dominating decisions.
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    0.8 * dr * dr + 1.2 * dg * dg + 0.7 * db * db
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code::{decode_frame, deterministic_payload, encode_frame};

    #[test]
    fn shape_code_has_distance_32() {
        for a in 0..16_u8 {
            for b in a + 1..16_u8 {
                let mut distance = 0;
                for y in 0..8 {
                    for x in 0..8 {
                        distance += (shape_bit(a, x, y) != shape_bit(b, x, y)) as usize;
                    }
                }
                assert_eq!(distance, 32, "shape {a} vs {b}");
            }
        }
    }

    #[test]
    fn clean_render_decodes() {
        let payload = deterministic_payload(9);
        let frame = encode_frame(9, &payload).unwrap();
        let geometry = Geometry {
            perspective: 0.0,
            ..Geometry::default()
        };
        let image = render(&frame.symbols, geometry);
        let visual = decode(&image, geometry, DecodeThresholds::default());
        let decoded = decode_frame(&visual.symbols).unwrap();
        assert_eq!(decoded.payload, payload);
    }
}
