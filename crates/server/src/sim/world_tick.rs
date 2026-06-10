//! Live world ticks: the §3 fluid cadence, §2 sand gravity, the random-tick
//! system (grass spread / die-when-covered, torch extinguishing), sapling →
//! tree growth, and level-1 puddle evaporation.
//!
//! High-volume changes from these systems go through `Sim::stage_tile` and
//! flush once per tick as a batched [`ServerMessage::TilesChanged`]
//! (`ServerMessage` is `ferraria_shared::protocol`'s); player-driven changes
//! keep the immediate single-cell `TileChanged` path.

use ferraria_shared::tiles::{
    Liquid, LiquidKind, TileId, GRASS_SPREAD_DENOM, LAVA_UPDATE_TICKS, PUDDLE_EVAPORATE_SECS,
    SAPLING_AIR_NEEDED, TREE_HEIGHT_MAX, TREE_HEIGHT_MIN, WATER_UPDATE_TICKS,
};
use ferraria_shared::TICK_RATE;

use crate::worldgen::plant_tree;

use super::game::Sim;

/// Slow housekeeping (sapling growth checks, puddle evaporation, damage-map
/// purging) runs once per real second.
const HOUSEKEEPING_TICKS: u64 = TICK_RATE as u64;

impl Sim {
    /// One tick of every live world system.
    pub(crate) fn world_tick(&mut self) {
        self.step_fluids();
        self.step_sand();
        self.random_ticks();
        if self.tick.is_multiple_of(HOUSEKEEPING_TICKS) {
            self.grow_saplings();
            self.evaporate_puddles();
            self.purge_stale_damage();
        }
    }

    /// §3 cadence: water cells update every 2 ticks, lava every 5. Changed
    /// cells are staged for the batched broadcast, then post-processed
    /// (torch extinguishing, puddle tracking).
    fn step_fluids(&mut self) {
        let water = self.tick.is_multiple_of(WATER_UPDATE_TICKS as u64);
        let lava = self.tick.is_multiple_of(LAVA_UPDATE_TICKS as u64);
        if !water && !lava {
            return;
        }
        let mut changed = Vec::new();
        self.fluids.step(&mut self.world, water, lava, &mut changed);
        changed.sort_unstable();
        changed.dedup();
        for &(x, y) in &changed {
            self.stage_tile(x, y);
            self.track_puddle(x, y);
            let t = self.world.tile(x, y);
            // §2 tile 16: torches extinguish in water (the item pops out).
            if t.id == TileId::Torch && t.liquid.kind() == Some(LiquidKind::Water) {
                self.break_tile(x, y);
            }
            // Sand above a cell the liquid just vacated stays put (it only
            // falls through Air *foreground*), but sand resting in/over
            // water that drained may now have an empty cell below it.
            if t.id == TileId::Sand {
                self.sand_active.insert((x, y));
            }
        }
    }

    /// §2 tile 4: unsupported sand descends one cell per tick (instant tile
    /// descent), sinking through liquids (displacing them upward) and
    /// settling on any non-air foreground.
    fn step_sand(&mut self) {
        if self.sand_active.is_empty() {
            return;
        }
        let mut cells: Vec<(u32, u32)> = self.sand_active.drain().collect();
        // Bottom-up so a whole column moves together within one tick.
        cells.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (x, y) in cells {
            if self.world.tile(x, y).id != TileId::Sand {
                continue;
            }
            let below_y = y + 1;
            if below_y >= self.world.height {
                continue; // world floor supports it
            }
            let below = self.world.tile(x, below_y);
            if below.id != TileId::Air {
                continue; // settled on a solid (or any foreground object)
            }
            // Move down one cell, displacing the destination's liquid into
            // the vacated cell so volume is conserved.
            let mut src = self.world.tile(x, y);
            let displaced = below.liquid;
            let mut dst = below;
            dst.id = TileId::Sand;
            dst.liquid = Liquid::NONE;
            dst.state &= ferraria_shared::tiles::state::WALL_PLACED;
            src.id = TileId::Air;
            src.liquid = displaced;
            src.state &= ferraria_shared::tiles::state::WALL_PLACED;
            self.world.set_tile(x, y, src);
            self.world.set_tile(x, below_y, dst);
            self.stage_tile(x, y);
            self.stage_tile(x, below_y);
            self.fluids.mark(x, y);
            self.fluids.mark(x, y.wrapping_sub(1));
            self.fluids.mark(x, below_y + 1);
            self.fluids.mark(x.wrapping_sub(1), below_y);
            self.fluids.mark(x + 1, below_y);
            // Keep falling next tick; the sand that may rest on the vacated
            // cell falls too.
            self.sand_active.insert((x, below_y));
            if y > 0 && self.world.tile(x, y - 1).id == TileId::Sand {
                self.sand_active.insert((x, y - 1));
            }
        }
    }

    /// The random-tick system: each tile has a ~1/[`GRASS_SPREAD_DENOM`]
    /// chance per tick of receiving a tile update (DESIGN §2 tile 3), i.e.
    /// `area / 600` random cells visited per tick.
    fn random_ticks(&mut self) {
        let area = self.world.width as u64 * self.world.height as u64;
        let budget = (area / GRASS_SPREAD_DENOM as u64).max(1);
        for _ in 0..budget {
            let x = self.loot_rng.gen_range_u32(0..self.world.width);
            let y = self.loot_rng.gen_range_u32(0..self.world.height);
            self.random_tick_cell(x, y);
        }
    }

    /// One tile update at `(x, y)` (split out so tests can drive cells
    /// deterministically).
    pub(crate) fn random_tick_cell(&mut self, x: u32, y: u32) {
        let t = self.world.tile(x, y);
        match t.id {
            // §2 tile 3: grass dies when covered, else spreads to adjacent
            // air-exposed dirt.
            TileId::Grass => {
                if !air_exposed(&self.world, x, y) {
                    let mut t = t;
                    t.id = TileId::Dirt;
                    self.world.set_tile(x, y, t);
                    self.stage_tile(x, y);
                    return;
                }
                let (xi, yi) = (x as i32, y as i32);
                let candidates: Vec<(u32, u32)> = [
                    (xi - 1, yi - 1),
                    (xi, yi - 1),
                    (xi + 1, yi - 1),
                    (xi - 1, yi),
                    (xi + 1, yi),
                    (xi - 1, yi + 1),
                    (xi, yi + 1),
                    (xi + 1, yi + 1),
                ]
                .into_iter()
                .filter(|&(nx, ny)| nx >= 0 && ny >= 0)
                .map(|(nx, ny)| (nx as u32, ny as u32))
                .filter(|&(nx, ny)| {
                    self.world.in_bounds(nx, ny)
                        && self.world.tile(nx, ny).id == TileId::Dirt
                        && air_exposed(&self.world, nx, ny)
                })
                .collect();
                if let Some(&(nx, ny)) = self.loot_rng.pick(&candidates) {
                    let mut n = self.world.tile(nx, ny);
                    n.id = TileId::Grass;
                    self.world.set_tile(nx, ny, n);
                    self.stage_tile(nx, ny);
                }
            }
            // Safety net for torches that ended up under water through any
            // path the fluid post-processing didn't see.
            TileId::Torch if t.liquid.kind() == Some(LiquidKind::Water) => {
                self.break_tile(x, y);
            }
            _ => {}
        }
    }

    /// §2 tile 31: due saplings become 7–16-segment trees when 7+ air tiles
    /// stand above them; blocked saplings retry every second.
    fn grow_saplings(&mut self) {
        let due: Vec<(u32, u32)> = self
            .saplings
            .iter()
            .filter(|&(_, &at)| self.tick >= at)
            .map(|(&pos, _)| pos)
            .collect();
        for (x, y) in due {
            if self.world.tile(x, y).id != TileId::Sapling {
                self.saplings.remove(&(x, y)); // broken/overwritten meanwhile
                continue;
            }
            let mut air_above = 0u32;
            while air_above < TREE_HEIGHT_MAX
                && y > air_above
                && self.world.is_empty(x, y - air_above - 1)
            {
                air_above += 1;
            }
            if air_above < SAPLING_AIR_NEEDED {
                continue; // §2: needs 7+ air tiles above; retry later
            }
            let height = self
                .loot_rng
                .gen_range_u32(TREE_HEIGHT_MIN..TREE_HEIGHT_MAX + 1)
                .min(air_above + 1);
            // The lowest trunk segment replaces the sapling cell itself.
            let mut cleared = self.world.tile(x, y);
            cleared.id = TileId::Air;
            self.world.set_tile(x, y, cleared);
            if plant_tree(&mut self.world, x, y + 1, height) {
                self.saplings.remove(&(x, y));
                for i in 1..=height {
                    self.stage_tile(x, y + 1 - i);
                }
            } else {
                // Couldn't grow after all; put the sapling back.
                let mut t = self.world.tile(x, y);
                t.id = TileId::Sapling;
                self.world.set_tile(x, y, t);
            }
        }
    }

    /// §3: level-1 puddles on flat ground evaporate after 60 s.
    fn evaporate_puddles(&mut self) {
        let ttl = (PUDDLE_EVAPORATE_SECS * TICK_RATE as f32) as u64;
        let due: Vec<((u32, u32), u64)> = self
            .puddles
            .iter()
            .map(|(&pos, &since)| (pos, since))
            .collect();
        for ((x, y), since) in due {
            if !is_puddle(&self.world, x, y) {
                self.puddles.remove(&(x, y));
                continue;
            }
            if self.tick.saturating_sub(since) >= ttl {
                self.puddles.remove(&(x, y));
                let mut t = self.world.tile(x, y);
                t.liquid = Liquid::NONE;
                self.world.set_tile(x, y, t);
                self.stage_tile(x, y);
                self.fluids.mark(x, y);
                self.fluids.mark(x.wrapping_sub(1), y);
                self.fluids.mark(x + 1, y);
                self.fluids.mark(x, y.wrapping_sub(1));
            }
        }
    }

    /// Starts/stops tracking `(x, y)` as an evaporation candidate.
    pub(crate) fn track_puddle(&mut self, x: u32, y: u32) {
        if !self.world.in_bounds(x, y) {
            return;
        }
        if is_puddle(&self.world, x, y) {
            let tick = self.tick;
            self.puddles.entry((x, y)).or_insert(tick);
        } else {
            self.puddles.remove(&(x, y));
        }
    }

    /// Seeds puddle tracking from the generated world (settled pools leave
    /// level-1 shores that are already due to dry up).
    pub(crate) fn scan_initial_puddles(&mut self) {
        for y in 0..self.world.height {
            for x in 0..self.world.width {
                if is_puddle(&self.world, x, y) {
                    self.puddles.insert((x, y), 0);
                }
            }
        }
    }

    /// Drops mining-damage entries whose §2 5 s decay long passed, so the
    /// map stays bounded.
    fn purge_stale_damage(&mut self) {
        let reset_ticks =
            (ferraria_shared::tiles::TILE_DAMAGE_RESET_SECS * TICK_RATE as f32) as u64;
        let tick = self.tick;
        self.tile_damage
            .retain(|_, d| tick.saturating_sub(d.last_hit_tick) <= reset_ticks);
    }
}

/// Has at least one non-solid cardinal neighbor (the §2 grass exposure test;
/// world borders don't count as exposure).
fn air_exposed(world: &ferraria_shared::world::World, x: u32, y: u32) -> bool {
    let (xi, yi) = (x as i32, y as i32);
    [(xi - 1, yi), (xi + 1, yi), (xi, yi - 1), (xi, yi + 1)]
        .into_iter()
        .any(|(nx, ny)| !world.is_solid(nx, ny))
}

/// A level-1 liquid cell resting directly on solid ground.
fn is_puddle(world: &ferraria_shared::world::World, x: u32, y: u32) -> bool {
    let t = world.tile(x, y);
    t.liquid.is_some() && t.liquid.level() == 1 && world.is_solid(x as i32, y as i32 + 1)
}
