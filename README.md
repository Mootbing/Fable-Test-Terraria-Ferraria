# Ferraria

An open-source, Terraria-inspired 2D sandbox game written entirely in Rust —
original code and assets. Online multiplayer: an authoritative server simulates
the world and streams it to browser clients over WebSockets; the client is the
same Rust codebase compiled to WebAssembly (macroquad).

## Architecture

```
crates/
  shared/   world model, tiles, items, entities, wire protocol (postcard)
  server/   axum + tokio: WebSocket endpoint, 60 tps authoritative sim, world gen
  client/   macroquad app compiled to wasm32-unknown-unknown, served by the server
web/        static shell: index.html + macroquad JS loader + built wasm
```

One deployable artifact: the server binary serves `web/` and `/ws` on `$PORT`.

## Development

The host needs only Docker; all Rust builds run in the `ferraria-dev` container:

```sh
docker build -t ferraria-dev:latest -f Dockerfile.dev .
scripts/dev.sh cargo test                 # shared + server
scripts/build-web.sh                      # client -> web/ferraria-client.wasm
scripts/dev.sh cargo run -p ferraria-server   # http://localhost:3000
```

## Deployment

Railway, via the root `Dockerfile` (multi-stage: builds wasm client + release
server, runtime image serves both). World saves persist to the `/data` volume.
