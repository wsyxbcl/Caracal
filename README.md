# serval-charton

Media timeline explorer built with `polars`, `charton`, and `wasm-bindgen`.

## Overview
This project ships as a local WASM web app by default. Running the binary starts a local server and opens an interactive browser interface for exploring media timestamp distributions by deployment.  

It is part of the [Serval](https://github.com/wsyxbcl/Serval) workflow where users review timestamps and deployments.

## Screenshot

![Deployment Timestamp Visualizer](docs/screenshot.png)

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
