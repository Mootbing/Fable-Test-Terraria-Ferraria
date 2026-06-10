//! Passes 8–9: sand patches, grass, trees, and mushroom forage
//! (DESIGN §1.2).

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::{state, TileId};
use ferraria_shared::world::World;

use super::caves::surface_scan;
use super::runner::{tile_runner, Brush, RunnerOpts};
use super::GenParams;

/// Pass 8: 10 sand patches at random heightmap positions at least 150 tiles
/// from the (future, center) spawn; TileRunner paint S 15–30, N 10–20,
/// clamped to ≤25 rows below the local surface.
pub fn sand_patches(world: &mut World, params: &GenParams, rng: &mut Pcg32, surface: &[u32]) {
    let center = params.width / 2;
    let col_max: Vec<u32> = surface
        .iter()
        .map(|&s| (s + params.sand_max_depth + 1).min(params.height))
        .collect();
    for _ in 0..params.sand_patches {
        // Rejection-sample a start column far enough from spawn.
        let mut x = rng.gen_range_u32(0..params.width);
        for _ in 0..64 {
            if x.abs_diff(center) >= params.sand_min_spawn_dist {
                break;
            }
            x = rng.gen_range_u32(0..params.width);
        }
        let s = rng.gen_range_f32(params.sand_strength.0, params.sand_strength.1);
        let n = rng.gen_range_u32(params.sand_steps.0..params.sand_steps.1 + 1);
        tile_runner(
            world,
            rng,
            (x as f32, surface[x as usize] as f32),
            s,
            n,
            Brush::Paint {
                tile: TileId::Sand,
                replaces: &[TileId::Dirt, TileId::Stone, TileId::Clay],
            },
            &RunnerOpts {
                col_max_y: Some(&col_max),
                ..RunnerOpts::default()
            },
        );
    }
}

/// Pass 9: every dirt tile with air directly above becomes grass; then trees
/// every 5–15 tiles on grass (7–16 trunk segments, ≥4 tiles apart) and one
/// mushroom forage plant per ~40 surface tiles (stored as a sprite variant
/// on the grass cell, [`state::GRASS_MUSHROOM`]).
pub fn grass_and_flora(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    // Grass: literal §1.2 rule, applied everywhere (cave dirt with an air
    // ceiling above it greens too — grass spread keeps it alive later).
    let w = world.width as usize;
    for y in 1..params.height {
        for x in 0..params.width {
            let idx = y as usize * w + x as usize;
            if world.tiles[idx].id == TileId::Dirt && world.tiles[idx - w].id == TileId::Air {
                world.tiles[idx].id = TileId::Grass;
            }
        }
    }

    // Trees + mushrooms walk the actual surface.
    let surface = surface_scan(world);
    let mut next_tree = rng.gen_range_u32(params.tree_gap.0..params.tree_gap.1 + 1);
    let mut last_tree: Option<u32> = None;
    for x in 0..params.width {
        let sy = surface[x as usize];
        if sy == 0 || sy >= params.height {
            continue;
        }
        let on_grass = world.tile(x, sy).id == TileId::Grass;

        let mut planted = false;
        if next_tree > 0 {
            next_tree -= 1;
        } else if on_grass && last_tree.is_none_or(|lx| x - lx >= params.tree_min_separation) {
            let height = rng.gen_range_u32(params.tree_height.0..params.tree_height.1 + 1);
            if plant_tree(world, x, sy, height) {
                last_tree = Some(x);
                next_tree = rng.gen_range_u32(params.tree_gap.0..params.tree_gap.1 + 1);
                planted = true;
            }
        }

        // Mushroom forage: ~1 per 40 surface tiles, never under a tree.
        if on_grass && !planted && rng.chance(1.0 / params.mushroom_per_tiles as f32) {
            let mut t = world.tile(x, sy);
            t.state = state::GRASS_MUSHROOM;
            world.set_tile(x, sy, t);
        }
    }
}

/// Plants a tree whose lowest trunk segment sits directly on `(x, ground)`.
/// Trunk cells use the frame byte: `TREE_SEGMENT_TRUNK` below,
/// `TREE_SEGMENT_TOP` for the crown. Fails (placing nothing) if any trunk
/// cell is obstructed.
pub fn plant_tree(world: &mut World, x: u32, ground: u32, height: u32) -> bool {
    if ground < height {
        return false;
    }
    for i in 1..=height {
        if !world.is_empty(x, ground - i) {
            return false;
        }
    }
    for i in 1..=height {
        let mut t = world.tile(x, ground - i);
        t.id = TileId::TreeTrunk;
        t.state = if i == height {
            state::TREE_SEGMENT_TOP
        } else {
            state::TREE_SEGMENT_TRUNK
        };
        world.set_tile(x, ground - i, t);
    }
    true
}
