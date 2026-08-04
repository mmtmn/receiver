(() => {
  "use strict";
  const GRID = 205;
  const CANVAS = 2160;
  const BACKGROUND = 0xfff2f6f6;
  const COLORS = [0xff392ccd, 0xff529722, 0xffd3562e, 0xffb930ae];
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const frames = (window.OPTICAL_LAB_FRAMES || []).map((encoded) =>
    Uint8Array.from(encoded, (char) => alphabet.indexOf(char)),
  );
  if (!frames.length) throw new Error("frames.js is missing or empty");

  const canvas = document.getElementById("pattern");
  const ctx = canvas.getContext("2d", { alpha: false });
  const pitchSelect = document.getElementById("pitch");
  const fpsSelect = document.getElementById("fps");
  const toggle = document.getElementById("toggle");
  const stage = document.getElementById("stage");
  const rendered = document.getElementById("rendered");
  const displayed = document.getElementById("displayed");
  const frameLabel = document.getElementById("frame");
  const actualLabel = document.getElementById("actual");

  let prepared = [];
  let running = false;
  let frameIndex = 0;
  let nextAt = 0;
  let shownTimes = [];

  function shapeBit(shape, x, y) {
    const coordinate = (x >= 4 ? 1 : 0)
      | (y >= 4 ? 2 : 0)
      | (x % 4 >= 2 ? 4 : 0)
      | (y % 4 >= 2 ? 8 : 0)
      | (x % 2 === 1 ? 16 : 0)
      | (y % 2 === 1 ? 32 : 0);
    let value = coordinate & (shape + 1);
    value ^= value >>> 4;
    value ^= value >>> 2;
    value ^= value >>> 1;
    return value & 1;
  }

  function raster(symbols, pitch) {
    const image = new ImageData(CANVAS, CANVAS);
    const pixels = new Uint32Array(image.data.buffer);
    pixels.fill(BACKGROUND);
    const glyph = Math.max(1, pitch - 1);
    const gridPx = GRID * pitch;
    const margin = Math.floor((CANVAS - gridPx) / 2);
    for (let row = 0; row < GRID; row++) {
      for (let col = 0; col < GRID; col++) {
        const symbol = symbols[row * GRID + col];
        const color = COLORS[symbol >>> 4];
        const shape = symbol & 15;
        for (let py = 0; py < glyph; py++) {
          const logicalY = Math.floor(py * 8 / glyph);
          let dst = (margin + row * pitch + py) * CANVAS + margin + col * pitch;
          for (let px = 0; px < glyph; px++) {
            const logicalX = Math.floor(px * 8 / glyph);
            if (shapeBit(shape, logicalX, logicalY)) pixels[dst + px] = color;
          }
        }
      }
    }
    const offscreen = document.createElement("canvas");
    offscreen.width = CANVAS;
    offscreen.height = CANVAS;
    offscreen.getContext("2d", { alpha: false }).putImageData(image, 0, 0);
    return offscreen;
  }

  function prepare() {
    running = false;
    toggle.textContent = "Start";
    toggle.disabled = true;
    const pitch = Number(pitchSelect.value);
    requestAnimationFrame(() => {
      prepared = frames.map((frame) => raster(frame, pitch));
      frameIndex = 0;
      draw(performance.now());
      rendered.textContent = `${CANVAS}² · ${pitch}px cells`;
      toggle.disabled = false;
      updateDisplayed();
    });
  }

  function draw(now) {
    ctx.drawImage(prepared[frameIndex], 0, 0);
    frameLabel.textContent = String(frameIndex + 1);
    shownTimes.push(now);
    shownTimes = shownTimes.filter((time) => time >= now - 2000);
    actualLabel.textContent = `${(shownTimes.length / 2).toFixed(1)} fps`;
  }

  function tick(now) {
    if (!running) return;
    requestAnimationFrame(tick);
    if (now < nextAt) return;
    frameIndex = (frameIndex + 1) % prepared.length;
    draw(now);
    const interval = 1000 / Number(fpsSelect.value);
    nextAt += interval;
    if (now - nextAt > interval * 2) nextAt = now + interval;
  }

  function updateDisplayed() {
    const rect = canvas.getBoundingClientRect();
    const physical = Math.round(rect.width * devicePixelRatio);
    const cell = physical / GRID;
    displayed.textContent = `${physical}px · ${cell.toFixed(1)}px/cell`;
  }

  toggle.addEventListener("click", () => {
    running = !running;
    toggle.textContent = running ? "Pause" : "Start";
    if (running) {
      shownTimes = [];
      nextAt = performance.now();
      requestAnimationFrame(tick);
    }
  });
  pitchSelect.addEventListener("change", prepare);
  fpsSelect.addEventListener("change", () => { nextAt = performance.now(); });
  document.getElementById("fullscreen").addEventListener("click", () => stage.requestFullscreen());
  window.addEventListener("resize", updateDisplayed);
  prepare();
})();
