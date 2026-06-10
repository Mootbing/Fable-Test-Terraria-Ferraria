//! Pass 6: ore veins — TileRunner painting ore into stone/dirt/ash
//! (DESIGN §1.2 table; Hellstone only replaces ash). Painting is clamped to
//! each ore's row range so veins never stray outside their §1.2 band.

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::TileId;
use ferraria_shared::world::World;

use super::runner::{tile_runner, Brush, RunnerOpts};
use super::GenParams;

/// One row of the §1.2 ore table (rows half-open, already scaled).
#[derive(Debug, Clone, Copy)]
pub struct OreSpec {
    pub tile: TileId,
    pub veins: u32,
    pub strength: (f32, f32),
    pub steps: (u32, u32),
    pub rows: (u32, u32),
    pub replaces: &'static [TileId],
}

/// What the four metal ores may replace (§1.2: "painting ore into
/// stone/dirt/ash").
pub const ORE_REPLACES: &[TileId] = &[TileId::Stone, TileId::Dirt, TileId::Ash];
/// Hellstone "only replaces ash".
pub const HELLSTONE_REPLACES: &[TileId] = &[TileId::Ash];

pub fn ore_veins(world: &mut World, params: &GenParams, rng: &mut Pcg32) {
    for spec in &params.ores {
        for _ in 0..spec.veins {
            let x = rng.gen_range_u32(0..params.width);
            let y = rng.gen_range_u32(spec.rows.0..spec.rows.1);
            let s = rng.gen_range_f32(spec.strength.0, spec.strength.1);
            let n = rng.gen_range_u32(spec.steps.0..spec.steps.1 + 1);
            tile_runner(
                world,
                rng,
                (x as f32, y as f32),
                s,
                n,
                Brush::Paint {
                    tile: spec.tile,
                    replaces: spec.replaces,
                },
                &RunnerOpts {
                    rows: spec.rows,
                    ..RunnerOpts::default()
                },
            );
        }
    }
}
