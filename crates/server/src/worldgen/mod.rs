//! Procedural world generation — DESIGN §1, all 12 passes in the fixed
//! order, seeded and fully deterministic (one shared-PCG32 stream per pass).
//!
//! Every §1 number lives in [`GenParams`]; [`GenParams::new`] scales the
//! 4200×1200 baselines to other world sizes (rows by height, counts by area,
//! noise wavelengths by width) so tests can generate smaller worlds with the
//! same structure.

mod caves;
mod flora;
mod fluids_fill;
pub mod loot;
pub mod noise;
mod ores;
pub mod pockets;
pub mod runner;
mod spawn;
mod structures;
mod terrain;

use ferraria_shared::rng::Pcg32;
use ferraria_shared::tiles::TileId;
use ferraria_shared::world::{World, WORLD_HEIGHT, WORLD_WIDTH};

pub use caves::surface_scan;
pub use ores::OreSpec;

/// DESIGN baselines that everything scales from.
const BASE_WIDTH: u32 = WORLD_WIDTH; // 4200
const BASE_HEIGHT: u32 = WORLD_HEIGHT; // 1200

/// The §1.2 pass list (index 0 = pass 1), reported through the progress
/// callback.
pub const PASS_NAMES: [&str; 12] = [
    "surface heightmap",
    "stone/dirt blobs",
    "clay",
    "caves (TileRunner)",
    "smoothing (CA)",
    "ore veins",
    "fluid pockets + settle",
    "sand patches",
    "grass & flora",
    "obsidian (settle check)",
    "structures",
    "spawn point",
];

/// All §1 generation parameters for one world size. Rows are half-open
/// `[start, end)` bands, already scaled.
#[derive(Debug, Clone)]
pub struct GenParams {
    pub width: u32,
    pub height: u32,
    // Pass 1 — §1.1 layers + §1.2 heightmap.
    pub surface_min: u32,
    pub surface_baseline: u32,
    pub surface_max: u32,
    /// `(wavelength, amplitude)` per octave: 120/40, 40/15, 10/5.
    pub octaves: [(f32, f32); 3],
    pub dirt_to_stone: u32,
    pub stone_to_ash: u32,
    // Pass 2.
    pub stone_blobs: u32,
    pub dirt_blobs: u32,
    pub blob_strength: (f32, f32),
    pub blob_steps: (u32, u32),
    // Pass 3.
    pub clay_blobs: u32,
    pub clay_rows: (u32, u32),
    pub clay_strength: (f32, f32),
    pub clay_steps: (u32, u32),
    // Pass 4.
    pub surface_worms: u32,
    pub surface_worm_strength: (f32, f32),
    pub surface_worm_steps: (u32, u32),
    pub surface_worm_bias_y: f32,
    pub cavern_worms: u32,
    pub cavern_worm_rows: (u32, u32),
    pub cavern_worm_strength: (f32, f32),
    pub cavern_worm_steps: (u32, u32),
    /// Underworld ceiling noise band (canonized: DESIGN names only the
    /// floor band).
    pub hell_ceiling_rows: (u32, u32),
    pub hell_floor_rows: (u32, u32),
    /// Open tiles at/below this row become lava.
    pub hell_lava_row: u32,
    pub hell_noise_wavelength: f32,
    // Pass 5.
    pub smoothing_passes: u32,
    // Pass 6.
    pub ores: Vec<OreSpec>,
    // Pass 7.
    pub water_rows: (u32, u32),
    pub water_chance: f32,
    pub lava_rows: (u32, u32),
    pub lava_chance: f32,
    pub fill_frac: (f32, f32),
    // Pass 8.
    pub sand_patches: u32,
    pub sand_min_spawn_dist: u32,
    pub sand_strength: (f32, f32),
    pub sand_steps: (u32, u32),
    pub sand_max_depth: u32,
    // Pass 9.
    pub tree_gap: (u32, u32),
    pub tree_height: (u32, u32),
    pub tree_min_separation: u32,
    pub mushroom_per_tiles: u32,
    // Pass 11.
    pub underground_chests: u32,
    pub underground_chest_rows: (u32, u32),
    pub chest_spacing: u32,
    pub surface_chests: u32,
    pub ruins: u32,
    pub ruin_size: (u32, u32),
    pub altars: u32,
    pub altar_rows: (u32, u32),
    pub life_crystals: u32,
    pub life_crystal_rows: (u32, u32),
    pub pots: u32,
    pub cobweb_rows: (u32, u32),
    pub cobweb_pocket_frac: f32,
    /// "Small" pocket cutoff in cells for cobweb fill (canonized).
    pub cobweb_max_pocket: u32,
    /// §1.2's "~2000" total; topped up with nook clusters because the cave
    /// network leaves almost no enclosed small pockets to fill.
    pub cobweb_target: u32,
    // Pass 12.
    pub spawn_search_radius: u32,
    pub spawn_flat_width: u32,
}

impl GenParams {
    /// §1 parameters scaled from the 4200×1200 baselines: rows scale with
    /// height, feature counts with area, noise wavelengths with width.
    /// Feature *sizes* (worm strength/steps, tree heights, footprints) don't
    /// scale — a cave is a cave in any world.
    pub fn new(width: u32, height: u32) -> GenParams {
        let rh = height as f64 / BASE_HEIGHT as f64;
        let rw = width as f64 / BASE_WIDTH as f64;
        let ra = rh * rw;
        let row = |r: u32| -> u32 { ((r as f64 * rh).round() as u32).min(height) };
        let rows = |a: u32, b_inclusive: u32| -> (u32, u32) {
            (row(a), (row(b_inclusive) + 1).min(height))
        };
        let count = |c: u32| -> u32 { ((c as f64 * ra).round() as u32).max(1) };

        GenParams {
            width,
            height,
            surface_min: row(220),
            surface_baseline: row(280),
            surface_max: row(340),
            octaves: [
                ((120.0 * rw) as f32, (40.0 * rh) as f32),
                ((40.0 * rw) as f32, (15.0 * rh) as f32),
                ((10.0 * rw) as f32, (5.0 * rh) as f32),
            ],
            dirt_to_stone: row(450),
            stone_to_ash: row(1000),
            stone_blobs: count(400),
            dirt_blobs: count(300),
            blob_strength: (6.0, 14.0),
            blob_steps: (6, 12),
            clay_blobs: count(300),
            clay_rows: (row(250), row(500)),
            clay_strength: (4.0, 9.0),
            clay_steps: (4, 8),
            surface_worms: count(80),
            surface_worm_strength: (4.0, 8.0),
            surface_worm_steps: (15, 30),
            surface_worm_bias_y: 0.3,
            cavern_worms: count(250),
            cavern_worm_rows: (row(450), row(980)),
            cavern_worm_strength: (10.0, 22.0),
            cavern_worm_steps: (60, 100),
            hell_ceiling_rows: (row(1020), row(1050)),
            hell_floor_rows: (row(1060), row(1140)),
            hell_lava_row: row(1100),
            hell_noise_wavelength: ((70.0 * rw) as f32).max(8.0),
            smoothing_passes: 3,
            ores: vec![
                OreSpec {
                    tile: TileId::CopperOre,
                    veins: count(600),
                    strength: (3.0, 6.0),
                    steps: (4, 8),
                    rows: rows(250, 700),
                    replaces: ores::ORE_REPLACES,
                },
                OreSpec {
                    tile: TileId::IronOre,
                    veins: count(450),
                    strength: (3.0, 6.0),
                    steps: (4, 8),
                    rows: rows(300, 850),
                    replaces: ores::ORE_REPLACES,
                },
                OreSpec {
                    tile: TileId::SilverOre,
                    veins: count(300),
                    strength: (3.0, 5.0),
                    steps: (4, 7),
                    rows: rows(450, 1000),
                    replaces: ores::ORE_REPLACES,
                },
                OreSpec {
                    tile: TileId::GoldOre,
                    veins: count(200),
                    strength: (3.0, 5.0),
                    steps: (4, 7),
                    rows: rows(600, 1000),
                    replaces: ores::ORE_REPLACES,
                },
                OreSpec {
                    tile: TileId::Hellstone,
                    veins: count(150),
                    strength: (3.0, 5.0),
                    steps: (4, 6),
                    rows: rows(1020, 1190),
                    replaces: ores::HELLSTONE_REPLACES,
                },
            ],
            water_rows: (row(450), row(800)),
            water_chance: 0.35,
            lava_rows: (row(800), row(1000)),
            lava_chance: 0.30,
            fill_frac: (0.25, 0.75),
            sand_patches: count(10).max(2),
            sand_min_spawn_dist: ((150.0 * rw).round() as u32).max(40),
            sand_strength: (15.0, 30.0),
            sand_steps: (10, 20),
            sand_max_depth: 25,
            tree_gap: (5, 15),
            tree_height: (7, 16),
            tree_min_separation: 4,
            mushroom_per_tiles: 40,
            underground_chests: count(150),
            underground_chest_rows: (row(350), row(1000)),
            chest_spacing: 25,
            surface_chests: count(20),
            ruins: count(10),
            ruin_size: (10, 8),
            altars: count(30),
            altar_rows: (row(500), row(1000)),
            life_crystals: count(100),
            life_crystal_rows: (row(450), row(1000)),
            pots: count(600),
            cobweb_rows: (row(450), row(1000)),
            cobweb_pocket_frac: 0.10,
            cobweb_max_pocket: 60,
            cobweb_target: count(2000),
            spawn_search_radius: 20,
            spawn_flat_width: 10,
        }
    }
}

/// Generates the standard 4200×1200 world (DESIGN §0).
pub fn generate(seed: u64) -> World {
    generate_with_size(seed, WORLD_WIDTH, WORLD_HEIGHT)
}

/// Generates a world of an arbitrary (scaled) size. Sizes below ~300×300
/// leave no room for the layer structure and panic.
pub fn generate_with_size(seed: u64, width: u32, height: u32) -> World {
    generate_with_progress(seed, width, height, |_, _| {})
}

/// [`generate_with_size`] with a per-pass progress callback
/// `(pass_number_1_based, pass_name)`, called before each pass runs.
pub fn generate_with_progress(
    seed: u64,
    width: u32,
    height: u32,
    mut progress: impl FnMut(usize, &'static str),
) -> World {
    assert!(
        width >= 300 && height >= 300,
        "world too small for the §1 layer structure"
    );
    let params = GenParams::new(width, height);
    let mut world = World::new(width, height);
    // One independent RNG stream per pass: editing one pass never reshuffles
    // the RNG of the others, keeping seeds comparable across versions.
    let stream = |pass: u64| Pcg32::with_stream(seed, pass);
    let mut report = |pass: usize| progress(pass, PASS_NAMES[pass - 1]);

    report(1);
    let surface = terrain::heightmap(&params, &mut stream(1));
    terrain::fill_base(&mut world, &params, &surface);

    report(2);
    terrain::stone_dirt_blobs(&mut world, &params, &mut stream(2), &surface);

    report(3);
    terrain::clay(&mut world, &params, &mut stream(3));

    report(4);
    let mut rng4 = stream(4);
    caves::surface_caves(&mut world, &params, &mut rng4, &surface);
    caves::cavern_caves(&mut world, &params, &mut rng4);
    caves::underworld_band(&mut world, &params, &mut rng4);

    report(5);
    caves::smooth(&mut world, &params);

    report(6);
    ores::ore_veins(&mut world, &params, &mut stream(6));

    report(7);
    let pockets = fluids_fill::fill_and_settle(&mut world, &params, &mut stream(7));

    report(8);
    flora::sand_patches(&mut world, &params, &mut stream(8), &surface);

    report(9);
    flora::grass_and_flora(&mut world, &params, &mut stream(9));

    // Pass 10 (obsidian at water–lava contacts) happens inside the §3
    // automaton whenever liquids touch; pass 7's settle already applied it.
    // Re-settling here is a cheap no-op guarantee that later passes left the
    // fluids at equilibrium.
    report(10);
    crate::sim::fluids::settle(&mut world);

    report(11);
    structures::place_structures(&mut world, &params, &mut stream(11), &pockets);

    report(12);
    spawn::pick_spawn(&mut world, &params);

    world
}
