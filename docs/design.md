# Experiment design

## Claim under test

The narrow claim is that a 2160-pixel-square capture can distinguish enough
color/icon cells to deliver at least 1,280 KiB/s of verified payload at 60 fps.
It is not a claim that arbitrary browsers and phones can already sustain that
capture mode.

## Capacity construction

| quantity | value |
|---|---:|
| grid | 205 × 205 cells |
| protected data cells | 205 × 204 = 41,820 |
| raw bits per cell | 6 |
| raw coded bytes | 31,365 |
| inner code | 123 × RS(255,204) |
| framed data bytes | 25,092 |
| envelope | 20 bytes |
| useful payload | 25,072 bytes/frame |

Codeword bytes are spatially interleaved before conversion to six-bit cells.
Localized uncertainty therefore removes a small number of bytes from many
codewords instead of destroying one contiguous codeword.

With 60 displayed frames/s, 90% stable frames, and 1.03× outer-code overhead:

```text
25,072 × 60 × 0.90 / 1.03 / 1024 = 1,283.6 KiB/s
```

That is only 0.3% above the formal 10× target. A production profile needs more
margin through a slightly larger grid, a better inner code, a stable-frame rate
above 90%, or a baseline target stated in decimal MB/s.

## Visual alphabet

The lower four bits select one of sixteen balanced 8×8 binary shapes. They are
first-order parity functions whose pairwise Hamming distance is exactly 32 of
64 samples. Broad coordinate bits are ordered before fine stripes to retain
some low-frequency structure under blur.

The upper two bits select one of four saturated colors. Every shape contains 32
foreground and 32 background samples, so the foreground color can be estimated
before the shape is known. The calibration row repeats all 64 combinations and
lets the decoder learn the observed palette and background after channel color
drift.

Classification is conservative:

1. calibrate background and four colors from the final row;
2. estimate a cell's foreground color from its balanced mean;
3. rank shapes using analog color distance, not thresholded pixels alone;
4. erase cells with weak shape/color margins;
5. pass known erasures and remaining errors to the BCH Reed–Solomon decoder;
6. accept a frame only when its envelope and CRC verify.

## Synthetic channel

Each trial renders a high-entropy protected payload, applies a trapezoidal
keystone transform, per-channel gain, gamma, Gaussian blur, deterministic sensor
noise, optional localized occlusion, and subpixel registration error. The
decoder receives the nominal geometry, representing a fiducial tracker, but not
the random registration offset.

The built-in scenarios vary cell pitch from 7–10 captured pixels and increase
blur, noise, color drift, perspective, and tracking error. Timings include
visual classification and Reed–Solomon correction, but not camera acquisition,
fiducial detection, or GPU/CPU image transfer.

## What simulation omits

- Bayer demosaicing and chroma subsampling;
- display subpixels, moiré, and camera/display modulation transfer functions;
- autofocus hunting, exposure adaptation, glare, and hand movement;
- rolling shutter and frames that straddle display refreshes;
- general four-corner detection and lens-distortion correction;
- mobile thermal throttling and browser camera-copy overhead.

The explicit 90% stable-frame factor budgets refresh-transition losses, but it
does not reproduce their pixels. These omissions are why a synthetic PASS is a
reason to run a field test, not evidence that the end-to-end link is complete.

## Production architecture if the field test passes

1. Native CameraX/AVFoundation capture of YUV planes at 4K60.
2. Metal/Vulkan compute for fiducial tracking, perspective warp, calibration,
   and cell confidence scoring.
3. A shared Rust library for inner FEC, outer fountain/Raptor coding, framing,
   chunk reassembly, and SHA-256 verification.
4. A Wasm or WebGPU sender retaining the existing web experience.
5. Multiple modulation profiles selected for actual screen/camera resolution.

