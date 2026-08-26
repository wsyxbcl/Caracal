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

## Video crops (P3) — implemented, awaiting real-data testing
**Vendored `mp4box.js` (BSD-3, `web-rde/vendor/`, 3 ESM files) demuxes; the
browser's `VideoDecoder` decodes.** WebCodecs is JS-only and consumes *encoded
chunks*, so a Rust/WASM demuxer would only split the pipeline and copy every
sample across the boundary; the demuxer choice is perf-irrelevant either way
(moov parse is milliseconds).

- **`rde-core`:** `Instance.frame_number: Option<u32>` — the 0-based
  **presentation-order** index (ffmpeg's `n`, what `run_md_video.py` selects on) —
  plus `MdDocument::video_frame_rates()`. 4 native tests over
  `tests/fixtures/video_demo.json`.
- **`web-rde/video-frames.js`:** `extractFrames(file, targets, onFrame)`; the
  Media Worker's `cropBatch` and `frame` (preview) kinds both route through it.
- **Decode strategy — adaptive RAP-range merge, not a linear pass.** Map each
  target to its preceding random-access point (from the real `stss`, never an
  assumed GOP), merge ranges, coalesce those <15 samples apart (cheaper than a
  decoder discontinuity plus another HDD seek), and fall back to a full linear
  pass only above 70% coverage. Measured on the demo set: **6–12% of a linear
  pass** at realistic 1–3 s keyframe intervals. Frames decoded merely to *reach* a
  target are closed without cropping — `VideoFrame.close()` is mandatory or the
  decoder pool stalls. Outputs are matched **by timestamp**, never by output order
  (decode order ≠ presentation order once B-frames exist).
- **Verified** with synthetic clips encoded so **luma == frame index**, which
  proves the exact requested frame comes back (B-frame clip included). Regenerate:
  `ffmpeg -f lavfi -i "color=c=black:s=128x96:r=30:d=8" -vf "geq=lum='N':cb=128:cr=128,scale=in_range=full:out_range=full" -color_range pc -c:v libx264 -crf 16 -bf 3 -g 40 -pix_fmt yuv420p out.mp4`
  End-to-end through the UI: 22 clips × 7 crops = 154/154 tiles at ~30 ms/clip;
  a mixed set gave 55/56 with the lone `.avi` correctly left as a placeholder.
- **Not demuxable in-browser:** `.avi`/`.mkv` (and codecs the browser lacks) stay a
  labelled placeholder — that's what the out-of-band ffmpeg pre-extraction escape
  hatch is for. `.mp4/.m4v/.mov` are covered.

## Remaining / roadmap
1. **Test P3 on real data.** The `maze_trans` clips aren't mounted here, so decode
   throughput on real 1080p H.264 is still an **estimate** (~3–10x an image crop).
   Watch the per-clip `[rde] clip …` console line for frame counts and whether any
   clip hit the `linear fallback`.
2. **Manual-test the FSA picker** on a real machine + folder (headless can't drive
   the native dialog): instant load, crops resolve per group, case-mismatch
   handled, genuinely-missing → "no image".
3. **P2 (SPEC §9):** live parameter sliders (re-run `find` on threshold change —
   it's cheap + pure), review-session save/load, review stats chart. NB the default
   `occurrence_threshold` of 20 yields **0 groups** on the 574-media demo json, so
   sliders are what make that dataset reviewable at all.
4. Optional: exclude/skip video in clustering, or video-aware clustering. Note
   clustering counts **distinct media**, so N frames inside one clip are a single
   occurrence — yet each still becomes its own tile (on the demo doc, 76% of
   suspicious *instances* were video vs 34% of suspicious *media*).

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
  `*-worker.js` change (`?v=BUILD` cache-bust). **The version must reach the
  workers' imports too** — a versioned worker paired with a stale cached
  `pkg/web_rde.js` is how you get `session.<new method> is not a function` after a
  wasm rebuild. Both workers therefore read their own `?v=` off
  `self.location` and `await import("./dep.js?v=" + BUILD)` rather than importing
  statically. **The `.wasm` binary needs it too:** wasm-bindgen's glue resolves
  `new URL("web_rde_bg.wasm", import.meta.url)`, which *drops the query*, so a
  fresh glue would still load a stale cached binary — the symptom is
  `wasm.<snake_case_name> is not a function` (glue-to-wasm call) as opposed to
  `session.<method> is not a function` (page-to-glue). Hence `rde-worker.js`
  passes an explicit `{ module_or_path: …/web_rde_bg.wasm?v=BUILD }` to `init`.
  Rebuilding the wasm alone is not enough; bump `BUILD` as well.
- **Data scale** (real testset `combined_md.json`): 73,578 media (54,198 image /
  19,380 video), 90,117 detections → 227 suspicious groups, 18,100 instances,
  9,751 distinct suspicious media (6,401 image + 3,350 video). Median 52
  crops/group, one of 1,630. Decode is disk-bound (~180 ms/image on an HDD);
  memory is not (crops are ~92 px). Json paths are Windows (`J:\…`); matching and
  resolution split on both `/` and `\`.
