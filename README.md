<p align="center">
  <img src="docs/logo.png" alt="Caracal" width="120">
</p>

<h1 align="center">Caracal</h1>

<p align="center">
  Camera-trap media <strong>timestamp</strong> visualizer — activity over time,
  time-of-day patterns, and a downloadable <code>trap_info</code> template.
  Parsing, aggregation, and rendering all run in your browser.
</p>

## Overview

Caracal is a companion to the [Serval](https://github.com/wsyxbcl/Serval)
workflow for reviewing camera-trap media timestamps and deployments. It reads a
CSV, then shows an overview heatmap across deployments, a per-deployment detail
scatter, an hour-of-day heatmap, and a deployments inventory, and it can export
a `trap_info` template.

It is built in Rust with [`polars`](https://pola.rs) and
[`charton`](https://crates.io/crates/charton), compiled to WebAssembly via
`wasm-bindgen`. The same browser bundle ships two ways:

- **Online** — a static site at <https://caracal.hinature.cn>. Nothing to
  install.
- **Offline** — the `serval-charton` binary serves the identical bundle from a
  local server.

In both cases the CSV never leaves the browser: the server only ever ships the
static assets, and all computation happens client-side in WebAssembly. This
keeps deployment GPS coordinates on your machine.

## Screenshot

![Caracal](docs/screenshot.png)

## Run

> First-time setup: the binary embeds the browser bundle from `web/pkg/` at
> compile time, so build it once with `wasm-pack` (see [Build](#build)) before
> running `cargo run` on a fresh checkout.

Start the local WASM server:

```bash
cargo run
```

Or explicitly:

```bash
cargo run -- serve-wasm --bind 127.0.0.1:8787
```

## Build

Rebuild the browser bundle:

```bash
cd web
wasm-pack build --release --target web --out-dir pkg
```

Build the native binary:

```bash
cd ..
cargo build --release
```
