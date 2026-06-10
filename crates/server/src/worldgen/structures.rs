//! Pass 11: structures & placements (DESIGN §1.2 table) — chests with §2.3
//! loot, underworld ruins with Infernal Forges, ritual altars, life
//! crystals, pots, cobwebs.

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::{Liquid, LiquidKind, Tile, TileId, WallId};
use ferraria_shared::world::World;

use super::caves::surface_scan;
use super::loot::{roll_chest, SURFACE_CHEST, UNDERGROUND_CHEST, UNDERWORLD_CHEST};
use super::pockets::Pockets;
use super::GenParams;

/// How many random candidates to try per requested placement before giving
/// up on it (placement counts are best-effort; tests bound them from below).
const TRIES_PER_PLACEMENT: u32 = 600;

pub fn place_structures(world: &mut World, params: &GenParams, rng: &mut Pcg32, pockets: &Pockets) {
    let surface = surface_scan(world);
    underworld_ruins(world, params, rng);
    underground_chests(world, params, rng);
    surface_chests(world, params, rng, &surface);
    ritual_altars(world, params, rng);
    life_crystals(world, params, rng);
    pots(world, params, rng, &surface);
    cobwebs(world, params, rng, pockets);
}

/// True if `w×1` tiles starting at `(x, y)` are all fully solid.
fn solid_run(world: &World, x: u32, y: u32, w: u32) -> bool {
    (0..w).all(|dx| world.tile(x + dx, y).is_solid())
}

/// True if the `w×h` box at `(x, y)` is all air, optionally lava-free.
fn air_box(world: &World, x: u32, y: u32, w: u32, h: u32, allow_lava: bool) -> bool {
    for dy in 0..h {
        for dx in 0..w {
            let t = world.tile(x + dx, y + dy);
            if t.id != TileId::Air {
                return false;
            }
            if !allow_lava && t.liquid.kind() == Some(LiquidKind::Lava) {
                return false;
            }
        }
    }
    true
}

/// Picks a random spot where a `w×h` object can stand on a cave floor whose
/// top row is in `rows`: `w×h` air above `w` solid tiles. Returns the
/// object's top-left origin.
fn find_floor_spot(
    world: &World,
    rng: &mut Pcg32,
    rows: (u32, u32),
    w: u32,
    h: u32,
) -> Option<(u32, u32)> {
    for _ in 0..TRIES_PER_PLACEMENT {
        let x = rng.gen_range_u32(1..world.width.saturating_sub(w + 1));
        let floor = rng.gen_range_u32(rows.0..rows.1);
        if floor < h + 1 {
            continue;
        }
        if solid_run(world, x, floor, w) && air_box(world, x, floor - h, w, h, false) {
            return Some((x, floor - h));
        }
    }
    None
}

/// 150 underground chests on cave floors (floor row in 350–999), ≥25 tiles
/// apart, rolled with the §2.3 underground table.
fn underground_chests(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    let mut placed: Vec<(u32, u32)> = Vec::new();
    let spacing_sq = (params.chest_spacing * params.chest_spacing) as u64;
    let (cw, ch) = (2, 2);
    'outer: for _ in 0..params.underground_chests {
        // Generous retry budget: a single find_floor_spot scan misses a few
        // percent of the time and the spacing rule rejects more — the §1.2
        // table asks for the full 150, so rescan instead of abandoning.
        for _ in 0..TRIES_PER_PLACEMENT / 5 {
            let Some((x, y)) = find_floor_spot(world, rng, params.underground_chest_rows, cw, ch)
            else {
                continue;
            };
            let far_enough = placed.iter().all(|&(px, py)| {
                let dx = px.abs_diff(x) as u64;
                let dy = py.abs_diff(y) as u64;
                dx * dx + dy * dy >= spacing_sq
            });
            if far_enough && world.place_multitile(x, y, TileId::Chest) {
                let loot = roll_chest(rng, &UNDERGROUND_CHEST);
                world.chests.insert((x, y), loot);
                placed.push((x, y));
                continue 'outer;
            }
        }
    }
}

/// 20 surface chests, on the surface or in surface caves (floor above the
/// underground-chest band), §2.3 surface table.
fn surface_chests(world: &mut World, params: &GenParams, rng: &mut Pcg32, surface: &[u32]) {
    'outer: for _ in 0..params.surface_chests {
        for _ in 0..TRIES_PER_PLACEMENT {
            let x = rng.gen_range_u32(2..params.width - 4);
            // Scan from this column's surface down to the underground line
            // for the first 2-wide floor with room (catches surface caves).
            let top = surface[x as usize];
            for floor in top..params.underground_chest_rows.0 {
                if solid_run(world, x, floor, 2)
                    && floor >= 2
                    && air_box(world, x, floor - 2, 2, 2, false)
                {
                    if world.place_multitile(x, floor - 2, TileId::Chest) {
                        let loot = roll_chest(rng, &SURFACE_CHEST);
                        world.chests.insert((x, floor - 2), loot);
                        continue 'outer;
                    }
                    break;
                }
            }
        }
    }
}

/// 10 ember-brick ruins (10×8 shells) on the underworld floor, each holding
/// an Infernal Forge and an underworld chest. One ruin per width-slice so
/// they spread across the map.
fn underworld_ruins(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    let (rw, rh) = params.ruin_size;
    if params.width < rw + 4 {
        return;
    }
    let slice = params.width / params.ruins.max(1);
    for i in 0..params.ruins {
        let x_lo = (i * slice + 2).min(params.width - rw - 2);
        let x_hi = ((i + 1) * slice).saturating_sub(rw + 2).max(x_lo + 1);
        let mut best: Option<(u32, u32)> = None;
        for _ in 0..TRIES_PER_PLACEMENT / 4 {
            let x = rng.gen_range_u32(x_lo..x_hi);
            // Find the underworld cavern floor under the shell's center.
            let cx = x + rw / 2;
            let Some(floor) = underworld_floor(world, params, cx) else {
                continue;
            };
            if floor + 2 >= params.height || floor < params.stone_to_ash + rh {
                continue;
            }
            best = Some((x, floor));
            // Prefer a dry floor (above the lava line); take it immediately.
            if floor <= params.hell_lava_row {
                break;
            }
        }
        if let Some((x0, base)) = best {
            build_ruin(world, params, rng, x0, base);
        }
    }
}

/// First solid row below the underworld's open band at column `x`.
fn underworld_floor(world: &World, params: &GenParams, x: u32) -> Option<u32> {
    let mut y = params.stone_to_ash;
    // Skip the ceiling material, find the open band.
    while y < params.height && world.tile(x, y).id != TileId::Air {
        y += 1;
    }
    // Descend through the open band to the floor.
    while y < params.height && !world.tile(x, y).is_solid() {
        y += 1;
    }
    (y < params.height).then_some(y)
}

/// Builds one 10×8 shell whose floor row replaces `base` (the cavern
/// floor): ember-brick frame, stone-walled interior, forge left, chest
/// right.
fn build_ruin(world: &mut World, params: &GenParams, rng: &mut Pcg32, x0: u32, base: u32) {
    let (rw, rh) = params.ruin_size;
    let top = base + 1 - rh; // shell occupies rows top..=base
    for dy in 0..rh {
        for dx in 0..rw {
            let (x, y) = (x0 + dx, top + dy);
            let edge = dx == 0 || dx == rw - 1 || dy == 0 || dy == rh - 1;
            let tile = Tile {
                id: if edge {
                    TileId::EmberBrick
                } else {
                    TileId::Air
                },
                wall: if edge { WallId::Air } else { WallId::Stone },
                liquid: Liquid::NONE,
                state: 0,
            };
            world.set_tile(x, y, tile);
        }
    }
    // Support pillars down to solid ground (the floor line is noisy).
    for dx in 0..rw {
        let x = x0 + dx;
        let mut y = base + 1;
        let mut filled = 0;
        while y < params.height && !world.tile(x, y).is_solid() && filled < 15 {
            let mut t = world.tile(x, y);
            t.id = TileId::EmberBrick;
            t.liquid = Liquid::NONE;
            t.state = 0;
            world.set_tile(x, y, t);
            y += 1;
            filled += 1;
        }
    }
    // Furnishings stand on the shell's floor row (`base`).
    let forge_origin = (x0 + 1, base - 2);
    world.place_multitile(forge_origin.0, forge_origin.1, TileId::InfernalForge);
    let chest_origin = (x0 + rw - 3, base - 2);
    if world.place_multitile(chest_origin.0, chest_origin.1, TileId::Chest) {
        let loot = roll_chest(rng, &UNDERWORLD_CHEST);
        world.chests.insert(chest_origin, loot);
    }
}

/// 30 ritual altars (3×2) on cavern floors, rows 500–999.
fn ritual_altars(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    'outer: for _ in 0..params.altars {
        for _ in 0..TRIES_PER_PLACEMENT / 50 {
            if let Some((x, y)) = find_floor_spot(world, rng, params.altar_rows, 3, 2) {
                if world.place_multitile(x, y, TileId::RitualAltar) {
                    continue 'outer;
                }
            }
        }
    }
}

/// 100 life crystals embedded in solid stone adjacent to caves, rows
/// 450–999 (1×1; replaces a stone tile that touches air).
fn life_crystals(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    'outer: for _ in 0..params.life_crystals {
        for _ in 0..TRIES_PER_PLACEMENT {
            let x = rng.gen_range_u32(1..params.width - 1);
            let y = rng.gen_range_u32(params.life_crystal_rows.0..params.life_crystal_rows.1);
            if world.tile(x, y).id != TileId::Stone {
                continue;
            }
            let touches_air = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                .iter()
                .any(|&(nx, ny)| world.tile(nx, ny).id == TileId::Air);
            if touches_air {
                let mut t = world.tile(x, y);
                t.id = TileId::LifeCrystal;
                t.state = 0;
                world.set_tile(x, y, t);
                continue 'outer;
            }
        }
    }
}

/// 600 pots on cave floors in every layer (below the surface, lava-free).
fn pots(world: &mut World, params: &GenParams, rng: &mut Pcg32, surface: &[u32]) {
    'outer: for _ in 0..params.pots {
        for _ in 0..TRIES_PER_PLACEMENT * 4 {
            let x = rng.gen_range_u32(1..params.width - 1);
            let lo = surface[x as usize] + 1;
            if lo + 2 >= params.height {
                continue;
            }
            let y = rng.gen_range_u32(lo..params.height - 1);
            let t = world.tile(x, y);
            if t.id == TileId::Air
                && t.liquid.kind() != Some(LiquidKind::Lava)
                && world.tile(x, y + 1).is_solid()
            {
                let mut t = t;
                t.id = TileId::Pot;
                t.state = 0;
                world.set_tile(x, y, t);
                continue 'outer;
            }
        }
    }
}

/// Cobwebs, rows 450–999. The §1.2 rule fills 10% of small cave pockets
/// (≤ `cobweb_max_pocket` cells, "small" canonized) — but the TileRunner
/// cave network is almost fully connected, so genuinely enclosed small
/// pockets are rare (a handful per world). To still land near the spec's
/// ~2000 cobwebs, the remainder is placed as small clusters in cave *nooks*
/// (air cells hugging several solid faces), which is where Terraria's webs
/// read as being anyway.
fn cobwebs(world: &mut World, params: &GenParams, rng: &mut Pcg32, pockets: &Pockets) {
    // Rule as written: whole small enclosed pockets.
    let mut fill = vec![false; pockets.regions.len()];
    for (i, region) in pockets.regions.iter().enumerate() {
        if !region.open_to_sky
            && region.cells <= params.cobweb_max_pocket
            && region.min_y >= params.cobweb_rows.0
            && region.max_y < params.cobweb_rows.1
        {
            fill[i] = rng.chance(params.cobweb_pocket_frac);
        }
    }
    let mut placed = 0u32;
    let w = world.width as usize;
    for y in params.cobweb_rows.0..params.cobweb_rows.1 {
        for x in 0..params.width {
            let idx = y as usize * w + x as usize;
            let label = pockets.label[idx];
            if label != 0 && fill[(label - 1) as usize] {
                let t = &mut world.tiles[idx];
                // Labels are from pass 7: skip cells later filled by liquid
                // or structures.
                if t.id == TileId::Air && t.liquid.is_none() {
                    t.id = TileId::Cobweb;
                    t.state = 0;
                    placed += 1;
                }
            }
        }
    }

    // Top-up: clusters seeded in nooks of the open cave network. Nook seeds
    // (≥4 of 8 neighbors solid) are rare among random picks, so the budget
    // is deliberately large — this loop is what gets us to the spec's ~2000.
    let mut attempts = params.cobweb_target * 250;
    while placed < params.cobweb_target && attempts > 0 {
        attempts -= 1;
        let x = rng.gen_range_u32(1..params.width - 1);
        let y = rng.gen_range_u32(params.cobweb_rows.0..params.cobweb_rows.1);
        if !is_webbable(world, x, y) || solid_8_neighbors(world, x, y) < 4 {
            continue;
        }
        // A small irregular cluster around the seed.
        for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
            let (cx, cy) = ((x as i64 + dx) as u32, (y as i64 + dy) as u32);
            if ((dx == 0 && dy == 0) || rng.chance(0.5)) && is_webbable(world, cx, cy) {
                let mut t = world.tile(cx, cy);
                t.id = TileId::Cobweb;
                t.state = 0;
                world.set_tile(cx, cy, t);
                placed += 1;
            }
        }
    }
}

fn is_webbable(world: &World, x: u32, y: u32) -> bool {
    let t = world.tile(x, y);
    world.in_bounds(x, y) && t.id == TileId::Air && t.liquid.is_none()
}

fn solid_8_neighbors(world: &World, x: u32, y: u32) -> u32 {
    let mut n = 0;
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if (dx != 0 || dy != 0) && world.is_solid(x as i32 + dx, y as i32 + dy) {
                n += 1;
            }
        }
    }
    n
}
