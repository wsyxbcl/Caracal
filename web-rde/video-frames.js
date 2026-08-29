// Video frame extraction (SPEC §7): the pixel provider's `video + frame_number
// -> pixels` half. mp4box.js demuxes the container (WebCodecs only decodes
// *encoded chunks* — it can't open a file), the browser's VideoDecoder decodes.
//
// `frame_number` is ffmpeg's `n`: the 0-based **presentation-order** frame index
// from the start of the clip (that is what run_md_video.py's `select` counts).
// Decode order is not presentation order once B-frames exist, so we sort samples
// by composition time to find the target, and match decoded frames back **by
// timestamp** rather than by output order.
//
// Decode strategy: RDE wants a handful of known frames per clip, not the whole
// clip, so decoding linearly is usually waste. We map each target back to its
// preceding random-access point, merge the resulting ranges, and only fall back
// to a full linear pass when those ranges already cover most of the clip.
// Measured on the demo set, merging decodes 6-12% of a linear pass at realistic
// (1-3 s) keyframe intervals.

import { createFile, DataStream, Endianness, MP4BoxBuffer } from "./vendor/mp4box.all.mjs";

/// Containers mp4box.js can demux. `.avi`/`.mkv` need a different demuxer and
/// fall back to the out-of-band ffmpeg pre-extraction escape hatch (SPEC §7).
export const DEMUXABLE = /\.(mp4|m4v|mov)$/i;

// Two ranges closer than this are decoded as one: pushing a few extra frames
// through the decoder costs less than a decoder discontinuity plus another seek
// on a spinning disk.
const GAP_MERGE = 15;
// Above this share of the clip, merging has stopped paying for itself — just run
// the whole track in one pass and skip the bookkeeping.
const LINEAR_COVERAGE = 0.7;
// Keep the decoder fed without queueing the whole range at once.
const QUEUE_HIGH_WATER = 24;

/// Demux `file` and return its video track plus the full sample table. Samples
/// carry absolute file offsets, so chunks are sliced straight out of the buffer
/// we already hold — mp4box never has to copy the payload back to us.
async function demux(file) {
  const tRead = performance.now();
  // NB (benchmark): this reads the WHOLE clip. The range-merge below cuts
  // *decode* to a fraction, but I/O is still 100% — that asymmetry is exactly
  // what `bytesRead` vs `bytesTotal` is here to expose.
  const buffer = await file.arrayBuffer();
  const tDemux = performance.now();
  const mp4 = createFile();
  const info = await new Promise((resolve, reject) => {
    mp4.onReady = resolve;
    mp4.onError = (error) => reject(new Error(`demux failed: ${error}`));
    mp4.appendBuffer(MP4BoxBuffer.fromArrayBuffer(buffer, 0));
    mp4.flush();
    // onReady fires synchronously during append; if it didn't, there is no moov.
    reject(new Error("no moov box (not a readable MP4)"));
  });
  const track = info.videoTracks?.[0];
  if (!track) throw new Error("no video track");
  const samples = mp4.getTrackById(track.id).samples;
  if (!samples?.length) throw new Error("no samples in video track");
  return {
    buffer, mp4, track, samples,
    readMs: tDemux - tRead,
    demuxMs: performance.now() - tDemux,
    bytesRead: buffer.byteLength,
  };
}

/// The codec's out-of-band configuration (avcC/hvcC/…), which `VideoDecoder`
/// needs as `description` for length-prefixed (non-Annex-B) samples.
function codecDescription(mp4, trackId) {
  const trak = mp4.getTrackById(trackId);
  for (const entry of trak.mdia.minf.stbl.stsd.entries) {
    const box = entry.avcC || entry.hvcC || entry.vpcC || entry.av1C;
    if (!box) continue;
    const stream = new DataStream(undefined, 0, Endianness.BIG_ENDIAN);
    box.write(stream);
    return new Uint8Array(stream.buffer, 8); // strip the 8-byte box header
  }
  return undefined; // some codecs (e.g. AnnexB-in-MP4) need none
}

/// Does this sample contain a random-access NAL unit? Samples are stored as
/// length-prefixed NAL units (AVCC/HVCC), so this walks the prefixes and looks
/// for an IDR (H.264 type 5) or IRAP (HEVC types 16-23).
function sampleIsRandomAccess(view, sample, nalLengthSize, isHevc) {
  let p = sample.offset;
  const end = sample.offset + sample.size;
  while (p + nalLengthSize <= end) {
    let length = 0;
    for (let i = 0; i < nalLengthSize; i++) length = length * 256 + view.getUint8(p + i);
    p += nalLengthSize;
    if (length <= 0 || p + length > end) break;
    const header = view.getUint8(p);
    if (isHevc ? ((header >> 1) & 0x3f) >= 16 && ((header >> 1) & 0x3f) <= 23 : (header & 0x1f) === 5) {
      return true;
    }
    p += length;
  }
  return false;
}

/// Random-access flags per sample, repaired against the bitstream when needed.
///
/// ISO/IEC 14496-12: when a track has no `stss` box, *every* sample is by
/// definition a sync sample, and mp4box reports that faithfully. Plenty of
/// camera MP4s omit `stss` while still encoding P-frames — trusting the flag
/// there makes every "keyframe" a P-frame, and WebCodecs rejects the chunk
/// ("marked as type key but wasn't a key frame") or silently emits nothing.
/// So whenever every sample claims to be sync, verify against the NAL units.
/// A genuinely all-intra clip verifies as all-sync anyway, so this is safe.
function randomAccessFlags(buffer, samples, config) {
  const flags = samples.map((s) => !!s.is_sync);
  if (!flags.every(Boolean) || samples.length < 2) return { flags, repaired: false };

  const codec = (config.codec || "").toLowerCase();
  const isHevc = codec.startsWith("hev") || codec.startsWith("hvc");
  const isAvc = codec.startsWith("avc");
  if (!isAvc && !isHevc) return { flags, repaired: false }; // unknown codec: trust the container

  // Length-prefix size lives in the codec config: avcC byte 4, hvcC byte 21.
  const desc = config.description;
  const offset = isHevc ? 21 : 4;
  const nalLengthSize = desc && desc.length > offset ? (desc[offset] & 0x03) + 1 : 4;

  const view = new DataView(buffer);
  let found = 0;
  for (let i = 0; i < samples.length; i++) {
    flags[i] = sampleIsRandomAccess(view, samples[i], nalLengthSize, isHevc);
    if (flags[i]) found++;
  }
  if (!found) return { flags: samples.map((s) => !!s.is_sync), repaired: false }; // no IDRs found; keep the container's word
  return { flags, repaired: found !== samples.length };
}

/// Which sample ranges must be decoded to reach `targets` (presentation-order
/// frame indices). Exported for testing — this is pure index arithmetic.
/// `isSync` overrides the container's flags when they have been repaired.
export function planRanges(samples, targets, isSync) {
  // Presentation order: sort by composition time, ties by decode order.
  const presentation = samples
    .map((_, index) => index)
    .sort((a, b) => samples[a].cts - samples[b].cts || a - b);

  // Nearest random-access point at or before each sample, precomputed.
  const sync_ = isSync || samples.map((s) => !!s.is_sync);
  const lastSync = new Int32Array(samples.length);
  let sync = 0;
  for (let i = 0; i < samples.length; i++) {
    if (sync_[i]) sync = i;
    lastSync[i] = sync;
  }

  const wanted = new Map(); // decode index -> frame number
  const missing = [];
  for (const frame of targets) {
    const index = presentation[frame];
    if (index === undefined) missing.push(frame);
    else wanted.set(index, frame);
  }

  const merged = [];
  for (const index of [...wanted.keys()].sort((a, b) => a - b)) {
    const start = lastSync[index];
    const last = merged[merged.length - 1];
    if (last && start <= last[1] + GAP_MERGE) last[1] = Math.max(last[1], index);
    else merged.push([start, index]);
  }

  const cost = merged.reduce((sum, [start, end]) => sum + (end - start + 1), 0);
  if (cost > samples.length * LINEAR_COVERAGE) {
    return { ranges: [[0, samples.length - 1]], wanted, missing, linear: true, cost: samples.length };
  }
  return { ranges: merged, wanted, missing, linear: false, cost };
}

/// Decode the frames `targets` (presentation-order indices) out of `file`,
/// calling `onFrame(frameNumber, videoFrame)` for each. The frame is closed as
/// soon as `onFrame` resolves — copy anything you need out of it first. Frames
/// decoded only to *reach* a target are closed without ever reaching `onFrame`.
/// Returns decode statistics.
export async function extractFrames(file, targets, onFrame) {
  if (typeof VideoDecoder === "undefined") throw new Error("WebCodecs unavailable");

  const tStart = performance.now();
  const { buffer, mp4, track, samples, readMs, demuxMs, bytesRead } = await demux(file);
  const tConfig = performance.now();

  const config = {
    codec: track.codec,
    codedWidth: track.video.width,
    codedHeight: track.video.height,
    description: codecDescription(mp4, track.id),
    optimizeForLatency: true,
  };
  if (!(await VideoDecoder.isConfigSupported(config)).supported) {
    throw new Error(`codec not supported: ${track.codec}`);
  }

  const tPlan = performance.now();
  const { flags: isSync, repaired } = randomAccessFlags(buffer, samples, config);
  const timescale = track.timescale;
  const stamp = (sample) => Math.round((sample.cts * 1e6) / timescale);
  const chunkFor = (index) => new EncodedVideoChunk({
    type: isSync[index] ? "key" : "delta",
    timestamp: stamp(samples[index]),
    duration: Math.round((samples[index].duration * 1e6) / timescale),
    data: new Uint8Array(buffer, samples[index].offset, samples[index].size),
  });

  // Measured once per codec, on the head of this clip's own plan — real frames
  // from the file we are about to decode, so the answer is about this footage
  // rather than a synthetic sample.
  const firstPlan = planRanges(samples, targets, isSync);
  const mode = await preferredMode(config, () => {
    // From the first random-access point, running PAST the planned range: a
    // range can be a single sample, and one frame measures configure() rather
    // than decode. The samples are contiguous, so the extra ones decode cleanly.
    const start = firstPlan.ranges[0]?.[0];
    if (start === undefined) return [];
    const chunks = [];
    for (let i = start; i < Math.min(samples.length, start + PROBE_FRAMES); i++) chunks.push(chunkFor(i));
    return chunks;
  });

  const tDecodeStart = performance.now();
  const delivered = new Set();
  let decoded = 0, onFrameMs = 0, plan = firstPlan;

  // One decode attempt, over whatever targets are still outstanding. Separated
  // so a hardware failure can be retried in software without re-delivering the
  // frames that already made it through.
  async function attempt(acceleration, wanted) {
    plan = planRanges(samples, wanted, isSync);
    const byTimestamp = new Map();
    for (const [index, frame] of plan.wanted) byTimestamp.set(stamp(samples[index]), frame);
    const cropping = [];
    let failure = null;
    const decoder = new VideoDecoder({
      output: (frame) => {
        decoded++;
        const number = byTimestamp.get(frame.timestamp);
        if (number === undefined) {
          frame.close(); // decoded only to reach a target
          return;
        }
        byTimestamp.delete(frame.timestamp);
        // Crop before closing; VideoFrames hold scarce decoder-pool memory.
        cropping.push(
          (async () => {
            const t = performance.now();
            try {
              await onFrame(number, frame);
              delivered.add(number);
            } finally {
              frame.close();
              // Overlaps decode, so it is NOT additive with decodeMs — recorded
              // separately so the two can be told apart.
              onFrameMs += performance.now() - t;
            }
          })(),
        );
      },
      error: (error) => { failure = error; },
    });
    decoder.configure({ ...config, hardwareAcceleration: acceleration });

    for (const [start, end] of plan.ranges) {
      for (let index = start; index <= end && !failure; index++) {
        decoder.decode(chunkFor(index));
        if (decoder.decodeQueueSize >= QUEUE_HIGH_WATER) await drain(decoder);
      }
    }
    if (!failure) await decoder.flush().catch((err) => { failure = err; });
    if (decoder.state !== "closed") decoder.close();
    await Promise.all(cropping);
    return { failure, undelivered: [...byTimestamp.values()] };
  }

  let accel = mode;
  let result = await attempt(accel, targets);
  if (result.failure && accel !== "prefer-software") {
    // Retry only what never arrived, so partial progress is not re-cropped.
    const remaining = targets.filter((t) => !delivered.has(t));
    if (!remaining.length) {
      result = { failure: null, undelivered: [] };
    } else {
      accel = "prefer-software";
      const retry = await attempt(accel, remaining);
      if (!retry.failure) noteHardwareFailure(config);
      result = retry;
    }
  }
  const tDecodeEnd = performance.now();
  if (result.failure) throw result.failure;

  const frames = track.nb_samples ?? samples.length;
  return {
    // What this clip is
    codec: track.codec,
    width: track.video?.width,
    height: track.video?.height,
    durationS: track.duration && track.timescale ? +(track.duration / track.timescale).toFixed(1) : null,
    bytesTotal: file.size,
    bytesRead, // today == bytesTotal; the gap is the byte-range prize

    // What we asked of it
    frames,
    targets: targets.length,
    planned: plan.cost, // frames the plan says must be decoded
    decoded, // frames the decoder actually emitted
    coverage: +(plan.cost / frames).toFixed(3), // planned share of the clip
    ranges: plan.ranges.length,
    linear: plan.linear,
    syncRepaired: repaired, // container claimed every sample was a keyframe
    keyframes: isSync.reduce((n, k) => n + (k ? 1 : 0), 0),
    missing: [...plan.missing, ...result.undelivered],
    accel, // what the probe picked, or "prefer-software" after a demotion

    // Where the time went (readMs..decodeMs are sequential and additive;
    // onFrameMs runs *inside* decodeMs)
    readMs: +readMs.toFixed(1),
    demuxMs: +demuxMs.toFixed(1),
    configMs: +(tPlan - tConfig).toFixed(1),
    planMs: +(tDecodeStart - tPlan).toFixed(1),
    decodeMs: +(tDecodeEnd - tDecodeStart).toFixed(1),
    onFrameMs: +onFrameMs.toFixed(1),
    totalMs: +(performance.now() - tStart).toFixed(1),
  };
}

// ---- Which decoder to ask for ---------------------------------------------
// Nothing reports whether a decode actually used hardware, and hardware is not
// reliably faster: setup latency dominates short jobs, integrated decoders can
// be slower than an optimised software path, and some cameras write streams a
// hardware decoder rejects but a software one accepts. So we time both on this
// device's own files and keep the winner.
//
// Keyed by codec fingerprint rather than by machine: one document can hold
// several profiles, and the answer can differ between them.
const PROBE_FRAMES = 12;   // one GOP-ish; enough to out-measure configure() noise
const HW_TIE_MARGIN = 1.1; // hardware wins ties: it also frees CPU for other clips
const preferences = new Map(); // fingerprint -> { mode, hwMs, swMs, demoted }
const probing = new Map();     // fingerprint -> in-flight probe, so clips don't race

const fingerprint = (config) => `${config.codec} ${config.codedWidth}x${config.codedHeight}`;

/// Decode `chunks` under one acceleration mode; ms, or null if it won't run.
async function timeDecode(config, mode, chunks) {
  const candidate = { ...config, hardwareAcceleration: mode };
  let support;
  try {
    support = await VideoDecoder.isConfigSupported(candidate);
  } catch { return null; }
  if (!support.supported) return null;
  let failed = null;
  const decoder = new VideoDecoder({ output: (frame) => frame.close(), error: (err) => { failed = err; } });
  const t = performance.now();
  try {
    decoder.configure(candidate);
    for (const chunk of chunks) {
      if (failed) break;
      decoder.decode(chunk);
    }
    if (!failed) await decoder.flush();
  } catch (err) {
    failed = err;
  } finally {
    if (decoder.state !== "closed") decoder.close();
  }
  return failed ? null : performance.now() - t;
}

async function preferredMode(config, makeChunks) {
  const key = fingerprint(config);
  const known = preferences.get(key);
  if (known) return known.mode;
  if (probing.has(key)) return probing.get(key);
  const run = (async () => {
    const chunks = makeChunks();
    let mode = "no-preference", hwMs = null, swMs = null;
    if (chunks.length) {
      hwMs = await timeDecode(config, "prefer-hardware", chunks);
      swMs = await timeDecode(config, "prefer-software", chunks);
      if (hwMs !== null && swMs !== null) mode = hwMs <= swMs * HW_TIE_MARGIN ? "prefer-hardware" : "prefer-software";
      else if (hwMs !== null) mode = "prefer-hardware";
      else if (swMs !== null) mode = "prefer-software";
    }
    preferences.set(key, { mode, hwMs, swMs, demoted: false });
    const ms = (v) => (v === null ? "unavailable" : `${Math.round(v)} ms`);
    console.log(`[rde] decode probe ${key}: hardware ${ms(hwMs)} vs software ${ms(swMs)}` +
      ` over ${chunks.length} frames → ${mode}`);
    return mode;
  })();
  probing.set(key, run);
  try { return await run; } finally { probing.delete(key); }
}

/// A clip that failed in hardware and then succeeded in software — evidence
/// that the decoder, not the file, is the problem. Only that combination counts:
/// camera files do get damaged, and a clip that fails both ways says nothing
/// about the decoder. Demote the codec only once it keeps happening, so one bad
/// file cannot cost every later clip its hardware path.
const HW_FAILURE_LIMIT = 3;
function noteHardwareFailure(config) {
  const key = fingerprint(config);
  const known = preferences.get(key) || { mode: "no-preference" };
  const failures = (known.failures || 0) + 1;
  const demoted = failures >= HW_FAILURE_LIMIT;
  preferences.set(key, {
    ...known, failures,
    ...(demoted ? { mode: "prefer-software", demoted: true } : {}),
  });
  console.warn(`[rde] ${key}: hardware decode failed where software succeeded` +
    ` (${failures}/${HW_FAILURE_LIMIT})` + (demoted ? " — switching this codec to software" : ""));
}

/// What the probe decided, for the benchmark dump.
export function decodePreferences() {
  return [...preferences].map(([codec, v]) => ({ codec, ...v }));
}

/// Wait for the decoder to work through its backlog.
function drain(decoder) {
  return new Promise((resolve) => {
    const done = () => {
      if (decoder.decodeQueueSize < QUEUE_HIGH_WATER / 2) {
        decoder.removeEventListener("dequeue", done);
        resolve();
      }
    };
    decoder.addEventListener("dequeue", done);
  });
}
