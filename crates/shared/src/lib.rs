//! Types shared between the Ferraria server and client: world model,
//! tiles, items, entities, and the WebSocket wire protocol.

pub mod protocol;

/// Protocol version; bumped on every breaking wire change. The server
/// rejects clients with a mismatching version at handshake.
pub const PROTOCOL_VERSION: u32 = 1;

/// World tiles are 16x16 px on screen; physics positions are in tile units.
pub const TILE_SIZE: f32 = 16.0;

/// Simulation rate, ticks per second.
pub const TICK_RATE: u32 = 60;
