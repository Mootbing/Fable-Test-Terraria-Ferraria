//! Pass 12: spawn point selection + flatten (DESIGN §1.2). The spawn column
//! is picked inside world-center ±20 by minimal surface-height variance over
//! a 10-tile window; that window is flattened into a platform and the spawn
//! sits one tile above it.

use ferraria_shared::tiles::{Liquid, Tile, TileId};
use ferraria_shared::world::World;

use super::caves::surface_scan;
use super::GenParams;

pub fn pick_spawn(world: &mut World, params: &GenParams) {
    let surface = surface_scan(world);
    let center = params.width / 2;
    let half = params.spawn_flat_width / 2;

    // Candidate platform centers: center ± spawn_search_radius.
    let lo = center.saturating_sub(params.spawn_search_radius).max(half);
    let hi = (center + params.spawn_search_radius).min(params.width - half - 1);
    let mut best = (center, f64::INFINITY);
    for c in lo..=hi {
        let window = &surface[(c - half) as usize..(c + half) as usize];
        let mean = window.iter().map(|&v| v as f64).sum::<f64>() / window.len() as f64;
        let var = window
            .iter()
            .map(|&v| (v as f64 - mean) * (v as f64 - mean))
            .sum::<f64>();
        if var < best.1 {
            best = (c, var);
        }
    }
    let c = best.0;

    // Platform height: mean surface of the window.
    let window = &surface[(c - half) as usize..(c + half) as usize];
    let target = (window.iter().map(|&v| v as u64).sum::<u64>() / window.len() as u64) as u32;
    let target = target.clamp(2, params.height - 2);

    // Remove chests overlapping the flatten footprint first (entries and
    // tiles together, so no orphaned contents or chest halves remain).
    let (x0, x1) = (c - half, c + half); // columns x0..x1 are rebuilt
    let doomed: Vec<(u32, u32)> = world
        .chests
        .keys()
        .copied()
        .filter(|&(cx, cy)| cx + 2 > x0 && cx < x1 && cy < target + 32)
        .collect();
    for (cx, cy) in doomed {
        for dy in 0..2 {
            for dx in 0..2 {
                let mut t = world.tile(cx + dx, cy + dy);
                if t.id == TileId::Chest {
                    t.id = TileId::Air;
                    t.state = 0;
                    world.set_tile(cx + dx, cy + dy, t);
                }
            }
        }
        world.chests.remove(&(cx, cy));
    }

    // Flatten: clear the columns above the platform (removing trees, pots,
    // ...), fill below it down to solid ground, grass the top.
    for x in (c - half)..(c + half) {
        for y in 0..target {
            world.set_tile(x, y, Tile::AIR);
        }
        let mut y = target;
        let mut filled = 0;
        while y < params.height && !world.tile(x, y).is_solid() && filled < 30 {
            let mut t = world.tile(x, y);
            t.id = TileId::Dirt;
            // Drop any settled liquid: a solid tile holding water/lava is a
            // state the §3 automaton doesn't model (reachable at small world
            // heights where the fill zone overlaps the water band).
            t.liquid = Liquid::NONE;
            t.state = 0;
            world.set_tile(x, y, t);
            y += 1;
            filled += 1;
        }
        let mut top = world.tile(x, target);
        top.id = TileId::Grass;
        top.state = 0;
        world.set_tile(x, target, top);
    }

    // Feet stand one tile above the platform.
    world.spawn = (c, target - 1);
}
