//! Ferraria server library: procedural world generation, the 60 tps
//! simulation task, and the axum HTTP/WebSocket front end. The
//! `ferraria-server` binary, the netplay integration test, and the
//! `worldgen_preview` dev tool all build on this.

pub mod net;
pub mod sim;
pub mod worldgen;
