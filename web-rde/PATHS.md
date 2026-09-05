# Which media folder to pick, and why it sometimes fails

A MegaDetector results file identifies each medium by a `file` string. Nothing in
the format says whether that string is absolute, or relative to something, or
relative to *what* — so every tool that wants to show you the actual pixels has
to answer that question somehow.

## What upstream does

The MegaDetector RDE scripts make you say it:

```
find_repeat_detections_with_video_frames.py --imageBase <dir> ...
    --imageBase   Base directory for original images/videos   (required)
```

The rule is one line, and there is no guessing in it:

> `imageBase` joined with a detection's `file` is the medium on disk.

That covers both shapes in the wild. Paths written **relative** to the folder
that was processed take a real `imageBase`. Paths written **absolute** — which is
what `run_detector_batch` produces without `--output_relative_filenames`, and
what this project's pipeline produces (`J:/AS_trapper/as…/a217_…/IMAG0001.JPG`)
— take `--imageBase /`, so the join is a no-op.

## What we do, and where it differs

A browser cannot take a base *path*: the user picks a folder through a dialog,
and we only ever get a handle to it plus its name. So the contract becomes:

> **The folder you pick plays the role of upstream's `imageBase`.**

The difference is that upstream is *told* the base and we have to *infer* how
much of each `file` path the picked folder already accounts for. That inference
is ours, it is not part of any upstream spec, and it is where this goes wrong.

Two mechanisms, depending on the browser:

**File System Access (Chromium, Edge).** We hold the directory handle and resolve
each path lazily. The leading components to drop are inferred once, by
`detectOffset`: look for the picked folder's own name in a sample path and drop
everything up to it, else try offsets 0–7 in turn; the first that successfully
opens a sample file wins.

**Enumeration (`webkitdirectory`; all browsers, and the only path in Firefox).**
Every picked file's relative path is known up front, so no offset is needed.
Each `file` is matched to a picked file by the **longest component suffix that is
unique** among the picked files. Camera datasets reuse basenames heavily —
`IMG_0001.JPG` under fifty camera folders — so a tie is reported as *ambiguous*
rather than guessed.

## What you can pick

Given `J:/AS_trapper/as202601-202604_aligned/as202601-202604/a217_gps1/IMAG0001.JPG`:

| you pick | works | why |
|---|---|---|
| `a217_gps1` | ✅ | folder name found in the path |
| `as202601-202604` | ✅ | ditto, one level up |
| `as202601-202604_aligned` | ✅ | ditto |
| `AS_trapper` | ✅ | ditto |
| a renamed copy of any of those | ✅ | name is not found, offsets 0–7 are tried |
| a folder more than 8 levels above the media | ❌ | offset search stops at 7 |
| two deployments merged under one folder | ⚠️ | fine if the sub-paths still differ; ambiguous basenames are refused, not guessed |

## Verified

The FSA branch was exercised by hand for the first time on 2026-09-05, against a
real deployment with absolute Windows paths:

```
[rde] FSA root "DIQ_trapper": dropping 2 leading path component(s), so
"diq202601-202604_aligned/diq…/6001_diq…-Ere 0024.JPG" is looked for under it
— 8/8 probes resolved
[rde] buffered "6001_diq202601-202604": 50 sources, 1446 ms → all 94 tiles on screen
```

`J:/DIQ_trapper/…` with `DIQ_trapper` picked gives strip 2, which is right, and
the crops rendered. The offset is now scored over up to 8 samples rather than
accepted on the first one, and a partial match says so in the status line instead
of reporting a candidate count as though it were a count of media found — a wrong
offset used to look like a successful load with every crop blank.

## Still unverified

- **Firefox**, which has no File System Access and therefore always takes the
  enumeration path. That path is covered by automated tests, but not by a human.
- **A document that mixes path shapes** — stills written one way, videos another,
  which `run_md_video.py` permits. The multi-sample scoring is what should catch
  it, and nothing has produced such a document to try.
- **Picking a folder more than 8 levels above the media**, which the offset search
  gives up on. No error distinguishes it from a wrong folder.
