# Optical Capacity Lab

A reproducible experiment for the question: **can a screen-to-camera channel
carry ten times Decimen's 128 KiB/s QR goodput?**

This repository tests one concrete design rather than treating “rewrite it in
Rust” as the hypothesis. The proposed frame is a 205×205 color/icon matrix:

- 204 data rows × 205 columns = 41,820 six-bit cells;
- sixteen balanced shapes carry four bits and four calibrated colors carry two;
- the final row cycles through all 64 symbols for per-frame color calibration;
- 123 interleaved RS(255,204) codewords turn 31,365 visual bytes into 25,092
  protected data bytes;
- a small sequence/length/CRC envelope leaves **25,072 useful bytes per frame**.

At 60 displayed fps, 90% stable captures, and 3% outer erasure-code overhead,
that projects to **1,284 KiB/s**—just above the 1,280 KiB/s target.

## Current verdict

The encoding geometry and throughput budget close, but the hardware hypothesis
is **not yet proven**.

The deterministic synthetic channel shows that 9–10 captured pixels per cell
can recover the target under mild degradation. It also shows a narrow margin:
7–8 px cells and moderate blur/tracking error exceed the correction budget.
Release-mode CPU decoding is close to the 16.7 ms/frame deadline on the test
machine, an Intel Core i9-12900K, so a mobile implementation should use a GPU
perspective warp/classifier with Rust handling FEC and framing.

See [the recorded sweep](results/latest.md) for the results and
[the experiment design](docs/design.md) for what the model does and does not
establish.

## Run it

Requires stable Rust. Results in this repository were produced with Rust 1.93.

```bash
cargo test
cargo run --release -- budget
cargo run --release -- sweep
```

The sweep uses deterministic payloads and channel noise. It writes raw data to
`results/latest.csv`, a readable report to `results/latest.md`, and
representative PNGs to the ignored `artifacts/` directory; timings remain
machine-dependent.

Render one scenario:

```bash
cargo run --release -- render nominal-9px --output artifacts/nominal.png
```

List all channel profiles:

```bash
cargo run --release -- scenarios
```

## Field pattern

[`web/index.html`](web/index.html) animates three real, pre-encoded frames at a
selected cell pitch and transmit rate. It has no build step and works from a
static server:

```bash
python3 -m http.server 8000
# open http://localhost:8000/web/
```

Use maximum display brightness and fullscreen, then record the complete square
at 4K60. The page reports the effective physical display pixels per cell.

After extracting a stable camera frame, perspective-rectify and resize the
complete 2160×2160 canvas, then test the actual bytes:

```bash
cargo run --release -- decode-image rectified.png --pitch 9
```

The field procedure and pass/fail rules are in [docs/field-test.md](docs/field-test.md).

Regenerate the browser's compatible frame corpus after changing the wire format:

```bash
cargo run --release -- web-frames
```

## Repository map

- `src/code.rs` — envelope, six-bit packing, spatial interleaving, RS coding.
- `src/visual.rs` — 16-shape codebook, renderer, calibration, soft classifier.
- `src/channel.rs` — deterministic blur, noise, gamma, color drift, occlusion.
- `src/experiment.rs` — scenarios, timing, link budget, Markdown/CSV reports.
- `web/` — no-build fullscreen field-pattern player.
- `results/` — committed reproducible findings.

## Why Rust

Rust is useful here for shared, bounded FEC/framing code, safe parallelism, and
eventual SIMD/Wasm/native builds. It does not create optical bandwidth. The
extra capacity comes from the new six-bit visual alphabet and higher captured
spatial resolution; a production receiver would still need native YUV camera
access and GPU processing.

## License

MIT
