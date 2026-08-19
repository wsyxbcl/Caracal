// Media Worker (SPEC §5.1): decode + crop off the main thread so scrolling never
// janks. createImageBitmap is the decoder; an OffscreenCanvas does the crop. We
// return crops as small PNG Blobs (shown in <img>, which paint into the grid
// layer — a <canvas> per tile would each become its own compositor layer and
// freeze software compositing).
//
// Media-agnostic + batch-by-source: a "cropBatch" decodes ONE source once and
// emits all of its crops. That halves disk reads (many tiles share a source
// image) and is the shape video needs later — open a video once, extract all its
// requested frames in one pass (SPEC §5, video is P3). Only the source-open step
// differs by media type; the crop step is shared.

const DECODE_W = 1000; // downscale cap: big camera images decode cheap + small
const TILE = 92;
const PAD = 0.6; // context padding around the box in a crop

// Draw the padded box crop of `src` into a tile-sized PNG blob, box outlined.
async function cropBlob(src, bbox) {
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
  return off.convertToBlob({ type: "image/png" });
}

// Decode a source image, EXIF-upright (matches MD bbox space), downscaled.
// (The file read off disk happens here — slow on an HDD, but off-thread.)
function decodeSource(file) {
  return createImageBitmap(file, { imageOrientation: "from-image", resizeWidth: DECODE_W, resizeQuality: "medium" });
}

self.onmessage = async (event) => {
  const { id, kind, file, bbox, items } = event.data;
  try {
    if (kind === "frame") {
      // Full downscaled frame for the context preview (box drawn on the main thread).
      const src = await decodeSource(file);
      self.postMessage({ id, ok: true, bitmap: src, width: src.width, height: src.height }, [src]);
      return;
    }
    if (kind === "cropBatch") {
      // One source open -> all its crops. `items` = [{ key, bbox }].
      const src = await decodeSource(file);
      const results = [];
      for (const it of items) results.push({ key: it.key, blob: await cropBlob(src, it.bbox) });
      src.close();
      self.postMessage({ id, ok: true, results });
      return;
    }
    // kind === "crop": a single crop.
    const src = await decodeSource(file);
    const blob = await cropBlob(src, bbox);
    src.close();
    self.postMessage({ id, ok: true, blob });
  } catch {
    self.postMessage({ id, ok: false }); // e.g. video file or unreadable image
  }
};
