use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::{
    channel::{ChannelConfig, apply},
    code::{DATA_CELLS, PAYLOAD_BYTES, decode_frame, deterministic_payload, encode_frame},
    visual::{DecodeThresholds, Geometry, decode, render},
};

pub const SEND_FPS: f64 = 60.0;
pub const STABLE_FRAME_FRACTION: f64 = 0.90;
pub const OUTER_CODE_OVERHEAD: f64 = 1.03;
pub const BASELINE_KIB_S: f64 = 128.0;
pub const TARGET_KIB_S: f64 = BASELINE_KIB_S * 10.0;

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub description: &'static str,
    pub geometry: Geometry,
    pub channel: ChannelConfig,
    pub thresholds: DecodeThresholds,
    /// Uniform +/- tracker error applied independently on x/y for each trial.
    pub registration_jitter: f32,
}

#[derive(Clone, Debug)]
pub struct TrialResult {
    pub success: bool,
    pub erased_cells: usize,
    pub confident_symbol_errors: usize,
    pub confident_shape_errors: usize,
    pub confident_color_errors: usize,
    pub encode_time: Duration,
    pub render_time: Duration,
    pub channel_time: Duration,
    pub visual_decode_time: Duration,
    pub fec_decode_time: Duration,
    pub error: Option<String>,
}

impl TrialResult {
    pub fn decoder_time(&self) -> Duration {
        self.visual_decode_time + self.fec_decode_time
    }
}

#[derive(Clone, Debug)]
pub struct ScenarioResult {
    pub scenario: Scenario,
    pub trials: Vec<TrialResult>,
}

impl ScenarioResult {
    pub fn successes(&self) -> usize {
        self.trials.iter().filter(|trial| trial.success).count()
    }

    pub fn success_fraction(&self) -> f64 {
        self.successes() as f64 / self.trials.len().max(1) as f64
    }

    pub fn projected_kib_s(&self) -> f64 {
        PAYLOAD_BYTES as f64 * SEND_FPS * STABLE_FRAME_FRACTION * self.success_fraction()
            / OUTER_CODE_OVERHEAD
            / 1024.0
    }

    pub fn median_decode_ms(&self) -> f64 {
        median(
            self.trials
                .iter()
                .map(|trial| trial.decoder_time().as_secs_f64() * 1000.0)
                .collect(),
        )
    }

    pub fn mean_erasure_percent(&self) -> f64 {
        self.trials
            .iter()
            .map(|trial| trial.erased_cells as f64 / DATA_CELLS as f64 * 100.0)
            .sum::<f64>()
            / self.trials.len().max(1) as f64
    }

    pub fn mean_confident_error_percent(&self) -> f64 {
        self.trials
            .iter()
            .map(|trial| trial.confident_symbol_errors as f64 / DATA_CELLS as f64 * 100.0)
            .sum::<f64>()
            / self.trials.len().max(1) as f64
    }
}

pub fn scenarios() -> Vec<Scenario> {
    let nominal_channel = ChannelConfig::default();
    let nominal_thresholds = DecodeThresholds::default();
    vec![
        Scenario {
            name: "clean-9px",
            description: "Control: 9 px pitch, exact registration, no degradation",
            geometry: Geometry {
                perspective: 0.0,
                ..Geometry::default()
            },
            channel: ChannelConfig {
                blur_sigma: 0.0,
                noise_sigma: 0.0,
                gains: [1.0; 3],
                gamma: 1.0,
                ..ChannelConfig::default()
            },
            thresholds: nominal_thresholds,
            registration_jitter: 0.0,
        },
        Scenario {
            name: "nominal-9px",
            description: "9 px pitch, mild focus blur/noise, color drift and 4% keystone",
            geometry: Geometry::default(),
            channel: nominal_channel,
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "nominal-10px",
            description: "10 px pitch (2050 px grid) with otherwise nominal channel",
            geometry: Geometry {
                pitch_px: 10,
                ..Geometry::default()
            },
            channel: nominal_channel,
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "nominal-8px",
            description: "8 px pitch with otherwise nominal channel",
            geometry: Geometry {
                pitch_px: 8,
                ..Geometry::default()
            },
            channel: nominal_channel,
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "nominal-7px",
            description: "7 px pitch with otherwise nominal channel",
            geometry: Geometry {
                pitch_px: 7,
                ..Geometry::default()
            },
            channel: nominal_channel,
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "moderate",
            description: "9 px pitch, 0.9 px blur, sensor noise, 6% keystone, 0.35 px tracking error",
            geometry: Geometry {
                perspective: 0.06,
                ..Geometry::default()
            },
            channel: ChannelConfig {
                blur_sigma: 0.9,
                noise_sigma: 7.0,
                gains: [1.10, 0.91, 1.10],
                gamma: 1.08,
                ..nominal_channel
            },
            thresholds: nominal_thresholds,
            registration_jitter: 0.35,
        },
        Scenario {
            name: "strong-color-drift",
            description: "Nominal optics with deliberately poor white balance and gamma",
            geometry: Geometry::default(),
            channel: ChannelConfig {
                gains: [1.18, 0.82, 1.14],
                gamma: 1.14,
                ..nominal_channel
            },
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "localized-occlusion",
            description: "Nominal channel plus a white occlusion 3% of frame width",
            geometry: Geometry::default(),
            channel: ChannelConfig {
                occlusion: 0.03,
                ..nominal_channel
            },
            thresholds: nominal_thresholds,
            registration_jitter: 0.18,
        },
        Scenario {
            name: "harsh",
            description: "9 px pitch, 1.25 px blur, heavy noise, 8% keystone, 0.6 px tracking error",
            geometry: Geometry {
                perspective: 0.08,
                ..Geometry::default()
            },
            channel: ChannelConfig {
                blur_sigma: 1.25,
                noise_sigma: 11.0,
                gains: [1.14, 0.86, 1.13],
                gamma: 1.12,
                ..nominal_channel
            },
            thresholds: nominal_thresholds,
            registration_jitter: 0.60,
        },
    ]
}

pub fn scenario_named(name: &str) -> Option<Scenario> {
    scenarios()
        .into_iter()
        .find(|scenario| scenario.name == name)
}

pub fn run_trial(scenario: &Scenario, sequence: u32) -> Result<(TrialResult, image::RgbImage)> {
    let payload = deterministic_payload(sequence);
    let started = Instant::now();
    let encoded = encode_frame(sequence, &payload)?;
    let encode_time = started.elapsed();

    let started = Instant::now();
    let clean = render(&encoded.symbols, scenario.geometry);
    let render_time = started.elapsed();

    let mut channel = scenario.channel;
    channel.seed = sequence as u64 * 0x9e37_79b9 + 17;
    let started = Instant::now();
    let captured = apply(&clean, channel);
    let channel_time = started.elapsed();

    let mut thresholds = scenario.thresholds;
    thresholds.registration_x = signed_unit(sequence as u64 * 2 + 1) * scenario.registration_jitter;
    thresholds.registration_y = signed_unit(sequence as u64 * 2 + 2) * scenario.registration_jitter;
    let started = Instant::now();
    let visual = decode(&captured, scenario.geometry, thresholds);
    let visual_decode_time = started.elapsed();

    let erased_cells = visual
        .symbols
        .iter()
        .filter(|symbol| symbol.is_none())
        .count();
    let confident_symbol_errors = visual
        .symbols
        .iter()
        .zip(&encoded.symbols)
        .take(DATA_CELLS)
        .filter(|(actual, expected)| actual.is_some_and(|actual| actual != **expected))
        .count();
    let confident_shape_errors = visual
        .symbols
        .iter()
        .zip(&encoded.symbols)
        .take(DATA_CELLS)
        .filter(|(actual, expected)| {
            actual.is_some_and(|actual| (actual & 0x0f) != (**expected & 0x0f))
        })
        .count();
    let confident_color_errors = visual
        .symbols
        .iter()
        .zip(&encoded.symbols)
        .take(DATA_CELLS)
        .filter(|(actual, expected)| {
            actual.is_some_and(|actual| (actual >> 4) != (**expected >> 4))
        })
        .count();

    let started = Instant::now();
    let decoded = decode_frame(&visual.symbols);
    let fec_decode_time = started.elapsed();
    let (success, error) = match decoded {
        Ok(decoded) if decoded.sequence == sequence && decoded.payload == payload => (true, None),
        Ok(_) => (
            false,
            Some("decoded bytes did not match the transmitted frame".to_string()),
        ),
        Err(error) => (false, Some(format!("{error:#}"))),
    };

    Ok((
        TrialResult {
            success,
            erased_cells,
            confident_symbol_errors,
            confident_shape_errors,
            confident_color_errors,
            encode_time,
            render_time,
            channel_time,
            visual_decode_time,
            fec_decode_time,
            error,
        },
        captured,
    ))
}

pub fn run_sweep(trials: usize, artifact_dir: Option<&Path>) -> Result<Vec<ScenarioResult>> {
    if let Some(path) = artifact_dir {
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    }
    scenarios()
        .into_iter()
        .enumerate()
        .map(|(scenario_index, scenario)| {
            let mut results = Vec::with_capacity(trials);
            for trial_index in 0..trials {
                let sequence = (scenario_index * trials + trial_index + 1) as u32;
                let (result, image) = run_trial(&scenario, sequence)?;
                if trial_index == 0
                    && matches!(scenario.name, "clean-9px" | "nominal-9px" | "harsh")
                    && let Some(path) = artifact_dir
                {
                    image
                        .save(path.join(format!("{}.png", scenario.name)))
                        .with_context(|| format!("save {} artifact", scenario.name))?;
                }
                results.push(result);
            }
            Ok(ScenarioResult {
                scenario,
                trials: results,
            })
        })
        .collect()
}

pub fn write_markdown(path: &Path, results: &[ScenarioResult]) -> Result<()> {
    let mut report = String::new();
    report.push_str("# Latest simulated-channel results\n\n");
    report.push_str("Generated by `cargo run --release -- sweep`. Payloads and channel noise are deterministic; timings are machine-dependent.\n\n");
    report.push_str("## Link budget\n\n");
    report.push_str(&format!(
        "- {} useful bytes per successfully decoded visual frame\n- {:.0} displayed fps\n- {:.0}% stable-frame assumption (the remaining 10% models refresh-transition captures)\n- {:.0}% outer erasure-code overhead\n- target: {:.0} KiB/s, ten times the 128 KiB/s reference\n\n",
        PAYLOAD_BYTES,
        SEND_FPS,
        STABLE_FRAME_FRACTION * 100.0,
        (OUTER_CODE_OVERHEAD - 1.0) * 100.0,
        TARGET_KIB_S,
    ));
    report.push_str("## Results\n\n");
    report.push_str("| scenario | recovered | cell erasures | confident cell errors | median CPU decode | projected goodput | 10× optical target | 60 fps CPU target |\n");
    report.push_str("|---|---:|---:|---:|---:|---:|:---:|:---:|\n");
    for result in results {
        let decode_ms = result.median_decode_ms();
        let projected = result.projected_kib_s();
        report.push_str(&format!(
            "| {} | {}/{} | {:.3}% | {:.4}% | {:.1} ms | {:.0} KiB/s | {} | {} |\n",
            result.scenario.name,
            result.successes(),
            result.trials.len(),
            result.mean_erasure_percent(),
            result.mean_confident_error_percent(),
            decode_ms,
            projected,
            if projected >= TARGET_KIB_S {
                "PASS"
            } else {
                "FAIL"
            },
            if decode_ms <= 1000.0 / SEND_FPS {
                "PASS"
            } else {
                "FAIL"
            },
        ));
    }
    report.push_str("\n## Interpretation\n\n");
    report.push_str(
        "`10× optical target` answers only whether the simulated stable frames carry enough recoverable information. `60 fps CPU target` is deliberately separate: a desktop scalar/SIMD Rust decoder missing 16.7 ms means a real receiver needs the proposed GPU warp/classifier pipeline. Neither result substitutes for recorded 4K60 camera footage; sensor demosaicing, rolling shutter, autofocus, moiré, display subpixels, and device video APIs are not represented by this model.\n\n",
    );
    report.push_str("A field trial is the next gate: display `web/index.html` at 1:1 physical pixels, record it at 4K60, extract stable frames, and replace the synthetic channel with those images.\n");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, report).with_context(|| format!("write {}", path.display()))
}

pub fn write_csv(path: &Path, results: &[ScenarioResult]) -> Result<()> {
    let mut csv = String::from(
        "scenario,trial,success,erased_cells,confident_symbol_errors,confident_shape_errors,confident_color_errors,encode_ms,render_ms,channel_ms,visual_decode_ms,fec_decode_ms,error\n",
    );
    for result in results {
        for (index, trial) in result.trials.iter().enumerate() {
            let error = trial.error.as_deref().unwrap_or("").replace('"', "\"\"");
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},\"{}\"\n",
                result.scenario.name,
                index + 1,
                trial.success,
                trial.erased_cells,
                trial.confident_symbol_errors,
                trial.confident_shape_errors,
                trial.confident_color_errors,
                trial.encode_time.as_secs_f64() * 1000.0,
                trial.render_time.as_secs_f64() * 1000.0,
                trial.channel_time.as_secs_f64() * 1000.0,
                trial.visual_decode_time.as_secs_f64() * 1000.0,
                trial.fec_decode_time.as_secs_f64() * 1000.0,
                error,
            ));
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, csv).with_context(|| format!("write {}", path.display()))
}

fn signed_unit(seed: u64) -> f32 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    ((value >> 40) as f32 / (1_u32 << 24) as f32) * 2.0 - 1.0
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
