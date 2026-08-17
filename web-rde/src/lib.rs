//! WASM bindings for the Caracal RDE reviewer (SPEC §5). Thin layer over
//! `rde-core`: parse MD json bytes, cluster suspicious groups, and export a
//! removal mask on the original document. All heavy work is `rde-core`; this
//! crate only crosses the JS boundary as bytes + small JSON DTOs (SPEC §5.1).

use rde_core::{apply_removals, find_suspicious, DetRef, MdDocument, RdeOptions};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Default RDE options as JSON, for initializing the UI controls.
#[wasm_bindgen]
pub fn default_options() -> String {
    serde_json::to_string(&RdeOptions::default()).expect("options serialize")
}

/// A parsed MegaDetector document held in WASM as the immutable original
/// (SPEC §6.1). Clustering and export both run against it.
#[wasm_bindgen]
pub struct RdeSession {
    doc: MdDocument,
}

#[wasm_bindgen]
impl RdeSession {
    /// Parse MD json bytes — hand the `File`'s `ArrayBuffer` straight in to skip
    /// the JS-string copy (SPEC §2.1).
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<RdeSession, JsValue> {
        let doc = MdDocument::from_slice(bytes).map_err(to_js)?;
        Ok(Self { doc })
    }

    pub fn image_count(&self) -> usize {
        self.doc.images().len()
    }

    pub fn total_detections(&self) -> usize {
        self.doc.total_detections()
    }

    /// Cluster suspicious groups (SPEC §3). `options_json` is an `RdeOptions`;
    /// returns the groups as a JSON array (the review DTO, SPEC §5.1). Cheap and
    /// pure, so the UI can re-run it on every parameter change.
    pub fn find(&self, options_json: &str) -> Result<String, JsValue> {
        let options: RdeOptions = serde_json::from_str(options_json).map_err(to_js)?;
        let groups = find_suspicious(&self.doc, &options);
        serde_json::to_string(&groups).map_err(to_js)
    }

    /// Apply a removal mask on the original document (SPEC §6.3).
    /// `remove_refs_json` is a JSON array of `DetRef`; returns filtered MD json
    /// bytes for a Blob download.
    pub fn export(&self, remove_refs_json: &str) -> Result<Vec<u8>, JsValue> {
        let refs: Vec<DetRef> = serde_json::from_str(remove_refs_json).map_err(to_js)?;
        Ok(apply_removals(&self.doc, &refs).to_json_vec())
    }
}

fn to_js(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}
