//! TileRunner — the drunken-walk brush behind caves, blobs, and ore veins
//! (DESIGN §1.2 pass 4, also reused by passes 2, 3, 6, 8).
//!
//! Per step: apply a circle of radius √S at `p`; `p += d × √S × 0.5`;
//! `d += rand(−0.5, 0.5)` on each axis (plus the configured downward bias);
//! clamp `|d|` components to ±1; 5% chance per step to branch a child worm
//! with `S×0.6` and `N×0.5` (child gets a fresh random direction; N is the
//! parent's *initial* step count — DESIGN doesn't say, canonized here).

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::TileId;
use ferraria_shared::world::World;

/// §1.2: 5% branch chance per step.
const BRANCH_CHANCE: f32 = 0.05;
/// Safety cap on total branches per root worm (not in DESIGN; prevents
/// pathological RNG runs from exploding generation time).
const MAX_BRANCHES: u32 = 32;

/// What the runner does to each covered cell.
#[derive(Clone, Copy)]
pub enum Brush {
    /// Cave digging: any foreground tile becomes air (walls/liquid kept).
    Carve,
    /// Blob/ore painting: cells whose id is in `replaces` become `tile`.
    Paint {
        tile: TileId,
        replaces: &'static [TileId],
    },
}

/// Row clamps and direction bias for one runner invocation.
pub struct RunnerOpts<'a> {
    /// Added to `d.y` every step (§1.2: surface caves bias +0.3, dig down).
    pub bias_y: f32,
    /// Brush applies only to rows in `[rows.0, rows.1)` (ore row ranges).
    pub rows: (u32, u32),
    /// Optional per-column exclusive max row (§1.2 pass 8: sand clamped to
    /// ≤25 rows below the local surface).
    pub col_max_y: Option<&'a [u32]>,
}

impl Default for RunnerOpts<'_> {
    fn default() -> Self {
        RunnerOpts {
            bias_y: 0.0,
            rows: (0, u32::MAX),
            col_max_y: None,
        }
    }
}

struct Worm {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    strength: f32,
    steps: u32,
}

/// Runs one TileRunner worm (plus any children it branches) from `start`.
pub fn tile_runner(
    world: &mut World,
    rng: &mut Pcg32,
    start: (f32, f32),
    strength: f32,
    steps: u32,
    brush: Brush,
    opts: &RunnerOpts,
) {
    let angle = rng.gen_range_f32(0.0, std::f32::consts::TAU);
    let mut queue = vec![Worm {
        x: start.0,
        y: start.1,
        dx: angle.cos(),
        dy: angle.sin(),
        strength,
        steps,
    }];
    let mut branches = 0;

    while let Some(mut worm) = queue.pop() {
        let radius = worm.strength.sqrt();
        for _ in 0..worm.steps {
            apply_circle(world, brush, worm.x, worm.y, worm.strength, opts);
            worm.x += worm.dx * radius * 0.5;
            worm.y += worm.dy * radius * 0.5;
            worm.dx = (worm.dx + rng.gen_range_f32(-0.5, 0.5)).clamp(-1.0, 1.0);
            worm.dy = (worm.dy + rng.gen_range_f32(-0.5, 0.5) + opts.bias_y).clamp(-1.0, 1.0);

            let child_strength = worm.strength * 0.6;
            if rng.chance(BRANCH_CHANCE) && branches < MAX_BRANCHES && child_strength >= 1.0 {
                branches += 1;
                let a = rng.gen_range_f32(0.0, std::f32::consts::TAU);
                queue.push(Worm {
                    x: worm.x,
                    y: worm.y,
                    dx: a.cos(),
                    dy: a.sin(),
                    strength: child_strength,
                    steps: (steps / 2).max(1),
                });
            }
        }
    }
}

/// Applies `brush` to every cell whose center is within √S of `(cx, cy)`.
fn apply_circle(world: &mut World, brush: Brush, cx: f32, cy: f32, s: f32, opts: &RunnerOpts) {
    let r = s.sqrt();
    let x0 = (cx - r).floor().max(0.0) as u32;
    let x1 = ((cx + r).ceil().max(0.0) as u32).min(world.width.saturating_sub(1));
    let y0 = ((cy - r).floor().max(0.0) as u32).max(opts.rows.0);
    let y1 = ((cy + r).ceil().max(0.0) as u32).min(world.height.saturating_sub(1));

    for y in y0..=y1.min(opts.rows.1.saturating_sub(1)) {
        for x in x0..=x1 {
            if let Some(col_max) = opts.col_max_y {
                if y >= col_max[x as usize] {
                    continue;
                }
            }
            let ddx = x as f32 + 0.5 - cx;
            let ddy = y as f32 + 0.5 - cy;
            if ddx * ddx + ddy * ddy > s {
                continue;
            }
            let mut t = world.tile(x, y);
            match brush {
                Brush::Carve => {
                    if t.id != TileId::Air {
                        t.id = TileId::Air;
                        t.state = 0;
                        world.set_tile(x, y, t);
                    }
                }
                Brush::Paint { tile, replaces } => {
                    if replaces.contains(&t.id) {
                        t.id = tile;
                        t.state = 0;
                        world.set_tile(x, y, t);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferraria_shared::tiles::Tile;

    #[test]
    fn carve_opens_a_tunnel_and_keeps_walls() {
        let mut w = World::new(100, 100);
        for y in 0..100 {
            for x in 0..100 {
                let mut t = Tile::of(TileId::Stone);
                t.wall = ferraria_shared::tiles::WallId::Stone;
                w.set_tile(x, y, t);
            }
        }
        let mut rng = Pcg32::new(1);
        tile_runner(
            &mut w,
            &mut rng,
            (50.0, 50.0),
            9.0,
            20,
            Brush::Carve,
            &RunnerOpts::default(),
        );
        let air = w.tiles.iter().filter(|t| t.id == TileId::Air).count();
        assert!(air > 50, "worm carved only {air} tiles");
        // Walls survive carving.
        assert!(w
            .tiles
            .iter()
            .all(|t| t.wall == ferraria_shared::tiles::WallId::Stone));
    }

    #[test]
    fn paint_respects_replace_filter_and_rows() {
        let mut w = World::new(60, 60);
        for y in 30..60 {
            for x in 0..60 {
                w.set_tile(x, y, Tile::of(TileId::Stone));
            }
        }
        let mut rng = Pcg32::new(2);
        tile_runner(
            &mut w,
            &mut rng,
            (30.0, 40.0),
            6.0,
            10,
            Brush::Paint {
                tile: TileId::CopperOre,
                replaces: &[TileId::Stone],
            },
            &RunnerOpts {
                rows: (35, 50),
                ..RunnerOpts::default()
            },
        );
        let mut painted = 0;
        for y in 0..60 {
            for x in 0..60 {
                if w.tile(x, y).id == TileId::CopperOre {
                    painted += 1;
                    assert!((35..50).contains(&y), "ore outside row clamp at y={y}");
                }
            }
        }
        assert!(painted > 0, "vein painted nothing");
        // Air cells (rows 0..30) were never painted.
        for y in 0..30 {
            for x in 0..60 {
                assert_eq!(w.tile(x, y).id, TileId::Air);
            }
        }
    }
}
