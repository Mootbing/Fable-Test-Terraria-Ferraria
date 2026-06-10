//! Client-side mirror of the server world: server-streamed chunks and tile
//! deltas applied into a full-size `shared::World`, plus the set of chunks
//! actually received.
//!
//! Deliberately backed by a real [`World`] rather than a bare chunk map with
//! a hand-rolled accessor: own-player prediction runs
//! `shared::physics::step_player` against it, and reimplementing `World`'s
//! solidity/liquid/bounds semantics over a `HashMap` would risk
//! client/server physics drift. Unreceived cells read as air; prediction is
//! frozen until the chunk under the player has arrived (the server streams
//! the 5×3 neighborhood before the player can reach its edge). Memory cost
//! is 4 bytes/tile ≈ 20 MB for the default 4200×1200 world — fine in wasm.

use std::collections::HashSet;

use ferraria_shared::tiles::Tile;
use ferraria_shared::world::{decode_chunk, ChunkDecodeError, World, CHUNK_SIZE};

pub struct WorldView {
    world: World,
    /// Chunk coords received from the server at least once.
    loaded: HashSet<(u32, u32)>,
}

impl WorldView {
    /// An empty mirror sized from the `Welcome` handshake.
    pub fn new(width: u32, height: u32, spawn: (u32, u32)) -> WorldView {
        let mut world = World::new(width, height);
        world.spawn = spawn;
        WorldView {
            world,
            loaded: HashSet::new(),
        }
    }

    /// Decodes and applies one `ChunkData` payload.
    pub fn apply_chunk(&mut self, cx: u32, cy: u32, bytes: &[u8]) -> Result<(), ChunkDecodeError> {
        let tiles = decode_chunk(bytes)?;
        self.world.apply_chunk(cx, cy, &tiles);
        self.loaded.insert((cx, cy));
        Ok(())
    }

    /// Applies a `TileChanged` delta.
    pub fn apply_tile(&mut self, x: u32, y: u32, tile: Tile) {
        self.world.set_tile(x, y, tile);
    }

    /// The mirrored world, for rendering and shared physics.
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn loaded_chunks(&self) -> usize {
        self.loaded.len()
    }

    /// Whether the chunk containing the world-space point has been received
    /// — gates own-player prediction so we never fall through still-loading
    /// terrain.
    pub fn chunk_loaded_at(&self, x: f32, y: f32) -> bool {
        let cx = ((x.max(0.0) as u32) / CHUNK_SIZE).min(self.world.chunks_x() - 1);
        let cy = ((y.max(0.0) as u32) / CHUNK_SIZE).min(self.world.chunks_y() - 1);
        self.loaded.contains(&(cx, cy))
    }
}
