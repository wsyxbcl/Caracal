// RDE worker (SPEC §5.1): all Rust/WASM work — MD json parse, clustering, and
// the export mask — runs here, off the UI thread. Communication is an id-keyed
// request/response over structured-clone messages; only bytes and small JSON
// DTOs cross the boundary (never shared pointers).

import init, { RdeSession, default_options } from "./pkg/web_rde.js";

const ready = init();
let session = null;

self.onmessage = async (event) => {
  const { id, type } = event.data;
  try {
    await ready;
    switch (type) {
      case "load": {
        // `bytes` arrives as a transferred ArrayBuffer (zero-copy, SPEC §2.1).
        session = new RdeSession(new Uint8Array(event.data.bytes));
        reply(id, {
          imageCount: session.image_count(),
          totalDetections: session.total_detections(),
          defaultOptions: default_options(),
        });
        break;
      }
      case "find": {
        requireSession();
        // find() returns a JSON string; keep it a string across the boundary.
        reply(id, { groups: session.find(event.data.options) });
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
