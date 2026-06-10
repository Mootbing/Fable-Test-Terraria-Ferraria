//! Ferraria server library: procedural world generation and the server-side
//! simulation modules. The `ferraria-server` binary (axum + WebSocket loop)
//! and the `worldgen_preview` dev tool both build on this.

pub mod sim;
pub mod worldgen;
