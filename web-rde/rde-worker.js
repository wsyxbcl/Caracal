// RDE worker (SPEC §5.1): all Rust/WASM work — MD json parse, clustering, and
// the export mask — runs here, off the UI thread. Communication is an id-keyed
// request/response over structured-clone messages; only bytes and small JSON
// DTOs cross the boundary (never shared pointers).

import init, { RdeSession, default_options, match_paths } from "./pkg/web_rde.js";

const ready = init();
let session = null;

self.onmessage = async (event) => {
  const { id, type } = event.data;
  try {
    await ready;
    switch (type) {
      case "load": {
        // `bytes` arrives as a transferred ArrayBuffer (zero-copy, SPEC §2.1).
        const t = performance.now();
        session = new RdeSession(new Uint8Array(event.data.bytes));
        console.log(`[rde/worker] parse: ${Math.round(performance.now() - t)} ms`);
        reply(id, {
          imageCount: session.image_count(),
          totalDetections: session.total_detections(),
          defaultOptions: default_options(),
          imageFiles: session.image_files(), // json paths, for path diagnostics + matching
        });
        break;
      }
      case "find": {
        requireSession();
        const t = performance.now();
        // find() returns a JSON string; keep it a string across the boundary.
        const groups = session.find(event.data.options);
        console.log(`[rde/worker] cluster: ${Math.round(performance.now() - t)} ms`);
        reply(id, { groups });
        break;
      }
      case "match": {
        requireSession();
        const t = performance.now();
        // Resolve json image paths to the picked files (SPEC §6.2). The json
        // paths stay inside the worker; only picked paths + results cross.
        const matches = match_paths(
          session.image_files(),
          JSON.stringify(event.data.pickedPaths),
        );
        console.log(`[rde/worker] match: ${Math.round(performance.now() - t)} ms`);
        reply(id, { matches });
        break;
      }
      case "export": {
        requireSession();
        const bytes = session.export(event.data.removeRefs);
        // Transfer the output buffer back (it's a fresh JS-owned copy).
        self.postMessage({ id, ok: true, result: { bytes } }, [bytes.buffer]);
        break;
      }
      default:
        throw new Error(`unknown message type: ${type}`);
    }
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err?.message || err) });
  }
};

function requireSession() {
  if (!session) throw new Error("no document loaded");
}

function reply(id, result) {
  self.postMessage({ id, ok: true, result });
}
