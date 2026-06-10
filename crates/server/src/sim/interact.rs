//! Mine/place/build intent handlers (DESIGN §2, §8 reach): `HitTile`,
//! `HitWall`, `PlaceTile`, `PlaceWall`, `ToggleDoor`.
//!
//! The §2 mining model: every tile has 100 break-points; each accepted
//! swing deals `tool_power × hardness_mult` (zero below the tile's minimum
//! power); accumulated damage decays after 5 s without hits. Swings are
//! rate-limited server-side by the held tool's use time (±1 tick of network
//! jitter tolerated). All world mutation goes through `Sim::change_tile`
//! (immediate `TileChanged`), and all drops through `Sim::spawn_item_drop`.

use ferraria_shared::items::{inventory, ItemId, Placement, ToolStats, BARE_HAND_USE_SECS};
use ferraria_shared::physics::{COLLISION_EPS, PLAYER_HEIGHT, PLAYER_WIDTH};
use ferraria_shared::protocol::ServerMessage;
use ferraria_shared::tiles::{
    state, Liquid, LiquidKind, Solidity, TileId, ToolKind, WallId, ACORN_DROP_CHANCE,
    SAPLING_GROW_MAX_SECS, SAPLING_GROW_MIN_SECS, TILE_BREAK_POINTS, TILE_DAMAGE_RESET_SECS,
    WOOD_PER_TREE_SEGMENT,
};
use ferraria_shared::world::World;
use ferraria_shared::{tile_in_reach, TICK_RATE};

use crate::worldgen::loot;

use super::game::Sim;

/// Accumulated §2 mining damage on one cell (foreground or wall layer).
#[derive(Debug, Clone, Copy)]
pub struct TileDamage {
    /// Break-points dealt so far (out of [`TILE_BREAK_POINTS`]).
    pub damage: f32,
    /// Tick of the last hit; the total resets after
    /// [`TILE_DAMAGE_RESET_SECS`] without one.
    pub last_hit_tick: u64,
}

/// Swing interval in ticks for whatever is in hand: tools and weapons use
/// their §4.1 use time, bare hands the canonized default.
fn use_ticks(held: Option<ItemId>) -> u64 {
    let secs = held
        .and_then(|i| {
            let d = i.data();
            d.tool
                .map(|t| t.use_secs)
                .or(d.weapon.map(|w| w.use_secs))
        })
        .unwrap_or(BARE_HAND_USE_SECS);
    ((secs * TICK_RATE as f32).round() as u64).max(1)
}

/// Does a held tool satisfy a tile's tool requirement?
fn tool_matches(required: ToolKind, tool: Option<ToolStats>) -> bool {
    match required {
        ToolKind::Any => true,
        ToolKind::None => false,
        k => tool.is_some_and(|t| t.kind == k),
    }
}

/// Support rule for single-tile placement (Terraria-style): a wall behind
/// the cell, or a solid/platform tile cardinally adjacent.
fn has_support(world: &World, x: u32, y: u32) -> bool {
    if world.tile(x, y).wall != WallId::Air {
        return true;
    }
    let (xi, yi) = (x as i32, y as i32);
    [(xi - 1, yi), (xi + 1, yi), (xi, yi - 1), (xi, yi + 1)]
        .into_iter()
        .any(|(nx, ny)| {
            // Out-of-bounds reads count as solid for physics, but the world
            // border must not support placement.
            nx >= 0
                && ny >= 0
                && world.in_bounds(nx as u32, ny as u32)
                && (world.is_solid(nx, ny) || world.is_platform(nx, ny))
        })
}

/// Every bottom cell of a `w`-wide footprint at origin `(x, y)`, height `h`,
/// stands on solid ground or a platform (furniture floor rule).
fn floor_under(world: &World, x: u32, y: u32, w: u32, h: u32) -> bool {
    (0..w).all(|dx| {
        let (fx, fy) = ((x + dx) as i32, (y + h) as i32);
        world.is_solid(fx, fy) || world.is_platform(fx, fy)
    })
}

impl Sim {
    /// Validates reach + swing rate for player `id` aiming at `(x, y)`.
    /// Returns the held tool (if any) and consumes the swing cooldown.
    /// `None` means the swing was rejected (don't consume the cooldown for
    /// invalid targets — callers check the target *before* calling this).
    fn accept_swing(&mut self, id: u32, x: u32, y: u32) -> Option<Option<ToolStats>> {
        if !self.world.in_bounds(x, y) {
            return None;
        }
        let tick = self.tick;
        let p = self.players.get_mut(&id)?;
        if !tile_in_reach(p.center(), x, y) {
            return None;
        }
        let held = p
            .inventory
            .get(p.held_slot as usize)
            .copied()
            .flatten()
            .map(|s| s.item);
        // Server-enforced swing rate from the held item's use time, with 1
        // tick of tolerance for client/server tick-phase jitter.
        if let Some(last) = p.last_swing_tick {
            if tick.saturating_sub(last) + 1 < use_ticks(held) {
                return None;
            }
        }
        p.last_swing_tick = Some(tick);
        Some(held.and_then(|i| i.data().tool))
    }

    /// `HitTile`: one swing of the held tool at the foreground layer.
    pub(crate) fn hit_tile(&mut self, id: u32, x: u32, y: u32) {
        let tile = self.world.tile(x, y);
        let data = tile.id.data();
        if tile.id == TileId::Air || !data.breakable {
            // Ritual Altars are unbreakable (§2; the hammer-backlash damage
            // arrives with the combat systems — there is no player HP here
            // yet). Air is simply not a target.
            return;
        }
        let Some(tool) = self.accept_swing(id, x, y) else {
            return;
        };
        if !tool_matches(data.tool, tool) {
            return;
        }
        let power = tool.map(|t| t.power).unwrap_or(0);
        if power < data.min_power {
            return; // §2: below minimum power the tile takes zero damage
        }
        if data.one_hit {
            self.break_tile(x, y);
            return;
        }
        let dealt = power as f32 * data.hardness_mult;
        if dealt <= 0.0 {
            return;
        }
        if self.apply_damage((x, y, false), dealt) {
            self.break_tile(x, y);
        }
    }

    /// `HitWall`: hammers only (§2 walls).
    pub(crate) fn hit_wall(&mut self, id: u32, x: u32, y: u32) {
        let tile = self.world.tile(x, y);
        if tile.wall == WallId::Air {
            return;
        }
        let Some(tool) = self.accept_swing(id, x, y) else {
            return;
        };
        let Some(tool) = tool.filter(|t| t.kind == ToolKind::Hammer) else {
            return;
        };
        let dealt = tool.power as f32 * tile.wall.data().hardness_mult;
        if dealt <= 0.0 {
            return;
        }
        if self.apply_damage((x, y, true), dealt) {
            let mut t = self.world.tile(x, y);
            // §2: only player-placed walls drop their item.
            if t.state & state::WALL_PLACED != 0 {
                if let Some(item) = t.wall.data().drops {
                    self.spawn_item_drop(item, 1, (x as f32 + 0.5, y as f32 + 0.5));
                }
            }
            t.wall = WallId::Air;
            t.state &= !state::WALL_PLACED;
            self.change_tile(x, y, t);
        }
    }

    /// Adds swing damage to a cell (resetting first if the §2 5 s decay
    /// elapsed). Returns `true` when the cell's break-points are exhausted;
    /// otherwise broadcasts the crack overlay frame.
    fn apply_damage(&mut self, key: (u32, u32, bool), dealt: f32) -> bool {
        let reset_ticks = (TILE_DAMAGE_RESET_SECS * TICK_RATE as f32) as u64;
        let tick = self.tick;
        let entry = self.tile_damage.entry(key).or_insert(TileDamage {
            damage: 0.0,
            last_hit_tick: tick,
        });
        if tick.saturating_sub(entry.last_hit_tick) > reset_ticks {
            entry.damage = 0.0;
        }
        entry.damage += dealt;
        entry.last_hit_tick = tick;
        let damage = entry.damage;
        if damage >= TILE_BREAK_POINTS as f32 {
            self.tile_damage.remove(&key);
            return true;
        }
        let frac = ((damage / TILE_BREAK_POINTS as f32) * 255.0).min(255.0) as u8;
        self.broadcast_at(
            key.0,
            key.1,
            &ServerMessage::BlockCrack {
                x: key.0,
                y: key.1,
                damage_frac: frac,
            },
        );
        false
    }

    /// Breaks the foreground object at `(x, y)`: trees fell upward, pots
    /// roll loot, multi-tile furniture clears its whole footprint (refusing
    /// for non-empty chests), everything else drops its §2 item.
    pub(crate) fn break_tile(&mut self, x: u32, y: u32) {
        let tile = self.world.tile(x, y);
        match tile.id {
            TileId::Air => {}
            TileId::TreeTrunk => self.fell_tree(x, y),
            TileId::Pot => {
                self.clear_cell(x, y);
                let mult = loot::pot_coin_mult(y, self.world.height);
                let slot = loot::roll_pot(&mut self.loot_rng, mult);
                self.spawn_item_drop(slot.item, slot.count, (x as f32 + 0.5, y as f32 + 0.5));
            }
            TileId::Sapling => {
                self.saplings.remove(&(x, y));
                self.clear_cell(x, y);
            }
            id if id.data().size != (1, 1) => self.break_multitile(x, y),
            id => {
                self.clear_cell(x, y);
                if let Some((item, n)) = id.data().drops {
                    self.spawn_item_drop(item, n as u16, (x as f32 + 0.5, y as f32 + 0.5));
                }
            }
        }
    }

    /// Clears one foreground cell (wall, liquid, and the WALL_PLACED bit
    /// survive) and lets anything resting on it react (sand above falls,
    /// trees/saplings above fall too).
    fn clear_cell(&mut self, x: u32, y: u32) {
        let mut t = self.world.tile(x, y);
        let was_solid = t.is_solid();
        t.id = TileId::Air;
        t.state &= state::WALL_PLACED;
        self.change_tile(x, y, t);
        if was_solid && y > 0 {
            let above = self.world.tile(x, y - 1);
            match above.id {
                TileId::TreeTrunk => self.fell_tree(x, y - 1),
                TileId::Sapling => self.break_tile(x, y - 1),
                _ => {}
            }
        }
    }

    /// §2 row 32: felling a segment fells everything above it, dropping 10
    /// wood + a 25% acorn per segment at each segment's position.
    fn fell_tree(&mut self, x: u32, y: u32) {
        let mut yy = y;
        loop {
            if self.world.tile(x, yy).id != TileId::TreeTrunk {
                break;
            }
            let mut t = self.world.tile(x, yy);
            t.id = TileId::Air;
            t.state &= state::WALL_PLACED;
            self.change_tile(x, yy, t);
            let center = (x as f32 + 0.5, yy as f32 + 0.5);
            self.spawn_item_drop(ItemId::Wood, WOOD_PER_TREE_SEGMENT as u16, center);
            if self.loot_rng.chance(ACORN_DROP_CHANCE) {
                self.spawn_item_drop(ItemId::Acorn, 1, center);
            }
            if yy == 0 {
                break;
            }
            yy -= 1;
        }
    }

    /// Breaks a multi-tile furniture object as a unit from any of its cells.
    fn break_multitile(&mut self, x: u32, y: u32) {
        let (ox, oy) = self.world.multitile_origin(x, y);
        let id = self.world.tile(ox, oy).id;
        let data = id.data();
        if id == TileId::Chest {
            let occupied = self
                .world
                .chests
                .get(&(ox, oy))
                .is_some_and(|slots| slots.iter().any(Option::is_some));
            if occupied {
                return; // §2: can't break a chest while non-empty
            }
            self.world.chests.remove(&(ox, oy));
        }
        let (w, h) = (data.size.0 as u32, data.size.1 as u32);
        for dy in 0..h {
            for dx in 0..w {
                if self.world.tile(ox + dx, oy + dy).id == id {
                    let mut t = self.world.tile(ox + dx, oy + dy);
                    t.id = TileId::Air;
                    t.state &= state::WALL_PLACED;
                    self.change_tile(ox + dx, oy + dy, t);
                }
            }
        }
        if let Some((item, n)) = data.drops {
            self.spawn_item_drop(
                item,
                n as u16,
                (ox as f32 + w as f32 / 2.0, oy as f32 + h as f32 / 2.0),
            );
        }
    }

    /// `PlaceTile`: validates reach, possession, §2 placement rules
    /// (emptiness, support, furniture floors, door frames, torch
    /// attachment), and that solids never overlap a player/entity hitbox,
    /// then consumes the item.
    pub(crate) fn place_tile(&mut self, id: u32, x: u32, y: u32, slot: u8) {
        if !self.world.in_bounds(x, y) || slot as usize >= inventory::HOTBAR {
            return;
        }
        let Some(p) = self.players.get(&id) else {
            return;
        };
        if !tile_in_reach(p.center(), x, y) {
            return;
        }
        let Some(stack) = p.inventory.get(slot as usize).copied().flatten() else {
            return;
        };
        let Some(Placement::Tile(tile_id)) = stack.item.data().places else {
            return;
        };
        let data = tile_id.data();
        let (w, h) = (data.size.0 as u32, data.size.1 as u32);

        // Footprint must be empty (also checked atomically by
        // place_multitile, but validating first keeps refusals side-effect
        // free).
        for dy in 0..h {
            for dx in 0..w {
                if !self.world.is_empty(x + dx, y + dy) {
                    return;
                }
            }
        }

        // Per-kind placement rules.
        let valid = match tile_id {
            // Acorns plant saplings on grass (§4.3).
            TileId::Sapling => {
                y + 1 < self.world.height && self.world.tile(x, y + 1).id == TileId::Grass
            }
            // Torches attach to an adjacent solid or a wall, never in water
            // (§2 tile 16).
            TileId::Torch => {
                let t = self.world.tile(x, y);
                t.liquid.kind() != Some(LiquidKind::Water) && has_support(&self.world, x, y)
            }
            // Doors need solid tiles directly above and below the 1×3 frame
            // (§2 tile 18).
            TileId::Door => {
                y > 0 && self.world.is_solid(x as i32, y as i32 - 1)
                    && self.world.is_solid(x as i32, (y + h) as i32)
            }
            // Other furniture stands on its floor.
            _ if data.furniture && data.size != (1, 1) => floor_under(&self.world, x, y, w, h),
            _ if data.furniture => {
                floor_under(&self.world, x, y, w, h) || has_support(&self.world, x, y)
            }
            // Plain blocks/platforms: the Terraria-style support rule.
            _ => has_support(&self.world, x, y),
        };
        if !valid {
            return;
        }

        // A placed solid must not overlap any player or entity hitbox.
        let solid = matches!(data.solidity, Solidity::Solid);
        if solid && self.footprint_overlaps_hitbox(x, y, w, h) {
            return;
        }

        // Apply.
        if data.size == (1, 1) {
            let mut t = self.world.tile(x, y);
            t.id = tile_id;
            t.state &= state::WALL_PLACED;
            if solid {
                // Solid blocks displace (destroy) the cell's liquid.
                t.liquid = Liquid::NONE;
            }
            self.change_tile(x, y, t);
        } else {
            if !self.world.place_multitile(x, y, tile_id) {
                return;
            }
            for dy in 0..h {
                for dx in 0..w {
                    let t = self.world.tile(x + dx, y + dy);
                    self.change_tile(x + dx, y + dy, t);
                }
            }
            if tile_id == TileId::Chest {
                self.world
                    .chests
                    .insert((x, y), vec![None; ferraria_shared::world::CHEST_SLOTS]);
            }
        }
        if tile_id == TileId::Sapling {
            let grow_secs = self
                .loot_rng
                .gen_range_f32(SAPLING_GROW_MIN_SECS, SAPLING_GROW_MAX_SECS);
            self.saplings
                .insert((x, y), self.tick + (grow_secs * TICK_RATE as f32) as u64);
        }
        self.consume_item(id, slot);
    }

    /// `PlaceWall`: empty wall layer + Terraria-style support; the placed
    /// wall gets the WALL_PLACED bit so hammering it drops the item (§2).
    pub(crate) fn place_wall(&mut self, id: u32, x: u32, y: u32, slot: u8) {
        if !self.world.in_bounds(x, y) || slot as usize >= inventory::HOTBAR {
            return;
        }
        let Some(p) = self.players.get(&id) else {
            return;
        };
        if !tile_in_reach(p.center(), x, y) {
            return;
        }
        let Some(stack) = p.inventory.get(slot as usize).copied().flatten() else {
            return;
        };
        let Some(Placement::Wall(wall_id)) = stack.item.data().places else {
            return;
        };
        let t = self.world.tile(x, y);
        if t.wall != WallId::Air || t.is_solid() {
            return;
        }
        // Wall support: an adjacent wall or an adjacent solid/platform.
        let (xi, yi) = (x as i32, y as i32);
        let supported = [(xi - 1, yi), (xi + 1, yi), (xi, yi - 1), (xi, yi + 1)]
            .into_iter()
            .any(|(nx, ny)| {
                nx >= 0
                    && ny >= 0
                    && self.world.in_bounds(nx as u32, ny as u32)
                    && (self.world.tile(nx as u32, ny as u32).wall != WallId::Air
                        || self.world.is_solid(nx, ny)
                        || self.world.is_platform(nx, ny))
            });
        if !supported {
            return;
        }
        let mut t = t;
        t.wall = wall_id;
        t.state |= state::WALL_PLACED;
        self.change_tile(x, y, t);
        self.consume_item(id, slot);
    }

    /// `ToggleDoor` (§2 tile 18): opening swings the panel to the side away
    /// from the toggling player (falling back to the other side, refusing
    /// if both are blocked); closing refuses while any player or entity
    /// overlaps the doorway.
    pub(crate) fn toggle_door(&mut self, id: u32, x: u32, y: u32) {
        if !self.world.in_bounds(x, y) || self.world.tile(x, y).id != TileId::Door {
            return;
        }
        let Some(p) = self.players.get(&id) else {
            return;
        };
        if !tile_in_reach(p.center(), x, y) {
            return;
        }
        let player_x = p.center().0;
        let (ox, oy) = self.world.multitile_origin(x, y);
        let open = self.world.tile(ox, oy).state & state::DOOR_OPEN != 0;
        let h = TileId::Door.data().size.1 as u32;
        if open {
            // Closing makes the column solid again: nobody may be standing
            // in it.
            if self.footprint_overlaps_hitbox(ox, oy, 1, h) {
                return;
            }
            for dy in 0..h {
                let mut t = self.world.tile(ox, oy + dy);
                t.state &= !(state::DOOR_OPEN | state::DOOR_OPEN_LEFT);
                self.change_tile(ox, oy + dy, t);
            }
        } else {
            // The panel needs a clear side column; prefer the side away
            // from the player.
            let away_left = player_x > ox as f32 + 0.5;
            let side_clear = |sim: &Sim, left: bool| -> bool {
                let sx = if left {
                    (ox as i32) - 1
                } else {
                    (ox as i32) + 1
                };
                (0..h).all(|dy| {
                    sx >= 0
                        && sim.world.in_bounds(sx as u32, oy + dy)
                        && !sim.world.is_solid(sx, (oy + dy) as i32)
                })
            };
            let left = if side_clear(self, away_left) {
                away_left
            } else if side_clear(self, !away_left) {
                !away_left
            } else {
                return; // jammed: both sides blocked
            };
            for dy in 0..h {
                let mut t = self.world.tile(ox, oy + dy);
                t.state |= state::DOOR_OPEN;
                if left {
                    t.state |= state::DOOR_OPEN_LEFT;
                } else {
                    t.state &= !state::DOOR_OPEN_LEFT;
                }
                self.change_tile(ox, oy + dy, t);
            }
        }
    }

    /// Whether the tile-aligned rect `(x, y, w, h)` overlaps any player or
    /// entity AABB (shrunk by the collision skin so flush contacts — feet
    /// exactly on top of the placed tile — don't count).
    fn footprint_overlaps_hitbox(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        let (rx0, ry0) = (x as f32 + COLLISION_EPS, y as f32 + COLLISION_EPS);
        let (rx1, ry1) = (
            (x + w) as f32 - COLLISION_EPS,
            (y + h) as f32 - COLLISION_EPS,
        );
        let overlaps = |pos: (f32, f32), size: (f32, f32)| -> bool {
            pos.0 < rx1 && pos.0 + size.0 > rx0 && pos.1 < ry1 && pos.1 + size.1 > ry0
        };
        self.players
            .values()
            .any(|p| overlaps(p.pos, (PLAYER_WIDTH, PLAYER_HEIGHT)))
            || self.entities.map.values().any(|e| overlaps(e.pos, e.size()))
    }

    /// Removes one item from `slot`, pushing the delta to the owner (and a
    /// held-item update to everyone else if their visible hand changed).
    fn consume_item(&mut self, id: u32, slot: u8) {
        let Some(p) = self.players.get_mut(&id) else {
            return;
        };
        let Some(Some(stack)) = p.inventory.get_mut(slot as usize) else {
            return;
        };
        stack.count = stack.count.saturating_sub(1);
        if stack.count == 0 {
            p.inventory[slot as usize] = None;
        }
        let new_stack = p.inventory[slot as usize];
        let held_changed = p.held_slot == slot && new_stack.is_none();
        self.send_to(
            id,
            &ServerMessage::SlotChanged {
                idx: slot,
                stack: new_stack,
            },
        );
        if held_changed {
            self.broadcast_held_item(id);
        }
    }
}
