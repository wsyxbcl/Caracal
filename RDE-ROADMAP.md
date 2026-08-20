# Caracal RDE — State & Roadmap

Client-side **Repeat-Detection-Elimination reviewer**: MegaDetector results
`.json` in → suspicious repeated-box groups per camera → human per-crop
keep/remove review → filtered `.json` export. Everything runs in the browser,
offline; no data leaves the device (camera-trap GPS privacy).

- **Spec:** `/home/wsyxbcl/scripts/MegaDetector/rde-lab/SPEC-caracal-rde.md`
- **Demo/reference:** `.../rde-lab/demo/` (`mdv1000_demo.json`, `GROUND_TRUTH.md`,
  `verify_rde.py`).
- Lives on branch **`experiment/rde`** (not on `master`).

## Decision: RDE becomes its own repo (later)
It shares **~zero code** with Caracal — only the repo, deploy pipeline, and the
offline-wasm delivery model. It's a *detection-review* tool (serde_json + image
decode, upstream of analysis), distinct from Caracal's data-panel direction
(polars + charts). When splitting, lift these out — they're self-contained:
- `crates/rde-core` — deps: `serde`, `serde_json` only.
- `web-rde` — deps: `rde-core`, `wasm-bindgen`, `getrandom`, `console_error_panic_hook`, `serde_json`.
- Frontend: `web-rde/{index.html, rde-worker.js, media-worker.js}` + this doc.
Then give it its own domain/service + release cadence.

## Architecture (current)
- **`crates/rde-core`** (browser/IO-independent, native-testable): `RdeOptions`,
  `MdDocument` (serde_json Value + `preserve_order`), `find_suspicious`
  (per-camera, category-aware, greedy IoU vs a fixed rep box, threshold on
  *distinct images*), `DetRef`/`Instance`/`Decision`, `apply_removals` (mask on
  the original by index — unknown fields survive), `match_paths` (§6.2
  longest-unique-suffix). **12 native tests** reproduce the demo ground truth.
- **`web-rde`** (own wasm bundle, ~100 KB): `RdeSession`
  (new/image_count/total_detections/image_files/find/export), `match_paths`,
  `default_options`.
- **Workers:** `rde-worker.js` (all wasm compute — parse/find/match/export,
  id-keyed RPC), `media-worker.js` (decode + crop; batch-by-source; File System
  Access path resolution).
- **Frontend:** `web-rde/index.html` — the review UI.

## What's done
**Performance** (root cause + fixes — see the `rde-scroll-perf` memory for the
full diagnosis):
- Scroll lag was **software-rendering main-thread scrolling** forced by a tall
  page + `position:fixed/sticky`. Fix: **app-shell layout** — the page never
  scrolls; only the review list scrolls in its own `overflow:auto` container
  (compositor scroll), no fixed/sticky/tall-page.
- Crops render as **`<img>` (blob), not a per-tile `<canvas>`** — each canvas was
  its own compositor layer; ~170 froze software compositing.
- **Media Worker** decodes + crops off the main thread.
- Removed `content-visibility` (it churned frames on scroll).

**Fast review UX:**
- **Batched crop cache + group prefetch:** the Media Worker's `cropBatch` decodes
  one source image once and emits all its crops (halves disk reads — 18,100 tiles
  from 9,751 distinct media).
- **One group at a time, mouse + keyboard:** ← → ↑ ↓ move a focus ring, Space/x
  keep-or-drop the focused crop, Enter/n = next group (marks reviewed), p = prev,
  k/d = keep/remove all. Default is Remove → mark the few real animals → advance.
- **Prefetch-next:** the next group decodes in the background, so advancing is
  instant. A shared source-decode limiter caps HDD concurrency; `evictExcept`
  keeps only current + next cached (bounded memory). Buffer progress bar.

**File access:**
- **File System Access** (`showDirectoryPicker`): the picked directory *handle* is
  retained in the Media Worker; each media's known json path is resolved **lazily**
  — walk root→subdir→file, **cache directory handles**, enumerate a *single*
  parent directory only as a case-insensitive/ambiguity fallback. **No global file
  index.** The path offset is auto-detected, so any ancestor folder can be picked.
  Replaces the ~12 s `webkitdirectory` enumeration freeze.
- `webkitdirectory` picker kept as a fallback (non-Chromium).

## Remaining / roadmap
1. **Manual-test the FSA picker** on a real machine + folder (headless can't drive
   the native dialog): instant load, crops resolve per group, case-mismatch
   handled, genuinely-missing → "no image".
2. **Video crops (P3):** WebCodecs + a demuxer. The pipeline is already shaped for
   it — add a WebCodecs branch to the Media Worker's batch-by-source path: open a
   video once via the handle, seek to `frame_number / frame_rate` (the json has
   per-video `frame_rate`; each detection carries `frame_number`), decode the
   frame, crop. Batch all of a video's requested frames in one pass (presentation
   order from a keyframe). **Caveat:** `.MP4` works with WebCodecs; `.avi/.mov/.mkv`
   need extra demuxers or may not decode in-browser. (~3,350 of 9,751 suspicious
   media are video.)
3. **P2 (SPEC §9):** live parameter sliders (re-run `find` on threshold change —
   it's cheap + pure), review-session save/load, review stats chart.
4. Optional: exclude/skip video in clustering, or video-aware clustering.

## Perf lessons (don't re-learn)
- Headless CDP **cannot** reproduce headed software-render compositor lag — ask
  for a **DevTools Performance trace** early when perf is environment-specific.
  Analyze `traceEvents` offline: async frame stages (`SendBeginMainFrameToCommit`
  vs presentation), `UpdateLayer` distinct `layerId` count, per-thread `RunTask`
  busy time, `Commit` gaps. DevTools `Screenshot` events are recording overhead,
  not the bug.
- Under software rendering (GPU disabled): avoid tall scrolling pages +
  fixed/sticky (forces main-thread scroll = full-viewport repaint per frame);
  avoid many `<canvas>` (each becomes a compositor layer); `content-visibility`
  churns render state on scroll.

## Dev + key facts
- Serve: `python3 -m http.server --directory web-rde`; open on **`127.0.0.1`**
  (secure context, required for File System Access).
- Workers cache hard: bump the **`BUILD`** constant in `index.html` on any
  `*-worker.js` change (`?v=BUILD` cache-bust).
- **Data scale** (real testset `combined_md.json`): 73,578 media (54,198 image /
  19,380 video), 90,117 detections → 227 suspicious groups, 18,100 instances,
  9,751 distinct suspicious media (6,401 image + 3,350 video). Median 52
  crops/group, one of 1,630. Decode is disk-bound (~180 ms/image on an HDD);
  memory is not (crops are ~92 px). Json paths are Windows (`J:\…`); matching and
  resolution split on both `/` and `\`.
