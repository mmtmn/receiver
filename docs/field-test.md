# 4K60 field test

The simulator's next gate is captured camera footage. No suitable `/dev/video*`
device was available on the machine that produced the committed sweep, so this
procedure is intentionally left reproducible rather than represented as done.

## Equipment

- sender capable of displaying the square at 9 or 10 physical pixels/cell;
- 120 Hz display for two refreshes per 60 fps optical frame;
- receiver capable of recording 4K60 with exposure and focus locked;
- tripod or rigid phone support for the first run.

## Capture

1. Serve the repository and open `/web/` on the sender.
2. Select 9 px, fullscreen, maximum brightness, and 60 fps.
3. Confirm the page reports at least 9 physical display pixels/cell.
4. Fill the receiver's short camera dimension with the square while retaining
   all four canvas edges.
5. Lock focus, white balance, and exposure if the camera permits it.
6. Record at least ten seconds stationary, then repeat handheld.
7. Repeat at 10 px and at the 7/8 px negative-control profiles.

## Extract and decode

Extract frames without JPEG recompression. For each candidate, locate the four
outer canvas corners, apply a perspective transform, and resize to exactly
2160×2160. Then run:

```bash
cargo run --release -- decode-image rectified.png --pitch 9
```

The command succeeds only if Reed–Solomon reconstruction and the payload CRC
both pass. It prints the embedded sequence number, erased-cell count, and
corrected errata.

## Pass criteria

For a 10-second, 600-frame recording:

- at least 540 frames must pass CRC (54 verified frames/s);
- refresh-transition captures must account for no more than the remaining 10%;
- aggregate projected goodput must remain at least 1,280 KiB/s;
- steady-state decode throughput, including rectification, must reach 60 fps.

Passing stationary but failing handheld supports a controlled-installation
use case, not the original phone-to-phone experience. Failing the 9/10 px runs
means the six-bit alphabet or camera pipeline must change before additional
Rust optimization is worthwhile.
