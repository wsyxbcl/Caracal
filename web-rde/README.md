# Caracal RDE

A browser-based reviewer for **repeat-detection elimination** on MegaDetector
results. Load a results `.json` and the folder of images it describes; the tool
groups detections that keep appearing at the same spot on the same camera, shows
you each group, and exports a `.json` with the ones you confirmed removed.

Everything runs locally. The `.json`, the images and their GPS never leave the
browser — there is no upload and no server-side anything.

## What this is, and what it is not

This is a *reviewer* for a method that belongs to MegaDetector. The method, the
parameters and the judgement behind them are documented upstream, and those pages
are worth reading before you trust the output of any tool, including this one:

- **The process, and why it is worth doing** —
  <https://lila.science/repeat-detection-elimination>
- **The function that finds repeat detections** —
  [`find_repeat_detections`](https://megadetector.readthedocs.io/en/latest/postprocessing.repeat_detection_elimination.html#find_repeat_detections---CLI-interface)
- **MegaDetector itself** — <https://github.com/agentmorris/MegaDetector>

What this adds is the review step: a fast keyboard loop over the groups, video
frames decoded in the browser so clips are reviewable alongside stills, and a
saved session so a review can be interrupted.

## The caution that matters most

A box that repeats in exactly the same place is **usually** a rock or a branch.
It is not always. Animals sleep in front of cameras. Animals and people enter the
frame at the same spot when the camera faces a narrow trail, so the same box can
recur many times and still be real.

So this is a semi-automated process, not an automatic one: the tool proposes,
and every group needs a human to look at it. Upstream's advice is to confirm
**one exemplar per group** rather than every crop, which is what makes the review
fast — reviewing a thousand exemplars is minutes of work.

## How the review works

One group at a time. Each group shows **six members**, chosen to differ from each
other — one per source file where possible, spread across the whole run, and
always including the highest-confidence detection, which is the one most likely
to actually be an animal.

For most groups that is enough to decide, and `Remove all` / `Keep all` applies to
every member, including the ones not shown — the buttons say how many. When a
group looks like it could be real, **Show all N members** loads the rest.

Two optional buttons decode crops ahead of time. *Precompute exemplars* makes
every group instantly openable; *Precompute every member* also does the crops you
would only see by expanding. Neither is required — a group decodes what it needs
as you reach it.

## Two places this differs from upstream on purpose

**`occurrence` counts distinct files, not detections.** MegaDetector compares its
occurrence threshold against every detection, so one clip with 30 detected frames
counts as 30. We count that clip once, because a rock seen in one clip is one
observation. For still-only data the two agree; for video ours is much stricter,
so **upstream's `20` and ours are not the same number**.

**The IoU default is 0.8, not 0.9.** Upstream notes why slack is needed — rocks
do not move, but cameras and branches do. Because review here is group-first,
looser matching mostly makes existing groups *bigger* rather than making more of
them, so it costs the reviewer little and catches noticeably more.

## Getting a results file

Produce `combined_md.json` with MegaDetector as usual; nothing here is specific
to this tool. Video needs one extra thing: each detection must carry the
`frame_number` it came from, and each clip its `frame_rate`, or video crops
cannot be located. Stills are unaffected.

## Picking the image folder

The folder you pick plays the role of upstream's `--imageBase`. Any level above
the media usually works — see [PATHS.md](PATHS.md) for exactly what is supported
and what to do when it does not resolve.

## When a crop will not appear

Tiles say why rather than going blank:

| label | meaning |
|---|---|
| `not found` | the path did not resolve under the folder you picked |
| `unreadable` | the file is damaged, or the decoder rejected it |
| `no decoder` / `no hevc` | this browser cannot decode that codec |
| `needs https` | video decoding needs a secure address; use `https://` |
| `no frame number` | the `.json` has no `frame_number` for that detection |
| `▶ avi`, `▶ mkv` | no in-browser demuxer for that container |

For the last three, the escape hatch is to pre-extract frames with `ffmpeg` and
point the tool at those instead.

## Browser support

Chromium and Edge get the native folder picker. Firefox has no File System
Access API, so it enumerates the folder instead — slower to start, otherwise the
same. Video decoding needs WebCodecs and a secure context (`https://` or
`localhost`).
