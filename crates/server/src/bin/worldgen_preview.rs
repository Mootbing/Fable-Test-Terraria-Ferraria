//! Dev tool: renders a generated world to a PNG, 1 px per tile, so humans
//! can eyeball generation quality.
//!
//! Usage:
//!   cargo run -p ferraria-server --bin worldgen_preview -- \
//!       <seed> <out.png> [downscale] [crop=x,y,w,h]
//!
//! With the optional integer `downscale`, an additional `<out>.small.png`
//! is written at 1/N size (nearest neighbor) for quick viewing. Each
//! `crop=x,y,w,h` argument writes a full-resolution `<out>.crop<N>.png` of
//! that tile region (zooming into the surface, an ore band, ...).

use std::time::Instant;

use ferraria_shared::tiles::{LiquidKind, TileId, WallId};
use ferraria_shared::world::World;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let usage = "usage: worldgen_preview <seed> <out.png> [downscale] [crop=x,y,w,h ...]";
    let seed: u64 = args.next().ok_or_else(|| anyhow::anyhow!(usage))?.parse()?;
    let out = args.next().ok_or_else(|| anyhow::anyhow!(usage))?;
    let mut downscale: Option<u32> = None;
    let mut crops: Vec<[u32; 4]> = Vec::new();
    for arg in args {
        if let Some(spec) = arg.strip_prefix("crop=") {
            let parts: Vec<u32> = spec.split(',').map(str::parse).collect::<Result<_, _>>()?;
            anyhow::ensure!(parts.len() == 4, "crop wants x,y,w,h");
            crops.push([parts[0], parts[1], parts[2], parts[3]]);
        } else {
            downscale = Some(arg.parse()?);
        }
    }

    let t0 = Instant::now();
    let world = ferraria_server::worldgen::generate_with_progress(
        seed,
        ferraria_shared::world::WORLD_WIDTH,
        ferraria_shared::world::WORLD_HEIGHT,
        |i, name| eprintln!("pass {i:2}: {name}"),
    );
    eprintln!("generated seed {seed} in {:.2?}", t0.elapsed());
    eprintln!("spawn at {:?}, {} chests", world.spawn, world.chests.len());

    let img = render(&world);
    img.save(&out)?;
    eprintln!("wrote {out} ({}x{})", world.width, world.height);

    if let Some(n) = downscale.filter(|&n| n > 1) {
        let small = image::imageops::resize(
            &img,
            world.width / n,
            world.height / n,
            image::imageops::FilterType::Nearest,
        );
        let small_path = format!("{}.small.png", out.trim_end_matches(".png"));
        small.save(&small_path)?;
        eprintln!("wrote {small_path}");
    }
    for (i, &[x, y, w, h]) in crops.iter().enumerate() {
        let crop = image::imageops::crop_imm(&img, x, y, w, h).to_image();
        let crop_path = format!("{}.crop{i}.png", out.trim_end_matches(".png"));
        crop.save(&crop_path)?;
        eprintln!("wrote {crop_path} ({x},{y} {w}x{h})");
    }
    Ok(())
}

fn render(world: &World) -> image::RgbImage {
    let hell_top = (world.height as u64 * 1000 / 1200) as u32;
    image::RgbImage::from_fn(world.width, world.height, |x, y| {
        image::Rgb(pixel(world, x, y, hell_top))
    })
}

/// Color per cell: liquids first, then the foreground tile, then walls/sky.
fn pixel(world: &World, x: u32, y: u32, hell_top: u32) -> [u8; 3] {
    let t = world.tile(x, y);
    if t.id == TileId::Air {
        if let Some(kind) = t.liquid.kind() {
            let lv = t.liquid.level() as u16;
            return match kind {
                LiquidKind::Water => [20, (40 + 10 * lv) as u8, (140 + 12 * lv) as u8],
                LiquidKind::Lava => [(180 + 8 * lv) as u8, (60 + 4 * lv) as u8, 10],
            };
        }
        return match t.wall {
            WallId::Dirt => [62, 44, 30],
            WallId::Stone => [42, 42, 50],
            WallId::Wood => [80, 62, 40],
            WallId::Air if y >= hell_top => [28, 12, 12],
            WallId::Air => [135, 206, 235], // sky
        };
    }
    match t.id {
        TileId::Air => unreachable!(),
        TileId::Dirt => [120, 80, 48],
        TileId::Stone => [115, 115, 122],
        TileId::Grass => [60, 170, 60],
        TileId::Sand => [212, 192, 120],
        TileId::Clay => [168, 105, 76],
        TileId::WoodPlank => [150, 112, 70],
        TileId::CopperOre => [205, 115, 60],
        TileId::IronOre => [195, 165, 145],
        TileId::SilverOre => [215, 215, 225],
        TileId::GoldOre => [235, 195, 60],
        TileId::Hellstone => [220, 70, 40],
        TileId::Obsidian => [80, 40, 120],
        TileId::Ash => [75, 70, 78],
        TileId::StoneBrick => [135, 135, 140],
        TileId::EmberBrick => [150, 55, 45],
        TileId::Torch => [255, 220, 120],
        TileId::Platform => [170, 130, 80],
        TileId::Door => [140, 100, 60],
        TileId::Chest => [255, 170, 30],
        TileId::Workbench => [150, 110, 70],
        TileId::Furnace => [120, 90, 80],
        TileId::Anvil => [90, 90, 95],
        TileId::InfernalForge => [255, 120, 40],
        TileId::RitualAltar => [190, 60, 200],
        TileId::Table | TileId::Chair | TileId::Bed => [150, 110, 70],
        TileId::Pot => [185, 130, 85],
        TileId::LifeCrystal => [255, 60, 130],
        TileId::Cobweb => [225, 225, 225],
        TileId::Sapling => [95, 175, 95],
        TileId::TreeTrunk => {
            if t.state == ferraria_shared::tiles::state::TREE_SEGMENT_TOP {
                [40, 130, 45]
            } else {
                [105, 78, 48]
            }
        }
    }
}
