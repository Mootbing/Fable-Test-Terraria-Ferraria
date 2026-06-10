//! Connected-air-region ("pocket") labeling, used by the fluid fill (pass 7)
//! and cobweb placement (pass 11).

use ferraria_shared::tiles::TileId;
use ferraria_shared::world::World;

/// One connected region of air cells (4-connectivity).
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub min_y: u32,
    pub max_y: u32,
    pub cells: u32,
    /// The region reaches above `sky_row` — i.e. it is connected to the open
    /// sky and is *not* an enclosed pocket (§1.2 pass 7).
    pub open_to_sky: bool,
}

/// Labels for every cell plus per-region stats. `label[i] == 0` means "not
/// air"; region ids start at 1 and index `regions[label - 1]`.
pub struct Pockets {
    pub label: Vec<u32>,
    pub regions: Vec<Region>,
}

impl Pockets {
    #[inline]
    pub fn region_at(&self, world: &World, x: u32, y: u32) -> Option<u32> {
        let id = self.label[(y * world.width + x) as usize];
        (id != 0).then_some(id)
    }
}

/// Flood-fills all `TileId::Air` cells into regions. `sky_row` marks the row
/// above which air is considered "open sky" (any region touching it is open).
pub fn find_air_pockets(world: &World, sky_row: u32) -> Pockets {
    let w = world.width as usize;
    let h = world.height as usize;
    let mut label = vec![0u32; w * h];
    let mut regions: Vec<Region> = Vec::new();
    let mut stack: Vec<u32> = Vec::new();

    let is_air = |idx: usize| world.tiles[idx].id == TileId::Air;

    for start in 0..w * h {
        if label[start] != 0 || !is_air(start) {
            continue;
        }
        let id = regions.len() as u32 + 1;
        let mut region = Region {
            min_y: u32::MAX,
            max_y: 0,
            cells: 0,
            open_to_sky: false,
        };
        label[start] = id;
        stack.push(start as u32);
        while let Some(idx) = stack.pop() {
            let idx = idx as usize;
            let (x, y) = (idx % w, idx / w);
            region.cells += 1;
            region.min_y = region.min_y.min(y as u32);
            region.max_y = region.max_y.max(y as u32);
            if (y as u32) < sky_row {
                region.open_to_sky = true;
            }
            let mut visit = |n: usize| {
                if label[n] == 0 && is_air(n) {
                    label[n] = id;
                    stack.push(n as u32);
                }
            };
            if x > 0 {
                visit(idx - 1);
            }
            if x + 1 < w {
                visit(idx + 1);
            }
            if y > 0 {
                visit(idx - w);
            }
            if y + 1 < h {
                visit(idx + w);
            }
        }
        regions.push(region);
    }

    Pockets { label, regions }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferraria_shared::tiles::Tile;

    #[test]
    fn labels_separate_pockets_and_flags_sky() {
        // Solid world with two pockets, one connected to the sky.
        let mut w = World::new(20, 20);
        for y in 5..20 {
            for x in 0..20 {
                w.set_tile(x, y, Tile::of(TileId::Stone));
            }
        }
        // Enclosed 3x2 pocket.
        for y in 10..12 {
            for x in 3..6 {
                w.set_tile(x, y, Tile::AIR);
            }
        }
        // Shaft open to the sky.
        for y in 5..15 {
            w.set_tile(12, y, Tile::AIR);
        }

        let p = find_air_pockets(&w, 5);
        let enclosed = p.regions.iter().filter(|r| !r.open_to_sky).count();
        assert_eq!(enclosed, 1);
        let pocket = p
            .regions
            .iter()
            .find(|r| !r.open_to_sky)
            .expect("enclosed pocket");
        assert_eq!(pocket.cells, 6);
        assert_eq!((pocket.min_y, pocket.max_y), (10, 11));
        // The shaft and the sky share one open region.
        let shaft_id = p.region_at(&w, 12, 10).expect("labeled");
        let sky_id = p.region_at(&w, 0, 0).expect("labeled");
        assert_eq!(shaft_id, sky_id);
        assert!(p.regions[(shaft_id - 1) as usize].open_to_sky);
        // Solid cells carry no label.
        assert_eq!(p.region_at(&w, 0, 19), None);
    }
}
