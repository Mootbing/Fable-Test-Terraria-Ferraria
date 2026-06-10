//! Pass 7: fluid pocket fill + settling (DESIGN §1.2). Enclosed cave pockets
//! in the water band get water at 35% (filled to 25–75% of pocket height);
//! pockets in the lava band get lava at 30%. The underworld lava lakes
//! (deferred from pass 4c) are filled here too, then everything settles to
//! equilibrium with the §3 automaton — which also performs the §3.2 obsidian
//! conversions (pass 10).

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::{Liquid, LiquidKind, TileId, LIQUID_MAX_LEVEL};
use ferraria_shared::world::World;

use crate::sim::fluids;

use super::pockets::{find_air_pockets, Pockets};
use super::GenParams;

/// Fills pockets, fills the underworld lakes, settles. Returns the pocket
/// labeling for reuse by the cobweb pass.
pub fn fill_and_settle(world: &mut World, params: &GenParams, rng: &mut Pcg32) -> Pockets {
    let pockets = find_air_pockets(world, params.surface_min);

    // Decide one fill per enclosed pocket, in deterministic region order.
    // A pocket qualifies if it lies entirely inside rows 450–999; whether it
    // rolls water or lava follows the band its vertical midpoint is in
    // (canonized — DESIGN doesn't say how a pocket straddling row 800 is
    // classified).
    let mut fills: Vec<Option<(LiquidKind, u32)>> = Vec::with_capacity(pockets.regions.len());
    for region in &pockets.regions {
        if region.open_to_sky
            || region.min_y < params.water_rows.0
            || region.max_y >= params.lava_rows.1
        {
            fills.push(None);
            continue;
        }
        let mid = (region.min_y + region.max_y) / 2;
        let (kind, chance) = if mid < params.water_rows.1 {
            (LiquidKind::Water, params.water_chance)
        } else {
            (LiquidKind::Lava, params.lava_chance)
        };
        if !rng.chance(chance) {
            fills.push(None);
            continue;
        }
        let pocket_height = region.max_y - region.min_y + 1;
        let frac = rng.gen_range_f32(params.fill_frac.0, params.fill_frac.1);
        let rows_filled = ((pocket_height as f32 * frac).round() as u32).max(1);
        let fill_from = region.max_y + 1 - rows_filled;
        fills.push(Some((kind, fill_from)));
    }

    // Apply the chosen fills in one grid scan.
    let w = world.width as usize;
    for y in params.water_rows.0..params.lava_rows.1 {
        for x in 0..params.width {
            let idx = y as usize * w + x as usize;
            let id = pockets.label[idx];
            if id == 0 {
                continue;
            }
            if let Some((kind, fill_from)) = fills[(id - 1) as usize] {
                if y >= fill_from {
                    let t = &mut world.tiles[idx];
                    t.liquid = Liquid::new(kind, LIQUID_MAX_LEVEL);
                }
            }
        }
    }

    // Underworld lava lakes: every open tile strictly below the lava line
    // (§1.2 4c: "below row 1100").
    for y in params.hell_lava_row + 1..params.height {
        for x in 0..params.width {
            let idx = y as usize * w + x as usize;
            let t = &mut world.tiles[idx];
            if t.id == TileId::Air {
                t.liquid = Liquid::new(LiquidKind::Lava, LIQUID_MAX_LEVEL);
            }
        }
    }

    fluids::settle(world);
    pockets
}
