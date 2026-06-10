//! Passes 1–3: surface heightmap + base layer fill, stone/dirt blobs, clay
//! (DESIGN §1.2).

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::{Liquid, Tile, TileId, WallId};
use ferraria_shared::world::World;

use super::noise::LayeredNoise;
use super::runner::{tile_runner, Brush, RunnerOpts};
use super::GenParams;

/// Pass 1a: the surface heightmap — 3-octave value noise around the baseline
/// row, clamped to the surface band.
pub fn heightmap(params: &GenParams, rng: &mut Pcg32) -> Vec<u32> {
    let noise = LayeredNoise::new(rng, params.width, &params.octaves);
    (0..params.width)
        .map(|x| {
            let v = params.surface_baseline as f32 + noise.sample(x as f32);
            (v.round() as i64).clamp(params.surface_min as i64, params.surface_max as i64) as u32
        })
        .collect()
}

/// Pass 1b: fill below the heightmap — dirt to the stone line, stone to the
/// ash line, ash below — and lay the natural background walls (dirt wall in
/// the dirt layer, stone wall in the caverns; the underworld has none).
/// Natural walls don't set `state::WALL_PLACED`, so they drop nothing.
pub fn fill_base(world: &mut World, params: &GenParams, surface: &[u32]) {
    let w = world.width as usize;
    for x in 0..params.width {
        let s = surface[x as usize];
        for y in s..params.height {
            let id = if y < params.dirt_to_stone {
                TileId::Dirt
            } else if y < params.stone_to_ash {
                TileId::Stone
            } else {
                TileId::Ash
            };
            let wall = if y <= s || y >= params.stone_to_ash {
                WallId::Air // none on the very surface row or in the underworld
            } else if y < params.dirt_to_stone {
                WallId::Dirt
            } else {
                WallId::Stone
            };
            world.tiles[y as usize * w + x as usize] = Tile {
                id,
                wall,
                liquid: Liquid::NONE,
                state: 0,
            };
        }
    }
}

/// Pass 2: 400 stone blobs painted into the dirt layer, 300 dirt blobs into
/// the stone layer (TileRunner painting, S 6–14, N 6–12).
pub fn stone_dirt_blobs(world: &mut World, params: &GenParams, rng: &mut Pcg32, surface: &[u32]) {
    for _ in 0..params.stone_blobs {
        let x = rng.gen_range_u32(0..params.width);
        let y = rng
            .gen_range_u32(surface[x as usize].min(params.dirt_to_stone - 1)..params.dirt_to_stone);
        blob(world, rng, params, (x, y), TileId::Stone, &[TileId::Dirt]);
    }
    for _ in 0..params.dirt_blobs {
        let x = rng.gen_range_u32(0..params.width);
        let y = rng.gen_range_u32(params.dirt_to_stone..params.stone_to_ash);
        blob(world, rng, params, (x, y), TileId::Dirt, &[TileId::Stone]);
    }
}

fn blob(
    world: &mut World,
    rng: &mut Pcg32,
    params: &GenParams,
    at: (u32, u32),
    tile: TileId,
    replaces: &'static [TileId],
) {
    let s = rng.gen_range_f32(params.blob_strength.0, params.blob_strength.1);
    let n = rng.gen_range_u32(params.blob_steps.0..params.blob_steps.1 + 1);
    tile_runner(
        world,
        rng,
        (at.0 as f32, at.1 as f32),
        s,
        n,
        Brush::Paint { tile, replaces },
        &RunnerOpts::default(),
    );
}

/// Pass 3: 300 clay blobs, rows 250–500, S 4–9, N 4–8.
pub fn clay(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    for _ in 0..params.clay_blobs {
        let x = rng.gen_range_u32(0..params.width);
        let y = rng.gen_range_u32(params.clay_rows.0..params.clay_rows.1);
        let s = rng.gen_range_f32(params.clay_strength.0, params.clay_strength.1);
        let n = rng.gen_range_u32(params.clay_steps.0..params.clay_steps.1 + 1);
        tile_runner(
            world,
            rng,
            (x as f32, y as f32),
            s,
            n,
            Brush::Paint {
                tile: TileId::Clay,
                replaces: &[TileId::Dirt, TileId::Stone],
            },
            &RunnerOpts::default(),
        );
    }
}
