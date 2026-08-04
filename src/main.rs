use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use optical_capacity_lab::{
    code::{PAYLOAD_BYTES, decode_frame, deterministic_payload, encode_frame},
    experiment::{
        BASELINE_KIB_S, OUTER_CODE_OVERHEAD, SEND_FPS, STABLE_FRAME_FRACTION, TARGET_KIB_S,
        run_sweep, run_trial, scenario_named, scenarios, write_csv, write_markdown,
    },
    visual::{DecodeThresholds, Geometry, decode},
};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the deterministic simulated optical-channel matrix.
    Sweep {
        #[arg(long, default_value_t = 5)]
        trials: usize,
        #[arg(long, default_value = "results/latest.md")]
        report: PathBuf,
        #[arg(long, default_value = "results/latest.csv")]
        csv: PathBuf,
        /// Optional directory for representative PNG captures.
        #[arg(long, default_value = "artifacts")]
        artifacts: PathBuf,
    },
    /// Render and decode one named scenario, saving its simulated capture.
    Render {
        #[arg(default_value = "nominal-9px")]
        scenario: String,
        #[arg(long, default_value = "artifacts/frame.png")]
        output: PathBuf,
        #[arg(long, default_value_t = 1)]
        sequence: u32,
    },
    /// Print the mathematical throughput budget without rendering a frame.
    Budget,
    /// List the built-in channel scenarios.
    Scenarios,
    /// Export compatible pre-encoded frames for the browser field viewer.
    WebFrames {
        #[arg(long, default_value = "web/frames.js")]
        output: PathBuf,
        #[arg(long, default_value_t = 3)]
        count: u32,
    },
    /// Decode an already square-cropped/rectified PNG from a camera recording.
    DecodeImage {
        input: PathBuf,
        #[arg(long, default_value_t = 9)]
        pitch: u32,
        #[arg(long, default_value_t = 0.0)]
        perspective: f32,
        #[arg(long, default_value_t = 0.0)]
        registration_x: f32,
        #[arg(long, default_value_t = 0.0)]
        registration_y: f32,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Sweep {
            trials,
            report,
            csv,
            artifacts,
        } => {
            if trials == 0 {
                bail!("--trials must be at least one");
            }
            let results = run_sweep(trials, Some(&artifacts))?;
            write_markdown(&report, &results)?;
            write_csv(&csv, &results)?;
            println!("wrote {} and {}", report.display(), csv.display());
            for result in results {
                println!(
                    "{:<22} {}/{} recovered, {:>6.1} ms decode, {:>6.0} KiB/s projected",
                    result.scenario.name,
                    result.successes(),
                    result.trials.len(),
                    result.median_decode_ms(),
                    result.projected_kib_s(),
                );
            }
        }
        Command::Render {
            scenario,
            output,
            sequence,
        } => {
            let scenario = scenario_named(&scenario).with_context(|| {
                format!(
                    "unknown scenario; choose one of: {}",
                    scenarios()
                        .iter()
                        .map(|scenario| scenario.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let (result, image) = run_trial(&scenario, sequence)?;
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            image.save(&output)?;
            println!(
                "{}: success={} erasures={} confident_errors={} (shape={}, color={}) decode={:.1}ms -> {}",
                scenario.name,
                result.success,
                result.erased_cells,
                result.confident_symbol_errors,
                result.confident_shape_errors,
                result.confident_color_errors,
                result.decoder_time().as_secs_f64() * 1000.0,
                output.display(),
            );
        }
        Command::Budget => {
            let projected = PAYLOAD_BYTES as f64 * SEND_FPS * STABLE_FRAME_FRACTION
                / OUTER_CODE_OVERHEAD
                / 1024.0;
            println!("useful payload/frame : {PAYLOAD_BYTES} bytes");
            println!("displayed fps        : {SEND_FPS:.0}");
            println!(
                "stable frames        : {:.0}%",
                STABLE_FRAME_FRACTION * 100.0
            );
            println!(
                "outer-code overhead  : {:.0}%",
                (OUTER_CODE_OVERHEAD - 1.0) * 100.0
            );
            println!("projected goodput    : {projected:.1} KiB/s");
            println!(
                "10x target           : {TARGET_KIB_S:.1} KiB/s ({BASELINE_KIB_S:.0} baseline)"
            );
        }
        Command::Scenarios => {
            for scenario in scenarios() {
                println!("{:<22} {}", scenario.name, scenario.description);
            }
        }
        Command::WebFrames { output, count } => {
            if count == 0 {
                bail!("--count must be at least one");
            }
            let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut javascript = String::from(
                "// Generated by `cargo run --release -- web-frames`.\nwindow.OPTICAL_LAB_FRAMES = [\n",
            );
            for sequence in 1..=count {
                let frame = encode_frame(sequence, &deterministic_payload(sequence))?;
                javascript.push_str("  \"");
                for symbol in frame.symbols {
                    javascript.push(alphabet[symbol as usize] as char);
                }
                javascript.push_str("\",\n");
            }
            javascript.push_str("];\n");
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output, javascript)?;
            println!("wrote {} compatible frames to {}", count, output.display());
        }
        Command::DecodeImage {
            input,
            pitch,
            perspective,
            registration_x,
            registration_y,
        } => {
            let image = image::open(&input)?.into_rgb8();
            if image.width() != image.height() {
                bail!(
                    "input must be square-cropped and rectified; got {}x{}",
                    image.width(),
                    image.height()
                );
            }
            let geometry = Geometry {
                canvas_px: image.width(),
                pitch_px: pitch,
                perspective,
            };
            let visual = decode(
                &image,
                geometry,
                DecodeThresholds {
                    registration_x,
                    registration_y,
                    ..DecodeThresholds::default()
                },
            );
            let erased = visual
                .symbols
                .iter()
                .filter(|symbol| symbol.is_none())
                .count();
            let frame = decode_frame(&visual.symbols)?;
            println!(
                "decoded sequence {}: {} payload bytes, {} erased cells, {} corrected errata",
                frame.sequence,
                frame.payload.len(),
                erased,
                frame.diagnostics.corrected_errata,
            );
        }
    }
    Ok(())
}
