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

## Known gaps

These are not hypothetical; they are what the code does today.

1. **The FSA offset is validated on one sample and then trusted for everything.**
   `detectOffset` resolves `samplePaths[0]` only. A document that mixes path
   shapes — images written one way and videos another, which `run_md_video.py`
   makes possible — can pass that check and then fail for everything else.
2. **The FSA branch reports success without measuring it.** It says
   "Linked <folder> — N media" where N is simply the candidate count, not a count
   of media it actually found. The enumeration branch does the honest thing and
   reports `matched/total` plus a sample of what missed. So a wrong offset looks
   like a working load with blank crops, which is the single most confusing
   failure this tool can produce.
3. **No feedback on what was inferred.** Nothing shows the user that
   `J:/AS_trapper/…/IMAG0001.JPG` is being looked for at `<picked>/a217_gps1/IMAG0001.JPG`.
   Upstream's version of this is a path you typed and can check; ours is a guess
   we keep to ourselves.
