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
    }
}
