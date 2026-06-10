# FERRARIA — Game Design Specification

**Version 1.0 — Pre-Hardmode Vertical Slice.** Target: Rust, 60 ticks/sec fixed simulation, 16 px tiles. All physics below are in **tiles** and **seconds** unless noted (1 px/tick = 3.75 tiles/s). All mechanics are adapted from Terraria research (sources at end); all names are original or generic. Everything in this document is in scope and will be implemented.

## 0. Global Constants

| Constant | Value |
|---|---|
| Tick rate | 60 ticks/s |
| Tile size | 16 px |
| World size | 4200 × 1200 tiles |
| Gravity | 90 tiles/s² (0.4 px/tick²) |
| Terminal fall velocity | 37.5 tiles/s (10 px/tick) |
| Coin denominations | 100 Copper = 1 Silver; 100 Silver = 1 Gold; 100 Gold = 1 Platinum |
| Damage formula (all combat) | `damage_dealt = max(1, attack − floor(defense / 2))` |
| Crit | 4% base chance, ×2 damage |
| Hit immunity (i-frames) | Player: 0.67 s (40 ticks) after taking a hit. Enemies: 0.17 s (10 ticks) per damage source |
| Max live enemies (global) | 200 |
| Stack sizes | Blocks/materials/ammo 999; potions 30; coins 999/slot; tools/weapons/armor/accessories 1 |

---

## 1. World Generation

Generation is seeded (u64 seed) and runs in the fixed pass order below. Row 0 = top of world.

### 1.1 Vertical layers

| Layer | Rows | Contents |
|---|---|---|
| Sky | 0 – ~219 | Air only (tall hills may intrude to row ~220) |
| Surface | ~220 – 340 (heightmap) | Grass, trees, sand patches, surface caves |
| Underground (dirt) | surface – 450 | Dirt with stone blobs, clay, copper/iron, shallow caves |
| Caverns (stone) | 450 – 999 | Stone with dirt blobs, all ores, water pockets (450–799), lava pockets (800+), cobwebs, chests, Life Crystals, Ritual Altars |
| Underworld | 1000 – 1199 | Ash, lava lakes, Hellstone, ember-brick ruins with Infernal Forges |

### 1.2 Pass order and parameters

1. **Surface heightmap** — 1D layered value noise, 3 octaves: wavelengths 120/40/10 tiles, amplitudes 40/15/5 tiles, baseline row 280. Clamp to rows 220–340. Fill below heightmap with dirt to row 450, stone from 450 to 999, ash from 1000 down.
2. **Stone/dirt blobs** — 400 stone blobs in dirt layer, 300 dirt blobs in stone layer. Blob = TileRunner (below) with strength 6–14, steps 6–12, painting instead of carving.
3. **Clay** — 300 blobs, rows 250–500, strength 4–9, steps 4–8.
4. **Caves — TileRunner random walk** (this is what Terraria actually uses, per decompiled source — a "drunken walk" brush, *not* cellular automata from noise; CA is only used to smooth). Each worm: position `p`, direction `d` (unit vector, random initial), strength `S` (carve radius ≈ √S tiles), steps `N`. Per step: carve circle of radius √S at `p`; `p += d × √S × 0.5`; `d.x += rand(−0.5, 0.5)`, `d.y += rand(−0.5, 0.5)`, clamp |d| components to ±1; 5% chance per step to branch a child worm with S×0.6, N×0.5.
   - **Surface caves:** 80 worms, start on heightmap, S = 4–8, N = 15–30, bias d.y +0.3 (dig down).
   - **Cavern worms:** 250 worms, start rows 450–980, S = 10–22, N = 60–100, no bias.
   - **Underworld caverns:** carve a horizontal lava-lake band: 1D noise floor between rows 1060–1140; everything between ash ceiling and floor is open; fill open tiles below row 1100 with lava.
5. **Smoothing** — 3 passes of cellular-automata majority rule over all carved regions: a solid tile with ≤3 solid neighbors (8-neighborhood) becomes air; an air tile with ≥6 solid neighbors becomes solid. (Rounds off the diamond-shaped TileRunner marks.)
6. **Ore veins** — TileRunner painting ore into stone/dirt/ash:

| Ore | Veins | Strength | Steps | Row range |
|---|---|---|---|---|
| Copper | 600 | 3–6 | 4–8 | 250–700 |
| Iron | 450 | 3–6 | 4–8 | 300–850 |
| Silver | 300 | 3–5 | 4–7 | 450–1000 |
| Gold | 200 | 3–5 | 4–7 | 600–1000 |
| Hellstone | 150 | 3–5 | 4–6 | 1020–1190 (only replaces ash) |

7. **Fluids** — 35% of enclosed cave pockets in rows 450–799 get water filled to a random level (25–75% of pocket height); pockets in rows 800–999 get lava at 30% chance. Run fluid settling (Section 3) to equilibrium before finishing.
8. **Sand** — 10 surface patches at random heightmap positions ≥150 tiles from spawn: TileRunner paint, S = 15–30, N = 10–20, clamped to ≤25 rows below surface.
9. **Grass & flora** — every dirt tile with air above becomes grass. On grass: tree every 5–15 tiles (height 7–16 trunk segments, no two trees within 4 tiles), 1 mushroom forage plant per ~40 surface tiles.
10. **Obsidian** — wherever settled water and lava became adjacent during pass 7 (see 3.2).
11. **Structures & placements:**

| Feature | Count | Placement rule |
|---|---|---|
| Underground chests | 150 | On cave floors, rows 350–999; ≥25 tiles apart; wooden chest sprite |
| Surface chests | 20 | On surface or in surface caves |
| Underworld ruins | 10 | Ember-brick shells (10×8) on underworld floor; each contains 1 Infernal Forge + 1 underworld chest |
| Ritual Altars | 30 | Cavern floors, rows 500–999 |
| Life Crystals | 100 | Embedded in solid stone adjacent to caves, rows 450–999 |
| Pots | 600 | Cave floors, all layers |
| Cobwebs | ~2000 | Fill 10% of small cave pockets, rows 450–999 |

12. **Spawn point** — world center column ± 20 tiles; pick the column whose surface is flattest (min height variance over 10 tiles); flatten a 10-tile platform; spawn = 1 tile above it.

---

## 2. Tiles (35 types + 2 fluids)

**Mining model:** every tile has 100 break-points. Each tool swing deals `tool_power × material_multiplier` points. A tile below its minimum power threshold takes **zero** damage. Accumulated damage on a tile resets after 5 s without hits. Furniture (✦) breaks in 1 hit from any pickaxe and drops its item; trees/saplings require an axe; walls require a hammer.

Multipliers: **Soft ×2** | **Medium ×1** | **Hard ×0.75** | **Very hard ×0.5**.

| ID | Tile | Solid | Tool | Mult / Min power | Drops | Light emit (0–32) | Notes |
|---|---|---|---|---|---|---|---|
| 1 | Dirt | Yes | Pick | Soft / 0 | Dirt | 0 | |
| 2 | Stone | Yes | Pick | Medium / 0 | Stone | 0 | |
| 3 | Grass | Yes | Pick | Soft / 0 | Dirt | 0 | Spreads to adjacent air-exposed dirt (1/600 chance/tile-update); dies if covered |
| 4 | Sand | Yes | Pick | Soft / 0 | Sand | 0 | Falls if no solid tile below (becomes falling entity, 37.5 t/s cap) |
| 5 | Clay | Yes | Pick | Soft / 0 | Clay | 0 | |
| 6 | Wood Plank | Yes | Pick | Medium / 0 | Wood Plank | 0 | Crafted block |
| 7 | Copper Ore | Yes | Pick | Medium / 0 | Copper Ore | 0 | |
| 8 | Iron Ore | Yes | Pick | Medium / 0 | Iron Ore | 0 | |
| 9 | Silver Ore | Yes | Pick | Hard / 0 | Silver Ore | 0 | |
| 10 | Gold Ore | Yes | Pick | Hard / 0 | Gold Ore | 0 | |
| 11 | Hellstone | Yes | Pick | Very hard / **55** | Hellstone | 13 | Touching it while not fire-immune: Burning 2 s |
| 12 | Obsidian | Yes | Pick | Very hard / **55** | Obsidian | 0 | |
| 13 | Ash | Yes | Pick | Soft / 0 | Ash | 0 | |
| 14 | Stone Brick | Yes | Pick | Medium / 0 | Stone Brick | 0 | Craftable, decorative |
| 15 | Ember Brick | Yes | Pick | Hard / 0 | Ember Brick | 0 | Underworld ruins |
| 16 | Torch ✦ | No | Any | — | Torch | **28** | Attaches to solid tile/wall; extinguishes in water |
| 17 | Platform | Semi | Pick | Soft / 0 | Platform | 0 | Solid from above only; press Down+Jump to drop through |
| 18 | Door ✦ | Closed: yes | Pick | — | Door | 0 | 1×3; right-click toggles; needs solid tile above & below |
| 19 | Chest ✦ | No | Pick | — | Chest (+spills contents) | 0 | 2×2, 40 slots; can't break while non-empty |
| 20 | Workbench ✦ | No | Pick | — | Workbench | 0 | 2×1 crafting station; counts as table for housing |
| 21 | Furnace ✦ | No | Pick | — | Furnace | 6 | 3×2 crafting station |
| 22 | Anvil ✦ | No | Pick | — | Anvil | 0 | 2×1 crafting station |
| 23 | Infernal Forge ✦ | No | Pick (min 55) | — | Infernal Forge | 10 | 3×2; smelts Ember Bars; world-gen only (relocatable) |
| 24 | Ritual Altar | No | **Unbreakable** | — | — | 8 | Crafting station for boss summons; hammering it deals 50% of the striker's current HP, altar unaffected |
| 25 | Table ✦ | No | Pick | — | Table | 0 | 3×2; housing flat-surface |
| 26 | Chair ✦ | No | Pick | — | Chair | 0 | 1×2; housing comfort |
| 27 | Bed ✦ | No | Pick | — | Bed | 0 | 4×2; right-click sets personal spawn; housing comfort |
| 28 | Pot | No | Any (1 hit) | — | Loot (2.3) | 0 | |
| 29 | Life Crystal | No | Pick (1 hit) | — | Life Crystal item | 6 | |
| 30 | Cobweb | No | Any (1 hit) | — | Cobweb (drops 1) | 0 | Entities inside: velocity clamped to 1.5 t/s |
| 31 | Sapling | No | Axe (1 hit) | — | — | 0 | Grows to tree after 5–10 min if 7+ air tiles above |
| 32 | Tree trunk | No (background) | Axe | Medium ×1 / — | 10 wood + 25% 1 acorn **per segment** | 0 | Chopping any segment fells everything above it; drops fall as items |
| 33 | Dirt Wall | Wall | Hammer | Soft / 0 | Dirt Wall (only if player-placed; natural walls drop nothing) | 0 | |
| 34 | Stone Wall | Wall | Hammer | Medium / 0 | Stone Wall (same rule) | 0 | |
| 35 | Wood Wall | Wall | Hammer | Soft / 0 | Wood Wall | 0 | Craftable; counts as "safe" wall for housing |

### 2.3 Loot tables

**Pot:** 50% coins (1–10 SC scaled by depth: ×1 surface, ×2 cavern, ×4 underworld), 20% 3–8 torches, 15% 1 Lesser Healing Potion, 10% 10–20 wooden arrows, 5% 1–4 gel.

**Chests** roll 1 primary + all extras:

| Chest | Primary (one of) | Extras (each independent) |
|---|---|---|
| Surface | Gust Jar 25%, Swift Boots 25%, Band of Vigor 25%, 30 Wooden Arrows 25% | Coins 1–10 SC (100%), Torches 3–10 (100%), Lesser Healing Potion ×1–3 (50%) |
| Underground | Swift Boots 20%, Gust Jar 20%, Lucky Charm 20%, Warp Mirror 15%, Band of Vigor 25% | Coins 5–20 SC (100%), Silver or Gold Bar ×3–8 (50%), Lesser Healing Potion ×2–5 (100%), Torches 5–15 (100%) |
| Underworld | Obsidian Charm 50%, Warp Mirror 50% | Coins 1–3 GC (100%), Hellstone ×10–20 (100%), Healing Potion ×2–5 (100%), Gold Bar ×5–10 (50%) |

---

## 3. Fluids (Water, Lava)

- Stored per cell: type + level 1–8. Cellular automaton: water cells update every 2 ticks, lava every 5 ticks (lava is sluggish). Rules per update: (1) flow down into non-solid cell below until full; (2) else equalize levels with horizontal neighbors (move 1 level toward the lower neighbor); level-1 puddles on flat ground evaporate after 60 s.
- **Water effects on entities:** horizontal speed ×0.5, gravity ×0.4, terminal velocity ×0.5; jump becomes a swim impulse (repeatable, 12 tiles/s). Torches can't be placed; breath meter runs (Section 8).
- **Lava effects:** as water for physics, plus on contact: 50 damage + Burning debuff (2 dmg/s) for 7 s. Item drops that touch lava are destroyed (except Obsidian, Hellstone, Ember-tier items).
- **3.2 Obsidian creation:** whenever a water cell and lava cell become adjacent (or one flows into the other), the **lava** cell converts to an Obsidian tile and the water cell loses 1 level.

---

## 4. Items, Tools, Weapons, Armor, Crafting

### 4.1 Tool tiers

Pickaxe/axe power numbers feed the mining model (Section 2). Swing interval = use time.

| Tier | Pick power | Pick use (s) | Axe power | Sword dmg / use (s) | Notes |
|---|---|---|---|---|---|
| Wood | 25 | 0.30 | 25 | 7 / 0.42 | Cannot mine Obsidian/Hellstone |
| Copper | 35 | 0.25 | 35 | 9 / 0.40 | |
| Iron | 40 | 0.23 | 40 | 12 / 0.38 | |
| Silver | 45 | 0.22 | 45 | 14 / 0.37 | |
| Gold | 55 | 0.20 | 55 | 16 / 0.35 | First pick that mines Obsidian & Hellstone (we deliberately set Hellstone's threshold to 55, not Terraria's 65, since Ferraria has no demonite-equivalent middle tier) |
| Ember (hell) | 100 | 0.17 | 100 | **Ember Blade** 36 / 0.55 | Ember Blade has 10% chance to inflict Burning 3 s |

Pickaxes also deal melee damage: 4/5/6/7/8/12 by tier, knockback 2 t/s. Axes: damage 5–14 by tier. Swords knockback 5 t/s. Melee hitbox: 3×3 tiles arc in facing direction for swing duration.

**Bows** (damage adds to arrow damage; arrows fly at 35 t/s, gravity ×0.35, despawn after 5 s; 50% chance to recover arrow from terrain):

| Bow | Damage | Use (s) |
|---|---|---|
| Wooden Bow | 4 | 0.50 |
| Iron Bow | 8 | 0.47 |
| Gold Bow | 11 | 0.45 |
| Cinderbow (hell) | 29 | 0.40 (wooden arrows fired become flaming arrows) |

Arrows: **Wooden Arrow** 5 dmg; **Flaming Arrow** 7 dmg + 33% Burning 3 s.

**Hammers** (wall removal): Wood Hammer (power 25, 0.33 s), Iron Hammer (power 55, 0.28 s).

### 4.2 Armor

Per-piece defense (helmet/chest/greaves), set bonus applies when all 3 worn.

| Set | Helm | Chest | Greaves | Set bonus | Cost per piece |
|---|---|---|---|---|---|
| Wood | 1 | 1 | 0 | +1 defense | 20 / 30 / 25 wood |
| Copper | 1 | 2 | 1 | +2 defense | 15 / 25 / 20 copper bars |
| Iron | 2 | 3 | 2 | +2 defense | 15 / 25 / 20 iron bars |
| Silver | 3 | 4 | 3 | +3 defense | 15 / 25 / 20 silver bars |
| Gold | 4 | 5 | 4 | +3 defense | 15 / 25 / 20 gold bars |
| Ember | 8 | 9 | 8 | +10% melee damage, immune to Burning | 10 / 20 / 15 ember bars |

### 4.3 Accessories (3 slots; effects stack across different accessories, duplicates don't stack)

| Accessory | Source | Effect |
|---|---|---|
| Swift Boots | Chests | +25% max run speed |
| Gust Jar | Chests | Double jump (second jump = 75% height) |
| Lucky Charm | Chests | No fall damage |
| Band of Vigor | Chests | +0.5 HP/s passive regen |
| Obsidian Charm | Underworld chests | Immune to Burning; lava deals half damage |
| Royal Gel Charm | Slime Monarch | Slimes (green/blue) never aggro |
| Bloodshot Lens | The Watcher | +10% all damage |
| Skull Charm | The Bone Warden | +4 defense |

**Usable specials:** Warp Mirror (use: 1 s channel, teleport to spawn point). Acorn (plant sapling on grass). Mining Helmet (head armor, 0 def, emits light 20 at player).

### 4.4 Crafting stations & recipes (~70)

Stations: **Hands, Workbench, Furnace, Anvil, Infernal Forge, Ritual Altar, placed Bottle** (bottle on a table/workbench enables potions). Crafting UI lists all recipes whose station is within 4 tiles and ingredients are in inventory.

| # | Output (qty) | Ingredients | Station |
|---|---|---|---|
| 1 | Workbench | 10 Wood | Hands |
| 2 | Torch (3) | 1 Wood + 1 Gel | Hands |
| 3 | Wood Plank block (1) | 1 Wood | Hands |
| 4 | Platform (2) | 1 Wood | Workbench |
| 5 | Door | 6 Wood | Workbench |
| 6 | Table | 8 Wood | Workbench |
| 7 | Chair | 4 Wood | Workbench |
| 8 | Chest | 8 Wood + 2 Iron Bars | Workbench |
| 9 | Bed | 15 Wood + 20 Cobwebs | Workbench |
| 10 | Wood Wall (4) | 1 Wood | Workbench |
| 11 | Furnace | 20 Stone + 4 Wood + 3 Torches | Workbench |
| 12 | Stone Brick (2) | 2 Stone | Furnace |
| 13 | Copper Bar | 3 Copper Ore | Furnace |
| 14 | Iron Bar | 3 Iron Ore | Furnace |
| 15 | Silver Bar | 4 Silver Ore | Furnace |
| 16 | Gold Bar | 4 Gold Ore | Furnace |
| 17 | Ember Bar | 3 Hellstone + 1 Obsidian | **Infernal Forge** |
| 18 | Glass | 2 Sand | Furnace |
| 19 | Bottle (2) | 1 Glass | Furnace |
| 20 | Anvil | 5 Iron Bars | Workbench |
| 21–26 | Pickaxes: Wood (12 wood, WB); Copper (8 bars+4 wood); Iron (10+4); Silver (10+4); Gold (10+4); Ember Pick (20 Ember Bars) | | Anvil unless noted |
| 27–32 | Axes: Wood (9 wood, WB); Copper/Iron/Silver/Gold (9 bars + 3 wood); Ember Axe (15 Ember Bars) | | Anvil |
| 33–38 | Swords: Wood (7 wood, WB); Copper/Iron/Silver/Gold (8 bars); Ember Blade (20 Ember Bars) | | Anvil |
| 39–42 | Bows: Wooden (10 wood, WB); Iron (7 iron bars); Gold (7 gold bars); Cinderbow (15 Ember Bars) | | Anvil |
| 43 | Wood Hammer | 8 Wood | Workbench |
| 44 | Iron Hammer | 8 Iron Bars | Anvil |
| 45 | Wooden Arrow (25) | 1 Wood + 1 Stone | Workbench |
| 46 | Flaming Arrow (10) | 10 Wooden Arrows + 1 Torch | Hands |
| 47 | Lesser Healing Potion (2) | 2 Bottles + 2 Gel + 1 Mushroom | Placed Bottle |
| 48 | Healing Potion | 2 Lesser Healing Potions + 1 Gel | Placed Bottle |
| 49–66 | Armor pieces per table 4.2 (18 recipes; wood set at Workbench, metal sets at Anvil, Ember set at Anvil) | | |
| 67 | Gold Crown | 5 Gold Bars | Anvil |
| 68 | **Gel Crown** (summons Slime Monarch) | 1 Gold Crown + 20 Gel | Ritual Altar |
| 69 | **Watcher's Iris** (summons The Watcher) | 6 Lenses | Ritual Altar |
| 70 | **Cursed Effigy** (summons The Bone Warden) | 30 Bones + 10 Gold Bars | Ritual Altar |

Potions: Lesser Healing Potion heals 50; Healing Potion heals 100. Both inflict **Potion Sickness** 60 s (cannot use another healing item).

---

## 5. Enemies & Spawning

### 5.1 Enemy roster

| Enemy | HP | Contact dmg | Def | KB resist | AI | Spawns | Drops |
|---|---|---|---|---|---|---|---|
| Green Slime | 14 | 6 | 0 | −20% (extra KB) | Slime | Surface, day+night | 1–2 Gel (100%), 5 CC |
| Blue Slime | 25 | 7 | 2 | 0% | Slime | Surface day/night + underground any time | 1–2 Gel (100%), 25 CC |
| Zombie | 45 | 14 | 6 | 50% | Fighter | Surface, night only | 60 CC; 50% 1 Wood, 2% Zombie Arm (10 dmg sword) |
| Demon Eye | 60 | 18 | 2 | 20% | Flier (bouncer) | Surface, night only | 75 CC; Lens 33% |
| Cave Bat | 16 | 13 | 2 | 20% | Flier (erratic) | Underground + caverns | 90 CC |
| Skeleton | 60 | 20 | 8 | 50% | Fighter | Caverns | 1 SC; Bone ×1–3 (50%) |
| Lava Slime | 50 | 15 | 10 | 0% | Slime (lava-proof) | Underworld | 1 SC 20 CC; no gel |
| Ash Demon | 120 | 32 | 8 | 20% | Swooper + caster | Underworld | 3 SC; Void Sickle weapon 2.86% |
| Watchling (boss minion) | 8 | 12 | 0 | 0% | Flier (straight) | Spawned by The Watcher | nothing |

Slimes are **passive** on the surface during the day until damaged; always hostile at night or underground. Coin drops vary ×(0.8–1.2). All enemies despawn at >168 tiles horizontal or >94 vertical from the nearest player.

### 5.2 AI patterns

- **Slime AI:** grounded; idle 0.7–2.0 s between hops; hop = vx 5.6 t/s toward nearest player, vy 21 t/s (≈2.4 tile apex); every 3rd hop is high: vy 26 t/s (≈3.7 tiles). Floats on water (lava slimes float on lava and bounce 1.5× higher out of it). Turns at ledges only when passive.
- **Fighter AI (Zombie, Skeleton):** walks toward target at 3.2 t/s (Skeleton 3.8). If blocked horizontally and on ground: jump vy 21 t/s (clears ~2.5 tiles); auto-steps up 1-tile ledges. Cannot open doors. Zombies at dawn: flee away from players and despawn when off-screen.
- **Flier — bouncer (Demon Eye):** accelerates toward player at 18 t/s², max speed 9.4 t/s, slow turn rate (max 90°/s); on tile collision, reflects velocity and adds vy −7.5 t/s (bounce up). Knockback fully changes trajectory.
- **Flier — erratic (Cave Bat, Watchling):** seeks player at max 12 t/s but every 0.25–0.6 s adds a random velocity jitter of up to ±6 t/s on each axis. Watchlings: no jitter, fly straight at the player at 10.5 t/s, blocked by tiles normally.
- **Swooper + caster (Ash Demon):** hovers 8–12 tiles from player, swoops through them at 14 t/s then retreats; every 4 s, if line-of-sight, fires a volley of 4 **Void Sickles** (projectile: 30 dmg, starts 6 t/s accelerating at 15 t/s² up to 25 t/s, destroyed by tiles, 33% Darkness debuff — player light radius halved 5 s).

### 5.3 Spawning algorithm (per player, evaluated every tick)

1. Determine player's environment → base **spawn rate denominator D** and **max spawns M**:

| Environment | D (1-in-D chance/tick) | M |
|---|---|---|
| Surface, day | 600 | 5 |
| Surface, night | 300 | 7 |
| Underground (rows 341–449) | 360 | 6 |
| Caverns (450–999) | 240 | 8 |
| Underworld (1000+) | 240 | 8 |

2. **Crowding scaling:** count active hostile NPCs in the player's despawn rectangle as `C` slots. If C ≥ M → no spawn. Else multiply D by 0.6 / 0.7 / 0.8 / 0.9 / 1.0 for C < 20% / 40% / 60% / 80% / 100% of M (lower D = more spawns: C<20% → D×0.6).
3. **Town suppression:** each town NPC within 50 tiles: D ×1.5 and M −2. If M ≤ 0 or 3+ town NPCs within 50 tiles: no hostile spawns.
4. Roll `1 in D`. On success, pick a random tile in the **spawn ring**: 62–84 tiles horizontally and 35–46 tiles vertically from the player (never on-screen). Grounded enemies need a solid tile with 3×2 air above; fliers need a 2×2 air pocket. Try 50 candidate tiles, else give up this tick.
5. Species weights: Surface day — Green Slime 60, Blue Slime 40. Surface night — Zombie 55, Demon Eye 35, Blue Slime 10. Underground — Blue Slime 60, Cave Bat 40. Caverns — Skeleton 45, Cave Bat 40, Blue Slime 15. Underworld — Lava Slime 50, Ash Demon 30, Cave Bat 20.
6. Each spawned enemy occupies 1 slot. Bosses ignore this system entirely.

---

## 6. Bosses

All bosses: immune to knockback, don't count toward spawn caps, persist until killed/despawn condition. On defeat, a world flag is set (used by NPC dialogue/Nurse pricing). Boss drops spawn at boss death position.

### 6.1 Slime Monarch (King Slime equivalent)

| Stat | Value |
|---|---|
| HP | 2000 |
| Contact damage | 40 |
| Defense | 10 |
| Size | 6×4 tiles at full HP; scale = 0.4 + 0.6 × (HP/2000); speed multiplier = 2 − HP/2000 |

**Summon:** Gel Crown (recipe #68), any time, any place. Natural: each dawn, 1/300 chance if a player is in the outer sixths of the map on the surface.
**AI loop:** repeat [normal hop ×2 → low hop → high hop]. Normal hop: vx 7 t/s toward player, vy 26 t/s. Low hop: vx 10 t/s, vy 17 t/s. High hop: vx 7 t/s, vy 38 t/s (≈8 tile apex). 0.5 s grounded pause between hops (scaled by speed multiplier).
**Teleport:** if it hasn't reached the player for 5 s, or player is >40 tiles away horizontally: shrink-sink over 1 s (invulnerable), reappear 10 tiles above the player's head after 2 s.
**Minions:** every 10% of max HP lost, spawns 2 Blue Slimes at its position.
**Despawn:** if no player within 150 tiles for 10 s.
**Drops:** 1 GC; 5–15 Lesser Healing Potions; 20–40 Gel; Royal Gel Charm (100% first kill; 25% after).

### 6.2 The Watcher (Eye of Cthulhu equivalent) — giant bloodshot eye

| Stat | Phase 1 (100–50% HP) | Phase 2 (<50%) |
|---|---|---|
| HP | 2800 total | (same pool) |
| Contact damage | 15 | 23 |
| Defense | 12 | 0 |
| Size | 4×4 tiles | 4×4 (pupil splits into fanged maw) |

**Summon:** Watcher's Iris (recipe #69), **night only** (disallow use if a Watcher is already alive). **Natural spawn:** at each dusk, 33% chance if: not yet defeated, no Watcher alive, any player has ≥200 max HP and ≥10 defense, and ≥2 town NPCs are housed. Warning chat line at dusk: *"You feel something watching you..."*; spawns 81 real seconds later.
**Phase 1 loop:** hover to a point 12 tiles above player (max speed 15 t/s, accel 30 t/s²) for 4 s, spawning 1 Watchling every 1.3 s (max 3 alive per hover); then 3 charges: aim 0.5 s → dash at 25 t/s for 0.8 s (passes through tiles) → 0.4 s drift; repeat.
**Phase 2 (at 1400 HP):** spins in place 1.5 s (invulnerable), transforms. No more minions. Loop: 3 charges at 30 t/s with 0.25 s between, then 1.5 s hover.
**Dawn rule:** at 4:30 AM it disengages, accelerates straight up off-world, and despawns (does not count as defeated).
**Drops:** 30–90 Gold Ore; 3 GC; 5–15 Lesser Healing Potions; Bloodshot Lens (100% first kill; 25% after).

### 6.3 The Bone Warden (Skeletron equivalent) — floating skull + 2 skeletal hands

**Why Skeletron over Wall of Flesh:** WoF requires a world-spanning moving wall, screen-edge kill pressure, and a post-fight world-state transition — out of scope. Skeletron's pattern reuses systems built for The Watcher (hovering flier, charge telegraphs, multi-entity boss), works in any 2D arena, and gives the underworld gear a target.

| Part | HP | Contact dmg | Defense |
|---|---|---|---|
| Skull | 4400 | 32 (56 while spinning) | 10 (0 while spinning) |
| Hand ×2 | 600 each | 20 | 14 |

**Summon:** Cursed Effigy (recipe #70), **night only**.
**AI:** Skull hovers 8 tiles above player, oscillating ±6 tiles horizontally (max 10 t/s). Hands orbit the skull at ±7 tiles; they alternate swipes every 1.5 s: windup 1 s (drift away), then sweep through the player's position at 20 t/s, return. Killing a hand removes its swipes; killing both raises skull aggression (hover speed +25%). Every **13.3 s** the skull roars and **spins for 6.7 s**: defense → 0, contact damage → 56, chases the player directly at 12 t/s (accel 20 t/s²), passes through tiles while spinning. Boss only takes damage on skull; hands are separate HP pools.
**Dawn enrage:** at 4:30 AM, permanent spin, contact damage 9999.
**Despawn:** all players dead, or dawn-enrage with no player within 100 tiles.
**Drops:** 5 GC; 5–15 Healing Potions; Skull Charm (100% first kill; 25% after).

---

## 7. Town NPCs & Housing

### 7.1 Housing validity (checked on demand + every dawn)

A house is valid iff, flood-filling from a candidate interior air tile (8-connectivity blocked by solid tiles, closed doors, and platforms):

1. Flood fill terminates within **750 cells** and never touches a tile within 10 tiles of world edge.
2. Total room size (interior cells + boundary frame) is **60–750 tiles**.
3. The boundary contains ≥1 **Door**.
4. Interior contains ≥1 light source (Torch/Furnace), ≥1 flat-surface item (Table or Workbench), ≥1 comfort item (Chair or Bed).
5. ≥60% of interior cells have a background wall (any wall type).
6. Interior contains ≥1 valid **home tile**: a 1×3 air column standing on solid floor not occupied by a door.
7. No other NPC already assigned to a cell of this room.

NPCs auto-claim the nearest vacant valid house at dawn. A dead town NPC respawns at the next dawn if a valid house exists. Town NPCs are passive, 250 HP, 15 defense, fight back for 10 damage when hurt, walk randomly within 25 tiles of home in day, stand inside at night.

### 7.2 Roster & arrival

| NPC | Arrival condition |
|---|---|
| **Sage** (Guide) | At world spawn from the start; claims first valid house |
| **Merchant** | All players' combined inventory coins ≥ 50 SC, and a vacant valid house exists |
| **Nurse** | Any player has max HP > 100 (used a Life Crystal), Merchant present, vacant valid house |

### 7.3 Merchant shop (buys back any item at 20% of base value)

| Item | Price |
|---|---|
| Torch | 50 CC |
| Bottle | 20 CC |
| Wooden Arrow | 5 CC |
| Lesser Healing Potion | 3 SC |
| Copper Pickaxe | 5 SC |
| Copper Axe | 4 SC |
| Anvil | 50 SC |
| Mining Helmet | 4 GC |

### 7.4 Nurse healing

Right-click → "Heal": restores full HP and clears all debuffs **except Potion Sickness**. Cost = `1 CC × HP restored + 1 SC per debuff cleared`, ×3 once The Watcher is defeated, ×10 once The Bone Warden is defeated. Minimum 10 CC. No effect (and no charge) at full HP.

### 7.5 Dialogue (pick uniformly among lines whose condition holds; `default` always eligible)

**Sage:**
1. (default) "You can press buttons to chop trees. Wood builds everything — start with a workbench."
2. (default) "If you see a pot, smash it. If you see a heart-shaped crystal, REALLY smash it."
3. (default) "Furnaces smelt ore into bars. Three copper ore per bar — the deeper metals cost four."
4. (night) "Keep your walls sealed at night. Zombies can't open doors, but they're patient."
5. (night, before Watcher defeated) "Sometimes I feel an enormous gaze on the back of my neck. Probably nothing."
6. (player HP < 30%) "You look terrible. Gel, a mushroom, and a bottle make a healing potion — write that down."
7. (boss alive) "Less talking, more fighting! I'll be under this table."
8. (Watcher defeated) "You actually beat it. The old stories say a crowned slime and a buried warden remain."
9. (homeless) "Build me a room — walls, a door, a light, a table, a chair. I'm not picky. That's a lie, I'm exactly that picky."

**Merchant:**
1. (default) "Everything's for sale, friend. Even my respect — that one's pricey."
2. (default) "Torches! Fifty copper! Darkness is free and look where THAT gets you."
3. (default) "I buy junk at a fifth of its worth. It's not a scam, it's logistics."
4. (night) "We're open all night. Mostly because I can't sleep with all that moaning outside."
5. (player coins > 1 GC) "Is that gold I smell? My prices may have just... matured."
6. (boss alive) "Shop's open but the refund window is CLOSED until that thing stops screaming."
7. (player HP < 30%) "No bleeding on the merchandise. Healing potion, three silver. A bargain, given the alternative."
8. (Slime Monarch defeated) "You sold me forty units of royal gel. I respect the hustle."

**Nurse:**
1. (default) "Walk it off? No. Pay me, and I'll fix it properly."
2. (default) "I've seen every injury this world can produce. You're going to show me a new one, aren't you."
3. (player HP < 30%) "Sit. Down. Now. You're getting blood on the floor I just had."
4. (player at full HP) "You're fine. Stop wasting my time and go get hurt somewhere."
5. (night) "Night shift again. The screaming outside really completes the clinic ambiance."
6. (boss alive) "Triage rules: heroes first, cowards pay double. Kidding. Mostly."
7. (after any boss defeated) "Fewer monsters means fewer patients. Don't take that personally."
8. (Potion Sickness active) "I can't purge potion sickness — your liver and I have an agreement."

---

## 8. Player

| Property | Value |
|---|---|
| Base max HP | 100; +20 per Life Crystal, max 400 (15 crystals) |
| Passive regen | 0.5 HP/s after 8 s without taking damage; ×2 while standing still |
| Run speed (max) | 11.25 tiles/s; acceleration 18 t/s²; ground friction decel 45 t/s² |
| Jump | Hold-to-rise: vy = −18.79 t/s held constant up to 0.25 s, then ballistic under gravity. Max height ≈ 6.5 tiles; tap = ~1.5 tiles. 1-tile ledges auto-step |
| Gravity / terminal | 90 t/s² / 37.5 t/s (37.5 also caps all entities) |
| Hitbox | 1.25 × 2.75 tiles (20×44 px) |
| Fall damage | Safe distance 25 tiles; beyond: `10 × (tiles_fallen − 25)`. Negated by Lucky Charm, landing in ≥2-deep liquid, or Gust Jar mid-air jump |
| Breath | 200 units; drains 1 unit / 7 ticks fully submerged (≈23.3 s); refills 3 units/tick out of water (~1.1 s full). At 0 breath: 10 dmg/s (ignores defense) |
| Reach | Place/mine within 6 tiles of player center |
| Death | Drop 50% of carried coins at death location (item pile, persists 10 min); respawn at bed spawn (if valid bed) else world spawn, with `max(100, maxHP/2)` HP. Respawn timer 10 s, 20 s while any boss is alive |
| Inventory | Hotbar 10 slots + backpack 40 slots + 3 armor + 3 accessory + trash slot |
| Starting kit | Wood Sword, Wood Pickaxe, Wood Axe, 5 Torches |

Debuffs: **Burning** (2 dmg/s, ignores defense), **Darkness** (light radius halved), **Potion Sickness** (no healing items).

---

## 9. Day/Night Cycle

1 in-game minute = 1 real second.

| Property | Value |
|---|---|
| Full cycle | 24 real minutes |
| Day | 4:30 AM – 7:30 PM (15 real min) |
| Night | 7:30 PM – 4:30 AM (9 real min) |
| New world starts at | 8:15 AM |
| Dusk (7:30 PM) | Surface switches to night spawn table; Watcher natural-spawn roll; night-only summons usable |
| Dawn (4:30 AM) | Zombies/Demon Eyes flee & despawn off-screen; The Watcher flees; Bone Warden enrages; dead town NPCs respawn; housing revalidation |
| Sleeping in bed | Time passes ×5 while all players sleep (night only) |

Sky light interpolates over the 30 in-game minutes around dusk/dawn (see §10).

---

## 10. Lighting

Block-resolution flood-fill lighting, client-side only.

- Each tile stores light level **0–32** (u8). Render brightness = level/32; level 0 = pitch black.
- **Sources:** Torch 28, Lava 18 per fluid cell, Hellstone 13, Infernal Forge 10, Ritual Altar 8, Life Crystal 6, Furnace 6, Mining Helmet 20 (at player head), player ambient glow 4.
- **Sunlight:** every air tile with no solid tile anywhere above it is a source at `skyLight`. `skyLight` = 32 during full day, 8 at full night, linearly interpolated across 30 in-game minutes centered on dusk/dawn.
- **Propagation:** BFS from all sources, 4-directional. Attenuation per step: **−2 entering air/non-solid**, **−6 entering solid**. Take max when fields overlap. Torch in open air → radius 14 tiles; sunlight penetrates ~5 tiles of solid ground.
- **Recompute:** chunk-based (32×32 light chunks); a chunk and its 8 neighbors are dirtied by any tile/source change; sky-light dirties exposed chunks every 10 ticks during dusk/dawn ramps. Only visible-area ±2 chunks need computation per client; the server doesn't compute lighting (spawn rules use time/depth, not light).

---

## 11. Multiplayer

Server-authoritative; clients send inputs + intents (mine tile, place tile, use item, open chest), server validates (range ≤ 6 tiles, item possession) and broadcasts diffs.

| Per-player state | Shared world state |
|---|---|
| Position/velocity, facing, HP, breath, debuffs, respawn timer | Tile map, walls, fluid grid (server-simulated) |
| Inventory, hotbar, armor, accessories, coins | Time of day, world flags (bosses defeated) |
| Personal bed spawn point | Chest contents (chest locked to one open player at a time) |
| Potion Sickness, regen timers | All enemies/bosses/projectiles/dropped items |
| Spawn-ring RNG (spawning per player, §5.3) | Town NPC roster, housing assignments |
| Lighting (client-side only) | Ritual Altars, world spawn point |

Rules: enemy spawn caps `M` count enemies in *each* player's rectangle independently; an enemy despawns only when outside **all** players' ranges. Bosses target the nearest living player, retarget on death. No boss HP scaling with player count in v1. Item drops are world-shared (first pickup wins). Nurse/Merchant interactions are per-player transactions. Tile changes are sequenced by server tick (first intent wins; loser's action is refunded).

---

## Deliberate deviations from Terraria

So engineers don't "fix" them: Hellstone/Obsidian mineable at 55 pick power (no demonite-tier exists here; The Watcher drops gold ore instead); arrow recipe yields 25; armor bar costs normalized to 15/25/20 per set; integer lighting instead of smooth colored lighting; Skeletron-pattern boss replaces Wall of Flesh; simplified wall-coverage housing rule (60%) replaces Terraria's hole-size rule.
