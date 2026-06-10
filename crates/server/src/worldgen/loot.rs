//! Worldgen loot tables (DESIGN §2.3) as static data, plus the chest-rolling
//! helper. The pot table also lives here so the live sim (pots roll on
//! break) reads the same numbers.

use ferraria_shared::items::{InvSlot, ItemId};
use ferraria_shared::rng::Pcg32;
use ferraria_shared::world::CHEST_SLOTS;

/// `min..=max` of `item`; when `alt` is set, the item is a 50/50 pick
/// between `item` and `alt` ("Silver or Gold Bar ×3–8").
#[derive(Debug, Clone, Copy)]
pub struct LootRoll {
    pub item: ItemId,
    pub alt: Option<ItemId>,
    pub min: u16,
    pub max: u16,
}

impl LootRoll {
    const fn of(item: ItemId, min: u16, max: u16) -> LootRoll {
        LootRoll {
            item,
            alt: None,
            min,
            max,
        }
    }

    const fn either(item: ItemId, alt: ItemId, min: u16, max: u16) -> LootRoll {
        LootRoll {
            item,
            alt: Some(alt),
            min,
            max,
        }
    }
}

/// A chest's primary entries (one is picked by weight) and extras (each
/// rolled independently at `chance`).
#[derive(Debug, Clone, Copy)]
pub struct ChestLootTable {
    pub primary: &'static [(LootRoll, u32)],
    pub extras: &'static [(LootRoll, f32)],
}

/// §2.3 surface chest.
pub const SURFACE_CHEST: ChestLootTable = ChestLootTable {
    primary: &[
        (LootRoll::of(ItemId::GustJar, 1, 1), 25),
        (LootRoll::of(ItemId::SwiftBoots, 1, 1), 25),
        (LootRoll::of(ItemId::BandOfVigor, 1, 1), 25),
        (LootRoll::of(ItemId::WoodenArrow, 30, 30), 25),
    ],
    extras: &[
        (LootRoll::of(ItemId::SilverCoin, 1, 10), 1.0),
        (LootRoll::of(ItemId::Torch, 3, 10), 1.0),
        (LootRoll::of(ItemId::LesserHealingPotion, 1, 3), 0.5),
    ],
};

/// §2.3 underground chest.
pub const UNDERGROUND_CHEST: ChestLootTable = ChestLootTable {
    primary: &[
        (LootRoll::of(ItemId::SwiftBoots, 1, 1), 20),
        (LootRoll::of(ItemId::GustJar, 1, 1), 20),
        (LootRoll::of(ItemId::LuckyCharm, 1, 1), 20),
        (LootRoll::of(ItemId::WarpMirror, 1, 1), 15),
        (LootRoll::of(ItemId::BandOfVigor, 1, 1), 25),
    ],
    extras: &[
        (LootRoll::of(ItemId::SilverCoin, 5, 20), 1.0),
        (
            LootRoll::either(ItemId::SilverBar, ItemId::GoldBar, 3, 8),
            0.5,
        ),
        (LootRoll::of(ItemId::LesserHealingPotion, 2, 5), 1.0),
        (LootRoll::of(ItemId::Torch, 5, 15), 1.0),
    ],
};

/// §2.3 underworld chest.
pub const UNDERWORLD_CHEST: ChestLootTable = ChestLootTable {
    primary: &[
        (LootRoll::of(ItemId::ObsidianCharm, 1, 1), 50),
        (LootRoll::of(ItemId::WarpMirror, 1, 1), 50),
    ],
    extras: &[
        (LootRoll::of(ItemId::GoldCoin, 1, 3), 1.0),
        (LootRoll::of(ItemId::Hellstone, 10, 20), 1.0),
        (LootRoll::of(ItemId::HealingPotion, 2, 5), 1.0),
        (LootRoll::of(ItemId::GoldBar, 5, 10), 0.5),
    ],
};

/// §2.3 pot loot, picked by weight when a pot breaks (live sim). The coin
/// roll is further scaled by depth: ×1 surface, ×2 cavern, ×4 underworld
/// ([`POT_COIN_MULT_SURFACE`] etc.).
pub const POT_LOOT: &[(LootRoll, u32)] = &[
    (LootRoll::of(ItemId::SilverCoin, 1, 10), 50),
    (LootRoll::of(ItemId::Torch, 3, 8), 20),
    (LootRoll::of(ItemId::LesserHealingPotion, 1, 1), 15),
    (LootRoll::of(ItemId::WoodenArrow, 10, 20), 10),
    (LootRoll::of(ItemId::Gel, 1, 4), 5),
];
pub const POT_COIN_MULT_SURFACE: u16 = 1;
pub const POT_COIN_MULT_CAVERN: u16 = 2;
pub const POT_COIN_MULT_UNDERWORLD: u16 = 4;

/// §2.3 pot coin depth multiplier for a pot at row `y` in a world of
/// `height` rows: ×1 above the cavern band, ×2 in it, ×4 in the underworld.
/// Band starts scale with world height exactly like `GenParams` scales rows
/// (450/1200 and 1000/1200 of the §1.1 baselines).
pub fn pot_coin_mult(y: u32, height: u32) -> u16 {
    let row = |r: u64| -> u32 { ((r * height as u64) / super::BASE_HEIGHT as u64) as u32 };
    if y >= row(1000) {
        POT_COIN_MULT_UNDERWORLD
    } else if y >= row(450) {
        POT_COIN_MULT_CAVERN
    } else {
        POT_COIN_MULT_SURFACE
    }
}

/// Rolls one pot break (§2.3): picks an entry by weight and scales the coin
/// roll by `coin_mult` (see [`pot_coin_mult`]).
pub fn roll_pot(rng: &mut Pcg32, coin_mult: u16) -> InvSlot {
    let weights: Vec<u32> = POT_LOOT.iter().map(|&(_, w)| w).collect();
    let i = rng
        .pick_weighted(&weights)
        .unwrap_or(0 /* table weights are non-zero; pinned by test */);
    let mut slot = roll(rng, &POT_LOOT[i].0);
    if slot.item == ItemId::SilverCoin {
        slot.count = (slot.count * coin_mult).min(slot.item.max_stack());
    }
    slot
}

fn roll(rng: &mut Pcg32, entry: &LootRoll) -> InvSlot {
    let item = match entry.alt {
        Some(alt) if rng.chance(0.5) => alt,
        _ => entry.item,
    };
    let count = rng.gen_range_u32(entry.min as u32..entry.max as u32 + 1) as u16;
    InvSlot::new(item, count)
}

/// Rolls a chest's contents (§2.3: 1 primary + all extras) into a fresh
/// [`CHEST_SLOTS`]-slot vec, primary first.
pub fn roll_chest(rng: &mut Pcg32, table: &ChestLootTable) -> Vec<Option<InvSlot>> {
    let mut slots = vec![None; CHEST_SLOTS];
    let mut next = 0;
    let weights: Vec<u32> = table.primary.iter().map(|&(_, w)| w).collect();
    if let Some(i) = rng.pick_weighted(&weights) {
        slots[next] = Some(roll(rng, &table.primary[i].0));
        next += 1;
    }
    for (entry, chance) in table.extras {
        if rng.chance(*chance) {
            slots[next] = Some(roll(rng, entry));
            next += 1;
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chest_rolls_have_primary_and_guaranteed_extras() {
        let mut rng = Pcg32::new(5);
        for _ in 0..200 {
            let slots = roll_chest(&mut rng, &UNDERGROUND_CHEST);
            assert_eq!(slots.len(), CHEST_SLOTS);
            let filled: Vec<_> = slots.iter().flatten().collect();
            // 1 primary + 3 guaranteed extras minimum.
            assert!(filled.len() >= 4, "only {} slots filled", filled.len());
            assert!(filled.iter().all(|s| s.count >= 1));
            // The coin extra is always present.
            assert!(filled.iter().any(|s| s.item == ItemId::SilverCoin));
            // Counts respect their ranges.
            for s in &filled {
                if s.item == ItemId::SilverCoin {
                    assert!((5..=20).contains(&s.count));
                }
                if s.item == ItemId::Torch {
                    assert!((5..=15).contains(&s.count));
                }
            }
        }
    }

    #[test]
    fn either_rolls_produce_both_options() {
        let mut rng = Pcg32::new(11);
        let entry = LootRoll::either(ItemId::SilverBar, ItemId::GoldBar, 3, 8);
        let mut silver = 0;
        let mut gold = 0;
        for _ in 0..400 {
            match roll(&mut rng, &entry).item {
                ItemId::SilverBar => silver += 1,
                ItemId::GoldBar => gold += 1,
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(silver > 100 && gold > 100, "{silver} vs {gold}");
    }

    #[test]
    fn pot_weights_total_100() {
        assert_eq!(POT_LOOT.iter().map(|&(_, w)| w).sum::<u32>(), 100);
        assert!(POT_LOOT.iter().all(|&(_, w)| w > 0));
    }

    #[test]
    fn pot_coin_mult_bands_scale_with_height() {
        // Full-size world: §1.1 rows directly.
        assert_eq!(pot_coin_mult(0, 1200), 1);
        assert_eq!(pot_coin_mult(449, 1200), 1);
        assert_eq!(pot_coin_mult(450, 1200), 2);
        assert_eq!(pot_coin_mult(999, 1200), 2);
        assert_eq!(pot_coin_mult(1000, 1200), 4);
        assert_eq!(pot_coin_mult(1199, 1200), 4);
        // Scaled world (300 rows): bands at 112/113 and 250.
        assert_eq!(pot_coin_mult(112, 300), 1);
        assert_eq!(pot_coin_mult(113, 300), 2);
        assert_eq!(pot_coin_mult(250, 300), 4);
    }

    #[test]
    fn pot_rolls_stay_within_spec_ranges() {
        // §2.3: 50% 1–10 SC (×depth), 20% 3–8 torches, 15% 1 lesser healing
        // potion, 10% 10–20 wooden arrows, 5% 1–4 gel.
        let mut rng = Pcg32::new(0xbeef);
        let mut seen = [0u32; 5];
        for _ in 0..2000 {
            let s = roll_pot(&mut rng, POT_COIN_MULT_CAVERN);
            match s.item {
                ItemId::SilverCoin => {
                    seen[0] += 1;
                    assert!((2..=20).contains(&s.count), "coins ×2: {}", s.count);
                    assert_eq!(s.count % 2, 0, "scaled coin count: {}", s.count);
                }
                ItemId::Torch => {
                    seen[1] += 1;
                    assert!((3..=8).contains(&s.count));
                }
                ItemId::LesserHealingPotion => {
                    seen[2] += 1;
                    assert_eq!(s.count, 1);
                }
                ItemId::WoodenArrow => {
                    seen[3] += 1;
                    assert!((10..=20).contains(&s.count));
                }
                ItemId::Gel => {
                    seen[4] += 1;
                    assert!((1..=4).contains(&s.count));
                }
                other => panic!("{other:?} is not in the pot table"),
            }
        }
        // Rough weight sanity: coins dominate, everything appears.
        assert!(seen.iter().all(|&n| n > 0), "{seen:?}");
        assert!(seen[0] > seen[1] && seen[1] > seen[4], "{seen:?}");
    }
}
