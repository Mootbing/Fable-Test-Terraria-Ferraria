//! Types shared between the Ferraria server and client: tile/item/world data
//! tables and model, deterministic RNG, player physics, crafting, and the
//! WebSocket wire protocol.
//!
//! No tokio, no macroquad, no I/O — everything here compiles for both native
//! and wasm32-unknown-unknown and is pure/deterministic (ARCHITECTURE.md).
//! Gameplay numbers live in the data tables and constants, sourced from
//! DESIGN.md; don't inline them elsewhere.

pub mod crafting;
pub mod items;
mod macros;
pub mod physics;
pub mod protocol;
pub mod rng;
pub mod tiles;
pub mod world;

// Most-used physics constants, re-exported at the root (§0).
pub use physics::{GRAVITY, TERMINAL_VELOCITY};

/// Protocol version; bumped on every breaking wire change. The server
/// rejects clients with a mismatching version at handshake.
pub const PROTOCOL_VERSION: u32 = 1;

/// World tiles are 16x16 px on screen; physics positions are in tile units.
pub const TILE_SIZE: f32 = 16.0;

/// Simulation rate, ticks per second.
pub const TICK_RATE: u32 = 60;

/// Seconds per tick. Velocities are tiles/second; multiply by `DT` per tick.
pub const DT: f32 = 1.0 / TICK_RATE as f32;

/// Players can mine/place/interact within this many tiles of their center.
pub const REACH: f32 = 6.0;

/// A crafting station enables its recipes within this many tiles (§4.4) —
/// deliberately shorter than [`REACH`].
pub const STATION_RANGE: f32 = 4.0;

/// Global cap on live hostile enemies (§0).
pub const MAX_LIVE_ENEMIES: u32 = 200;

/// Hit immunity: players 40 ticks after a hit; enemies 10 ticks per damage
/// source (§0).
pub const PLAYER_IFRAME_TICKS: u32 = 40;
pub const ENEMY_IFRAME_TICKS: u32 = 10;

/// Crits: 4% base chance, ×2 damage (§0).
pub const CRIT_CHANCE: f32 = 0.04;
pub const CRIT_MULT: f32 = 2.0;

/// Player HP (§8): base 100, +20 per Life Crystal, max 400.
pub const PLAYER_BASE_MAX_HP: u32 = 100;
pub const LIFE_CRYSTAL_HP: u32 = 20;
pub const PLAYER_MAX_MAX_HP: u32 = 400;

/// Passive regen (§8): 0.5 HP/s once 8 s have passed without taking damage,
/// doubled while standing still.
pub const REGEN_HP_PER_SEC: f32 = 0.5;
pub const REGEN_DELAY_SECS: f32 = 8.0;
pub const REGEN_STANDING_STILL_MULT: f32 = 2.0;

/// Breath (§8): 200 units; fully submerged it drains 1 unit every 7 ticks
/// (≈23.3 s total); out of water it refills 3 units/tick (≈1.1 s). At 0
/// breath the player takes 10 dmg/s, ignoring defense.
pub const PLAYER_MAX_BREATH: u32 = 200;
pub const BREATH_DRAIN_INTERVAL_TICKS: u32 = 7;
pub const BREATH_REFILL_PER_TICK: u32 = 3;
pub const DROWNING_DPS: u32 = 10;

/// Death & respawn (§8): drop 50% of carried coins where you died (the pile
/// persists 10 min); respawn after 10 s, or 20 s while any boss is alive.
pub const DEATH_COIN_DROP_FRAC: f32 = 0.5;
pub const DEATH_PILE_PERSIST_SECS: u32 = 600;
pub const RESPAWN_SECS: u32 = 10;
pub const RESPAWN_SECS_BOSS_ALIVE: u32 = 20;

/// Debuff magnitudes (§8): Burning ticks 2 dmg/s ignoring defense; Darkness
/// halves the client-computed light radius (§10). Durations are per-source
/// (`tiles::LAVA_BURN_SECS`, `items::POTION_SICKNESS_SECS`, ...); kinds are
/// `protocol::Debuff`.
pub const BURNING_DPS: u32 = 2;
pub const DARKNESS_LIGHT_RADIUS_MULT: f32 = 0.5;

/// Coin denominations (§0): 100 Copper = 1 Silver, etc. Values in code are
/// in copper coins (see `items::ItemData::value`).
pub const COPPER_PER_SILVER: u32 = 100;
pub const COPPER_PER_GOLD: u32 = 10_000;
pub const COPPER_PER_PLATINUM: u32 = 1_000_000;

/// The one damage formula (§0): `max(1, attack − floor(defense / 2))`.
/// Crit doubling and accessory/set multipliers apply to `attack` before
/// calling this.
pub fn damage_dealt(attack: u32, defense: u32) -> u32 {
    (attack as i64 - defense as i64 / 2).max(1) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_formula() {
        assert_eq!(damage_dealt(10, 0), 10);
        assert_eq!(damage_dealt(10, 6), 7);
        assert_eq!(damage_dealt(10, 7), 7); // floor(7/2) = 3
        assert_eq!(damage_dealt(1, 100), 1); // never below 1
        assert_eq!(damage_dealt(0, 0), 1);
    }

    #[test]
    fn tick_constants_agree() {
        assert_eq!((1.0 / DT).round() as u32, TICK_RATE);
    }
}
