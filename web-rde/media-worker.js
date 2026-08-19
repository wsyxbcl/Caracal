// Media Worker (SPEC §5.1): decode + crop off the main thread so scrolling never
// janks. createImageBitmap is the decoder; an OffscreenCanvas does the
// crop/resize. We transfer back only a small crop (or a downscaled frame for the
// preview) ImageBitmap, so the main thread does ~no pixel work — it just blits
// the result into a canvas via a bitmaprenderer context.

const DECODE_W = 1000; // downscale cap: big camera images decode cheap + small
const TILE = 92;
const PAD = 0.6; // context padding around the box in a crop

self.onmessage = async (event) => {
  const { id, kind, file, bbox } = event.data;
  try {
    // imageOrientation:'from-image' => EXIF-upright frame (matches bbox space).
    // (Reading the file off disk happens here — slow on an HDD, but off-thread.)
    const src = await createImageBitmap(file, {
      imageOrientation: "from-image",
      resizeWidth: DECODE_W,
      resizeQuality: "medium",
    });

    if (kind === "frame") {
      // Full downscaled frame for the context preview (box drawn on the main
      // thread, where it's cheap and one-off).
      self.postMessage({ id, ok: true, bitmap: src, width: src.width, height: src.height }, [src]);
      return;
    }

    // kind === "crop": draw the padded box crop into a tile-sized bitmap.
    const off = new OffscreenCanvas(TILE, TILE);
    const ctx = off.getContext("2d");
    const [x, y, w, h] = bbox;
    const bx = x * src.width, by = y * src.height, bw = w * src.width, bh = h * src.height;
    const cx = Math.max(0, bx - bw * PAD), cy = Math.max(0, by - bh * PAD);
    const cw = Math.min(src.width - cx, bw * (1 + 2 * PAD));
    const ch = Math.min(src.height - cy, bh * (1 + 2 * PAD));
    const scale = Math.min(TILE / cw, TILE / ch);
    const dw = cw * scale, dh = ch * scale, dx = (TILE - dw) / 2, dy = (TILE - dh) / 2;
    ctx.fillStyle = "#efe9df";
    ctx.fillRect(0, 0, TILE, TILE);
    ctx.drawImage(src, cx, cy, cw, ch, dx, dy, dw, dh);
    ctx.strokeStyle = "#ffcc33";
    ctx.lineWidth = 2;
    ctx.strokeRect(dx + (bx - cx) * scale, dy + (by - cy) * scale, bw * scale, bh * scale);
    src.close();

    // Return a Blob, not an ImageBitmap: the tile shows it in an <img>, which
    // paints into the grid layer. A <canvas> per tile would each become its own
    // compositor layer — ~170 of them freezes the software compositor (SPEC §5,
    // confirmed by trace: 156-171 layers, 2.6s commit stalls).
    const blob = await off.convertToBlob({ type: "image/png" });
    self.postMessage({ id, ok: true, blob });
  } catch {
    self.postMessage({ id, ok: false }); // e.g. video file or unreadable image
  }
};
