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

## Measured on real production clips (2026-08-26)
36 clips off the capture drive onto a local HDD, plus a full-document run on the
user's Windows box. See the `rde-video-benchmark-2026-08-26` memory for the
numbers; the load-bearing conclusions:
- Clips are **2592x1944 (5 MP)**, ~20 s, ~50 MB, GOP **160** — not 1080p. GOP
  varies per camera (8 / 39 / 150 / 160), which is what decides the merge ratio.
- **Two regimes.** Only 21% of video detections target frame 0; those clips are
  I/O-bound (a whole ~50 MB read per crop). The other **77% are decode-bound** —
  up to 2.9 s/clip in software. Byte-range reads therefore help far less than the
  "52 MB per crop" headline suggests, and are now the *smallest* lever.
- **Concurrency is flat** from 2 to 6 (the disk saturates at ~242 MB/s); 6 only
  inflates per-clip latency 6.6x and holds six ~50 MB reads in flight. Default 3.
- **Thumbnail encode is 1-5 ms/clip** — format choice is a cache-size question,
  not a latency one.
- The user's machine runs **entirely software-rendered** (`chrome://gpu`: Canvas,
  Compositing, Rasterization, Video Decode all "Software only", adapter =
  Microsoft Basic Render Driver via SwANGLE). Getting a real GPU driver active is
  probably a bigger lever than any code change.

## Tuning (P2, partly done)
Built around **parameter impact**, not controls. Two kinds of parameter, because
they cost different things:
- **Occurrence — instant.** `occurrence_threshold` is only ever a *final filter*
  on `media_count` (pinned by `threshold_is_only_a_final_filter`), so the app
  clusters ONCE at 2 and the threshold is a client-side view: a ladder of
  groups/candidates per threshold, scrubbable with no recompute.
- **IoU — exact, on demand.** IoU decides what *merges*, so nothing about it can
  be derived; estimating it by re-filtering was measured up to **44% wrong**.
  Five real re-clusters (~1 s total). Production shape: groups peak at 0.90
  (180/227/203/209/194) while candidates climb 10,416 → 26,923 — loosening IoU
  gives *bigger* groups, not more of them.
- Confidence / box area / category stay ordinary Apply controls. No histograms.
  A representative-sample UI was built and reverted (`4da967d`, `4bf4d5c`).

**Review state is derived** (see the `rde-review-state-model` memory): decisions
belong to stable DetRefs, groups are ephemeral views marked ● / ◐ / ○ from their
current members, and a re-cluster inherits every decision. Explicit judgements stay
authoritative at export even if the detection later stops being a candidate.

## Remaining / roadmap
1. **Review-session save/load (P2, SPEC §9) — the blocking gap.** Decisions live
   only in memory, so a refresh or crash loses hours of work on a document with
   18,101 candidates. `state.decisions` is already exactly the right shape to
   serialize (DetRef → `{decision, at, round}`), plus `state.options`/threshold and
   the document identity to rebind against.
2. **Decide the mid-review export semantic.** Export currently removes every
   candidate left at the default Remove, *including ones never looked at* (the bar
   says so). That matches upstream RDE, but an unfinished review silently removes
   unreviewed candidates. The alternative — export only explicit decisions — makes
   an unfinished review a no-op. Unresolved.
3. **IoU "what merged" diff** — the one piece of visual tuning feedback still
   wanted: match old groups to new by camera + box overlap and show what changed
   between two IoU values.
4. **OPFS / precompute** (with the `has/get/put` cache tier that waits with it).
   `sourcesFor(instances)` is already the seam. ~0.4 h at concurrency 3 for all
   3,350 clips; ~0.11 GB. OPFS is origin-scoped → the offline launcher needs a
   **stable localhost port** (same constraint as persisting an FSA handle).
5. ~~**Write down the path contract, then manual-test the FSA picker.**~~ DONE
   2026-09-05 — see `web-rde/PATHS.md`. Contract adopted from upstream's
   `--imageBase` rule; FSA verified by hand against absolute Windows paths
   (strip 2, 8/8 probes, 94 crops). Firefox's enumeration path still has no
   human test.
   *Original note:* Which shapes
   of MegaDetector `file` path, against which picked folder, do we claim to
   support? Today that answer exists only as code: `alignDirectoryHandle` probes a
   few strip offsets against sample paths, and the `webkitdirectory` fallback
   matches on basename. Both are inferences, neither is specified, and neither is
   explained to the user. Real documents carry absolute Windows paths, paths
   relative to a drive root, or a nested tree that the picked folder sits inside
   — and getting it wrong shows up as "no image" on every tile rather than as an
   error anyone can act on. Specify the supported shapes first, then test the
   picker against each. FSA itself is Chromium/Edge only; Firefox has no File
   System Access and always takes the enumeration path, so the FSA branch has
   never been exercised at all.
6. Move `n_dir_levels_from_leaf` into dataset setup (it is camera grouping, not RDE
   tuning; currently a read-only note showing real camera names).
7. Optional: video-aware clustering. Clustering counts **distinct media**, so N
   frames inside one clip are a single occurrence — yet each is its own tile.

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
