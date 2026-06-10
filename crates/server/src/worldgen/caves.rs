//! Passes 4–5: TileRunner caves (surface worms, cavern worms, the underworld
//! lava-lake band) and cellular-automata smoothing (DESIGN §1.2).

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::{Solidity, Tile, TileId};
use ferraria_shared::world::World;

use super::noise::Noise1d;
use super::runner::{tile_runner, Brush, RunnerOpts};
use super::GenParams;

/// Pass 4a: 80 surface worms starting on the heightmap, S 4–8, N 15–30,
/// biased downward (d.y +0.3).
pub fn surface_caves(world: &mut World, params: &GenParams, rng: &mut Pcg32, surface: &[u32]) {
    for _ in 0..params.surface_worms {
        let x = rng.gen_range_u32(0..params.width);
        let y = surface[x as usize];
        let s = rng.gen_range_f32(
            params.surface_worm_strength.0,
            params.surface_worm_strength.1,
        );
        let n = rng.gen_range_u32(params.surface_worm_steps.0..params.surface_worm_steps.1 + 1);
        tile_runner(
            world,
            rng,
            (x as f32, y as f32),
            s,
            n,
            Brush::Carve,
            &RunnerOpts {
                bias_y: params.surface_worm_bias_y,
                ..RunnerOpts::default()
            },
        );
    }
}

/// Pass 4b: 250 cavern worms, start rows 450–980, S 10–22, N 60–100,
/// unbiased.
pub fn cavern_caves(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    for _ in 0..params.cavern_worms {
        let x = rng.gen_range_u32(0..params.width);
        let y = rng.gen_range_u32(params.cavern_worm_rows.0..params.cavern_worm_rows.1);
        let s = rng.gen_range_f32(params.cavern_worm_strength.0, params.cavern_worm_strength.1);
        let n = rng.gen_range_u32(params.cavern_worm_steps.0..params.cavern_worm_steps.1 + 1);
        tile_runner(
            world,
            rng,
            (x as f32, y as f32),
            s,
            n,
            Brush::Carve,
            &RunnerOpts::default(),
        );
    }
}

/// Pass 4c: the underworld lava-lake band. A 1D-noise ceiling (canonized to
/// rows 1020–1050; DESIGN names only the floor band) and a 1D-noise floor
/// between rows 1060–1140; everything between them is opened. The §1.2 lava
/// fill ("open tiles below row 1100") happens in the fluids pass so CA
/// smoothing (pass 5) never runs over liquid.
pub fn underworld_band(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    let ceiling_noise = Noise1d::new(rng, params.width, params.hell_noise_wavelength);
    let floor_noise = Noise1d::new(rng, params.width, params.hell_noise_wavelength);
    for x in 0..params.width {
        let t = (ceiling_noise.sample(x as f32) + 1.0) / 2.0;
        let ceiling = params.hell_ceiling_rows.0 as f32
            + t * (params.hell_ceiling_rows.1 - params.hell_ceiling_rows.0) as f32;
        let t = (floor_noise.sample(x as f32) + 1.0) / 2.0;
        let floor = params.hell_floor_rows.0 as f32
            + t * (params.hell_floor_rows.1 - params.hell_floor_rows.0) as f32;
        for y in ceiling.round() as u32..(floor.round() as u32).min(params.height) {
            world.set_tile(x, y, Tile::AIR);
        }
    }
}

/// Pass 5: cellular-automata majority smoothing. Per pass (synchronous, over
/// a snapshot): a solid tile with ≤3 solid 8-neighbors becomes air; an air
/// tile with ≥6 solid 8-neighbors becomes the most common solid neighbor.
/// Out-of-world counts as solid. Runs over everything from just above the
/// surface band down (the sky can't change: air with ≤ 3 solid neighbors).
pub fn smooth(world: &mut World, params: &GenParams) {
    for _ in 0..params.smoothing_passes {
        smooth_once(world, params.surface_min.saturating_sub(10));
    }
}

fn smooth_once(world: &mut World, y_start: u32) {
    let w = world.width as usize;
    let h = world.height as usize;
    let ids: Vec<TileId> = world.tiles.iter().map(|t| t.id).collect();
    let solid: Vec<bool> = ids
        .iter()
        .map(|id| matches!(id.data().solidity, Solidity::Solid))
        .collect();

    let mut neighbor_ids = [TileId::Air; 8];
    for y in y_start as usize..h {
        for x in 0..w {
            let i = y * w + x;
            // Count solid 8-neighbors; out-of-world counts as solid.
            let mut solid_count = 0;
            let mut n = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        solid_count += 1;
                        continue;
                    }
                    let ni = ny as usize * w + nx as usize;
                    if solid[ni] {
                        solid_count += 1;
                        neighbor_ids[n] = ids[ni];
                        n += 1;
                    }
                }
            }
            if solid[i] && solid_count <= 3 {
                let t = &mut world.tiles[i];
                t.id = TileId::Air;
                t.state = 0;
            } else if ids[i] == TileId::Air && solid_count >= 6 {
                // Walls are intentionally untouched (cave walls stay).
                let t = &mut world.tiles[i];
                t.id = majority(&neighbor_ids[..n]);
                t.state = 0;
            }
        }
    }
}

/// Most frequent id in `ids` (non-empty; ties go to the earliest scanned).
fn majority(ids: &[TileId]) -> TileId {
    let mut best = ids[0];
    let mut best_count = 0;
    for &candidate in ids {
        let count = ids.iter().filter(|&&i| i == candidate).count();
        if count > best_count {
            best = candidate;
            best_count = count;
        }
    }
    best
}

/// Recomputes the *actual* surface after carving: the first fully solid row
/// in each column (`height` if a column is somehow all air).
pub fn surface_scan(world: &World) -> Vec<u32> {
    (0..world.width)
        .map(|x| {
            (0..world.height)
                .find(|&y| world.tile(x, y).is_solid())
                .unwrap_or(world.height)
        })
        .collect()
}
