//! World generation invariants (DESIGN §1) on one shared full-size world
//! (seed 42, 4200×1200 — generation is ~2 s in debug, built once via
//! `OnceLock`).

use std::sync::OnceLock;

use ferraria_server::worldgen::{generate, generate_with_size, GenParams};
use ferraria_shared::tiles::{state, LiquidKind, TileId};
use ferraria_shared::world::{decode_chunk, World, CHUNK_SIZE};

static WORLD: OnceLock<World> = OnceLock::new();

fn world() -> &'static World {
    WORLD.get_or_init(|| generate(42))
}

fn params() -> GenParams {
    GenParams::new(world().width, world().height)
}

fn count_tiles(w: &World, id: TileId) -> usize {
    w.tiles.iter().filter(|t| t.id == id).count()
}

#[test]
fn generation_completes_with_full_dimensions() {
    let w = world();
    assert_eq!((w.width, w.height), (4200, 1200));
    assert_eq!(w.tiles.len(), 4200 * 1200);
    assert!(w.is_day(), "new worlds start at 8:15 AM");
}

#[test]
fn spawn_is_on_solid_ground_with_air_above() {
    let w = world();
    let (x, y) = w.spawn;
    assert!(
        w.tile(x, y + 1).is_solid(),
        "no platform under spawn {:?}",
        w.spawn
    );
    for dy in 0..3 {
        assert_eq!(
            w.tile(x, y - dy).id,
            TileId::Air,
            "spawn not clear at {} above feet",
            dy
        );
    }
    // §1.2 pass 12: world center column ± 20.
    assert!(x.abs_diff(w.width / 2) <= 20);
    // The 10-tile platform is flat and solid.
    let ground = y + 1;
    for px in (x - 5)..(x + 5) {
        assert!(w.tile(px, ground).is_solid(), "hole in platform at {px}");
        assert_eq!(w.tile(px, ground - 1).id, TileId::Air);
    }
}

#[test]
fn all_five_ores_present_within_their_row_bands() {
    let w = world();
    let p = params();
    for spec in &p.ores {
        let mut count = 0usize;
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for y in 0..w.height {
            for x in 0..w.width {
                if w.tile(x, y).id == spec.tile {
                    count += 1;
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        // Plausible volume: a vein paints well over 8 tiles on average.
        assert!(
            count as u32 >= spec.veins * 8,
            "{:?}: only {count} tiles from {} veins",
            spec.tile,
            spec.veins
        );
        // Painting is clamped to the §1.2 row band.
        assert!(
            min_y >= spec.rows.0 && max_y < spec.rows.1,
            "{:?} outside rows {:?}: found {min_y}..={max_y}",
            spec.tile,
            spec.rows
        );
    }
}

#[test]
fn underground_chests_are_plentiful_and_looted() {
    let w = world();
    let p = params();
    let underground = w
        .chests
        .keys()
        .filter(|&&(_, y)| {
            // Origin is 2 above the floor row, which must be in 350–999.
            (p.underground_chest_rows.0..p.underground_chest_rows.1).contains(&(y + 2))
        })
        .count();
    assert!(
        underground >= 100,
        "only {underground} underground chests (want >= 100 of {})",
        p.underground_chests
    );
    // Every chest has loot and a real 2×2 chest at its origin.
    for (&(x, y), slots) in &w.chests {
        assert!(
            slots.iter().any(|s| s.is_some()),
            "empty chest at ({x},{y})"
        );
        for dy in 0..2 {
            for dx in 0..2 {
                let t = w.tile(x + dx, y + dy);
                assert_eq!(t.id, TileId::Chest, "broken chest at ({x},{y})");
                assert_eq!(state::part_x(t.state) as u32, dx);
                assert_eq!(state::part_y(t.state) as u32, dy);
            }
        }
    }
    // No stray chest tiles without contents.
    let chest_tiles = count_tiles(w, TileId::Chest);
    assert_eq!(chest_tiles, w.chests.len() * 4, "orphaned chest tiles");
}

#[test]
fn thirty_ritual_altars_on_cavern_floors() {
    let w = world();
    let p = params();
    let mut origins = 0;
    for y in 0..w.height {
        for x in 0..w.width {
            let t = w.tile(x, y);
            if t.id == TileId::RitualAltar && t.state == state::part(0, 0) {
                origins += 1;
                // Altars are 3×2, standing on solid ground in 500–999.
                assert!(
                    (p.altar_rows.0..p.altar_rows.1).contains(&(y + 2)),
                    "altar at ({x},{y}) outside cavern band"
                );
                for dx in 0..3 {
                    assert!(w.tile(x + dx, y + 2).is_solid(), "floating altar");
                }
            }
        }
    }
    assert_eq!(origins, 30);
}

#[test]
fn underworld_has_lava_lakes_and_hellstone() {
    let w = world();
    let p = params();
    let mut lava = 0usize;
    for y in p.stone_to_ash..w.height {
        for x in 0..w.width {
            if w.tile(x, y).liquid.kind() == Some(LiquidKind::Lava) {
                lava += 1;
            }
        }
    }
    assert!(
        lava > 5_000,
        "underworld lava lakes too small: {lava} cells"
    );
    let hellstone = count_tiles(w, TileId::Hellstone);
    assert!(hellstone > 1_000, "only {hellstone} hellstone tiles");
    // Infernal forges exist (one per ruin).
    let forges = w
        .tiles
        .iter()
        .filter(|t| t.id == TileId::InfernalForge && t.state == state::part(0, 0))
        .count();
    assert!(
        (1..=p.ruins as usize).contains(&forges) && forges >= 5,
        "{forges} forges from {} ruins",
        p.ruins
    );
}

#[test]
fn no_water_cell_touches_a_lava_cell() {
    let w = world();
    for y in 0..w.height {
        for x in 0..w.width {
            if w.tile(x, y).liquid.kind() != Some(LiquidKind::Water) {
                continue;
            }
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                assert_ne!(
                    w.tile(nx, ny).liquid.kind(),
                    Some(LiquidKind::Lava),
                    "water at ({x},{y}) adjacent to lava — obsidian rule failed"
                );
            }
        }
    }
}

#[test]
fn grass_only_grows_exposed() {
    let w = world();
    let mut grass = 0usize;
    for y in 1..w.height {
        for x in 0..w.width {
            if w.tile(x, y).id == TileId::Grass {
                grass += 1;
                assert!(!w.tile(x, y - 1).is_solid(), "covered grass at ({x},{y})");
            }
        }
    }
    assert!(grass > 1_000, "barely any grass: {grass}");
}

#[test]
fn tree_trunks_have_valid_frame_bytes() {
    let w = world();
    let mut tops = 0usize;
    let mut segments = 0usize;
    for y in 1..w.height - 1 {
        for x in 0..w.width {
            let t = w.tile(x, y);
            if t.id != TileId::TreeTrunk {
                continue;
            }
            segments += 1;
            match t.state {
                state::TREE_SEGMENT_TOP => {
                    tops += 1;
                    assert_ne!(
                        w.tile(x, y - 1).id,
                        TileId::TreeTrunk,
                        "top segment with trunk above at ({x},{y})"
                    );
                }
                state::TREE_SEGMENT_TRUNK => {
                    assert_eq!(
                        w.tile(x, y - 1).id,
                        TileId::TreeTrunk,
                        "trunk segment with no tree above at ({x},{y})"
                    );
                }
                other => panic!("invalid tree frame byte {other} at ({x},{y})"),
            }
            // Trunks stand on something: solid ground or more trunk.
            let below = w.tile(x, y + 1);
            assert!(
                below.is_solid() || below.id == TileId::TreeTrunk,
                "floating tree segment at ({x},{y})"
            );
        }
    }
    assert!(tops > 50, "too few trees: {tops}");
    // §1.2: 7–16 segments per tree on average.
    let avg = segments as f64 / tops as f64;
    assert!((7.0..=16.0).contains(&avg), "weird tree height avg {avg}");
}

#[test]
fn life_crystals_pots_and_cobwebs_exist() {
    let w = world();
    let p = params();
    let crystals = count_tiles(w, TileId::LifeCrystal);
    assert!(
        crystals as u32 >= p.life_crystals / 2,
        "only {crystals} life crystals"
    );
    let pots = count_tiles(w, TileId::Pot);
    assert!(pots as u32 >= p.pots / 2, "only {pots} pots");
    let cobwebs = count_tiles(w, TileId::Cobweb);
    assert!(cobwebs > 200, "only {cobwebs} cobwebs");
}

#[test]
fn chunk_roundtrip_on_generated_data() {
    let w = world();
    // Sample chunks across the world, including the ragged right/bottom
    // edges and the spawn chunk.
    let mut coords = vec![
        (w.spawn.0 / CHUNK_SIZE, w.spawn.1 / CHUNK_SIZE),
        (w.chunks_x() - 1, w.chunks_y() - 1),
        (0, 0),
    ];
    for cy in (0..w.chunks_y()).step_by(5) {
        for cx in (0..w.chunks_x()).step_by(7) {
            coords.push((cx, cy));
        }
    }
    for (cx, cy) in coords {
        let encoded = w.encode_chunk(cx, cy);
        let tiles = decode_chunk(&encoded).expect("generated chunk decodes");
        for (i, &t) in tiles.iter().enumerate() {
            let x = cx * CHUNK_SIZE + i as u32 % CHUNK_SIZE;
            let y = cy * CHUNK_SIZE + i as u32 / CHUNK_SIZE;
            assert_eq!(t, w.tile(x, y), "chunk ({cx},{cy}) mismatch at ({x},{y})");
        }
    }
}

#[test]
fn generation_is_deterministic() {
    let a = generate_with_size(7, 600, 400);
    let b = generate_with_size(7, 600, 400);
    assert_eq!(a.spawn, b.spawn);
    assert_eq!(a.tiles, b.tiles);
    assert_eq!(a.chests, b.chests);
    // And different seeds diverge.
    let c = generate_with_size(8, 600, 400);
    assert_ne!(a.tiles, c.tiles);
}

#[test]
fn scaled_world_keeps_the_layer_structure() {
    let w = generate_with_size(123, 1200, 600);
    let p = GenParams::new(1200, 600);
    // Spawn valid.
    let (x, y) = w.spawn;
    assert!(w.tile(x, y + 1).is_solid());
    assert_eq!(w.tile(x, y).id, TileId::Air);
    // Layers exist: dirt band, stone band, ash band.
    assert!(count_tiles(&w, TileId::Dirt) > 10_000);
    assert!(count_tiles(&w, TileId::Stone) > 50_000);
    assert!(count_tiles(&w, TileId::Ash) > 10_000);
    // Scaled structure counts.
    assert!(w.chests.len() as u32 >= p.underground_chests / 2);
    let altars = w
        .tiles
        .iter()
        .filter(|t| t.id == TileId::RitualAltar && t.state == state::part(0, 0))
        .count();
    assert_eq!(altars as u32, p.altars);
}

/// Belt-and-braces run on a second full-size seed (slow; `--ignored`).
#[test]
#[ignore]
fn full_size_second_seed_invariants() {
    let w = generate(1337);
    let (x, y) = w.spawn;
    assert!(w.tile(x, y + 1).is_solid());
    assert_eq!(w.tile(x, y).id, TileId::Air);
    assert!(w.chests.len() >= 100);
    for y in 0..w.height {
        for x in 0..w.width {
            if w.tile(x, y).liquid.kind() == Some(LiquidKind::Water) {
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    assert_ne!(w.tile(nx, ny).liquid.kind(), Some(LiquidKind::Lava));
                }
            }
        }
    }
    let altars = w
        .tiles
        .iter()
        .filter(|t| t.id == TileId::RitualAltar && t.state == state::part(0, 0))
        .count();
    assert_eq!(altars, 30);
}

/// Prints generation stats for tuning (`cargo test -- --ignored stats`).
#[test]
#[ignore]
fn stats() {
    let w = world();
    let p = params();
    for spec in &p.ores {
        println!("{:?}: {}", spec.tile, count_tiles(w, spec.tile));
    }
    for id in [
        TileId::Pot,
        TileId::LifeCrystal,
        TileId::Cobweb,
        TileId::Obsidian,
        TileId::Chest,
        TileId::TreeTrunk,
        TileId::Sand,
        TileId::Grass,
    ] {
        println!("{id:?}: {}", count_tiles(w, id));
    }
    println!("chests: {}", w.chests.len());
    let lava = w
        .tiles
        .iter()
        .filter(|t| t.liquid.kind() == Some(LiquidKind::Lava))
        .count();
    let water = w
        .tiles
        .iter()
        .filter(|t| t.liquid.kind() == Some(LiquidKind::Water))
        .count();
    println!("water cells: {water}, lava cells: {lava}");
    println!("spawn: {:?}", w.spawn);

    // Enclosed-pocket size histogram in the cobweb band.
    let pockets = ferraria_server::worldgen::pockets::find_air_pockets(w, p.surface_min);
    let mut buckets = [0usize; 6]; // <=20, <=60, <=150, <=400, <=1000, big
    for r in pockets
        .regions
        .iter()
        .filter(|r| !r.open_to_sky && r.min_y >= p.cobweb_rows.0 && r.max_y < p.cobweb_rows.1)
    {
        let b = match r.cells {
            0..=20 => 0,
            21..=60 => 1,
            61..=150 => 2,
            151..=400 => 3,
            401..=1000 => 4,
            _ => 5,
        };
        buckets[b] += 1;
    }
    println!("enclosed pockets in cobweb band by size: {buckets:?}");
}
