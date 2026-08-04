//! Deterministic display/camera degradations used by the offline experiment.

use image::{Rgb, RgbImage, imageops};

#[derive(Clone, Copy, Debug)]
pub struct ChannelConfig {
    /// Gaussian point-spread approximation in captured pixels.
    pub blur_sigma: f32,
    /// Per-channel approximately Gaussian sensor noise, 0--255 scale.
    pub noise_sigma: f32,
    /// White-balance / display-channel gain.
    pub gains: [f32; 3],
    /// Display/camera transfer curve. 1.0 is unchanged.
    pub gamma: f32,
    /// A square white occlusion as a fraction of grid width. Zero disables it.
    pub occlusion: f32,
    pub seed: u64,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            blur_sigma: 0.65,
            noise_sigma: 4.0,
            gains: [1.05, 0.96, 1.08],
            gamma: 1.04,
            occlusion: 0.0,
            seed: 1,
        }
    }
}

pub fn apply(source: &RgbImage, config: ChannelConfig) -> RgbImage {
    let mut corrected = source.clone();
    for pixel in corrected.pixels_mut() {
        for channel in 0..3 {
            let normalized = pixel[channel] as f32 / 255.0;
            pixel[channel] = (normalized.powf(config.gamma) * config.gains[channel] * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }

    let mut output = if config.blur_sigma > 0.0 {
        imageops::blur(&corrected, config.blur_sigma)
    } else {
        corrected
    };

    if config.noise_sigma > 0.0 {
        for (index, pixel) in output.pixels_mut().enumerate() {
            for channel in 0..3 {
                // Difference of two independent uniforms has variance 1/6;
                // sqrt(6) scales it to the requested standard deviation.
                let a = unit_float(hash(config.seed ^ ((index * 3 + channel) as u64 * 2)));
                let b = unit_float(hash(config.seed ^ ((index * 3 + channel) as u64 * 2 + 1)));
                let noise = (a - b) * 6.0_f32.sqrt() * config.noise_sigma;
                pixel[channel] = (pixel[channel] as f32 + noise).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    if config.occlusion > 0.0 {
        let side = (output.width() as f32 * config.occlusion.clamp(0.0, 0.25)).round() as u32;
        let x0 = output.width() * 57 / 100;
        let y0 = output.height() * 39 / 100;
        for y in y0..(y0 + side).min(output.height()) {
            for x in x0..(x0 + side).min(output.width()) {
                output.put_pixel(x, y, Rgb([250, 250, 247]));
            }
        }
    }
    output
}

fn hash(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn unit_float(value: u64) -> f32 {
    ((value >> 40) as u32) as f32 / (1_u32 << 24) as f32
}
