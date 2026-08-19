// Media Worker (SPEC §5.1): decode + crop off the main thread so scrolling never
// janks. createImageBitmap is the decoder; an OffscreenCanvas does the crop. We
// return crops as small PNG Blobs (shown in <img>, which paint into the grid
// layer — a <canvas> per tile would each become its own compositor layer and
// freeze software compositing).
//
// Media-agnostic + batch-by-source: a "cropBatch" decodes ONE source once and
// emits all of its crops. That halves disk reads (many tiles share a source
// image) and is the shape video needs later — open a video once, extract all its
// requested frames in one pass (SPEC §5, video is P3).
//
// File access (SPEC §6.2): two modes. In the File System Access mode the worker
// holds the picked directory *handle* and resolves each media's known path
// lazily — walk root -> subdir -> file, caching directory handles, enumerating a
// single parent directory only as a case-insensitive fallback. NO global file
// index is ever built. In the fallback mode the main thread passes a File object
// (from a <input webkitdirectory> pick) and the worker just decodes it.

const DECODE_W = 1000; // downscale cap: big camera images decode cheap + small
const TILE = 92;
const PAD = 0.6; // context padding around the box in a crop

// ---- File System Access resolution ----------------------------------------
let rootHandle = null;
let stripComponents = 0;            // leading json-path components to drop for the path under root
const dirCache = new Map();         // "a/b/c" (dirs under root) -> FileSystemDirectoryHandle

const splitPath = (p) => p.replace(/\\/g, "/").split("/").filter(Boolean);

async function childDir(parent, name) {
  try { return await parent.getDirectoryHandle(name); }
  catch {
    // Case-insensitive / ambiguity fallback: enumerate THIS directory only.
    const lower = name.toLowerCase();
    for await (const [n, h] of parent.entries()) if (h.kind === "directory" && n.toLowerCase() === lower) return h;
    throw new Error("dir not found: " + name);
  }
}
async function childFile(parent, name) {
  try { return await parent.getFileHandle(name); }
  catch {
    const lower = name.toLowerCase();
    for await (const [n, h] of parent.entries()) if (h.kind === "file" && n.toLowerCase() === lower) return h;
    throw new Error("file not found: " + name);
  }
}

// Resolve a media path (its json path) to a File, via the retained handle.
async function resolveFile(path, strip = stripComponents) {
  if (!rootHandle) throw new Error("no directory handle");
  const parts = splitPath(path).slice(strip);
  const filename = parts.pop();
  let handle = rootHandle, key = "";
  for (const name of parts) {
    key = key ? key + "/" + name : name;
    let h = dirCache.get(key);
    if (!h) { h = await childDir(handle, name); dirCache.set(key, h); }
    handle = h;
  }
  const fh = await childFile(handle, filename);
  return fh.getFile();
}

// Find how many leading path components map to the picked folder, by probing
// sample paths (match the folder's own name first, then brute-force small
// offsets), so the user can pick any ancestor folder.
async function detectOffset(samplePaths) {
  const candidates = [];
  for (const p of samplePaths) {
    const comps = splitPath(p);
    const idx = comps.lastIndexOf(rootHandle.name);
    if (idx >= 0 && idx < comps.length - 1) candidates.push(idx + 1);
  }
  for (let o = 0; o < 8; o++) candidates.push(o); // fallback: shallow offsets
  const seen = new Set();
  for (const off of candidates) {
    if (seen.has(off)) continue; seen.add(off);
    dirCache.clear();
    try { await resolveFile(samplePaths[0], off); return off; } catch { /* wrong offset */ }
  }
  throw new Error("could not align the folder to the json paths");
}

async function sourceFile(data) {
  return data.file || await resolveFile(data.path); // fallback File, or resolve by path
}

// ---- Decode + crop ---------------------------------------------------------
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
function decodeSource(file) {
  return createImageBitmap(file, { imageOrientation: "from-image", resizeWidth: DECODE_W, resizeQuality: "medium" });
}

self.onmessage = async (event) => {
  const { id, kind, file, path, bbox, items } = event.data;
  try {
    if (kind === "setRoot") {
      rootHandle = event.data.rootHandle;
      dirCache.clear();
      stripComponents = await detectOffset(event.data.samplePaths);
      self.postMessage({ id, ok: true, offset: stripComponents });
      return;
    }
    if (kind === "frame") {
      const src = await decodeSource(await sourceFile(event.data));
      self.postMessage({ id, ok: true, bitmap: src, width: src.width, height: src.height }, [src]);
      return;
    }
    if (kind === "cropBatch") {
      const src = await decodeSource(await sourceFile(event.data)); // one source open -> all its crops
      const results = [];
      for (const it of items) results.push({ key: it.key, blob: await cropBlob(src, it.bbox) });
      src.close();
      self.postMessage({ id, ok: true, results });
      return;
    }
    // single crop
    const src = await decodeSource(await sourceFile(event.data));
    const blob = await cropBlob(src, bbox);
    src.close();
    self.postMessage({ id, ok: true, blob });
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err && err.message || err) }); // e.g. video, unreadable, or not found
  }
};
