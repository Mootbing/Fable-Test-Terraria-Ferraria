//! Server-simulated entities: the [`EntityStore`] and the item-drop systems
//! (physics, lava destruction, merging, pickup, despawn).
//!
//! The store is the foundation every later entity feature builds on:
//! enemies and projectiles become new [`EntityKind`] variants with their own
//! per-tick systems, sharing ids, snapshot batching, and chunk-window
//! visibility with item drops.
//!
//! Wire mapping: item drops announce themselves with
//! [`ServerMessage::ItemDropSpawn`] (it carries item + count, which the
//! generic `EntitySpawn` state byte cannot), move via the shared
//! [`ServerMessage::EntityUpdate`] batches every 3 ticks (only to players
//! whose chunk window contains them), and leave with
//! [`ServerMessage::EntityDespawn`] / [`ServerMessage::ItemPickedUp`]
//! (despawn-class messages go to everyone so no mirror keeps a ghost).

use std::collections::BTreeMap;

use ferraria_shared::items::{add_to_inventory, ItemId};
use ferraria_shared::physics::{hitbox, step_item_drop, ITEM_SPAWN_SPEED_X, ITEM_SPAWN_SPEED_Y};
use ferraria_shared::protocol::{DespawnReason, EntityState, ServerMessage};
use ferraria_shared::tiles::LiquidKind;
use ferraria_shared::world::CHUNK_SIZE;
use ferraria_shared::{
    DT, ITEM_DESPAWN_SECS, ITEM_MERGE_RADIUS, ITEM_PICKUP_ARM_SECS, ITEM_PICKUP_RADIUS, TICK_RATE,
};

use super::game::Sim;

/// What an entity *is*. Currently only item drops; enemies, bosses, and
/// projectiles slot in as new variants (each with its own system in the
/// tick), reusing ids, snapshots, and visibility.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EntityKind {
    ItemDrop { item: ItemId, count: u16 },
}

/// One live entity. Positions are the AABB top-left in tile units, matching
/// the player/`EntityState` convention.
#[derive(Debug, Clone, Copy)]
pub struct Entity {
    pub pos: (f32, f32),
    pub vel: (f32, f32),
    pub kind: EntityKind,
    /// Sim tick it spawned (drives pickup arming and the 10-min despawn).
    pub spawn_tick: u64,
    /// Changed since the last snapshot batch; cleared after broadcasting.
    pub awake: bool,
}

impl Entity {
    pub fn size(&self) -> (f32, f32) {
        match self.kind {
            EntityKind::ItemDrop { .. } => hitbox::ITEM_DROP,
        }
    }

    pub fn center(&self) -> (f32, f32) {
        let (w, h) = self.size();
        (self.pos.0 + w / 2.0, self.pos.1 + h / 2.0)
    }

    /// Chunk coordinate containing the entity's center (unclamped; matches
    /// the player chunk-subscription keys for in-world positions).
    pub fn chunk(&self) -> (u32, u32) {
        let (cx, cy) = self.center();
        (
            (cx.max(0.0) as u32) / CHUNK_SIZE,
            (cy.max(0.0) as u32) / CHUNK_SIZE,
        )
    }
}

/// Id-keyed entity collection. `BTreeMap` so every per-tick iteration is in
/// deterministic id order ("first pickup wins" must not depend on hash
/// order).
pub struct EntityStore {
    next_id: u32,
    pub map: BTreeMap<u32, Entity>,
}

impl EntityStore {
    pub fn new() -> EntityStore {
        EntityStore {
            next_id: 1,
            map: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, entity: Entity) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.map.insert(id, entity);
        id
    }

    /// Spawn messages for every entity currently inside chunk `c` — sent to
    /// players whose window just gained that chunk.
    pub fn spawn_messages_in_chunk(&self, c: (u32, u32)) -> Vec<ServerMessage> {
        self.map
            .iter()
            .filter(|(_, e)| e.chunk() == c)
            .map(|(&id, e)| spawn_message(id, e))
            .collect()
    }
}

impl Default for EntityStore {
    fn default() -> Self {
        EntityStore::new()
    }
}

fn spawn_message(id: u32, e: &Entity) -> ServerMessage {
    match e.kind {
        EntityKind::ItemDrop { item, count } => ServerMessage::ItemDropSpawn {
            id,
            item,
            count,
            pos: e.pos,
            vel: e.vel,
        },
    }
}

/// Distance from a point to an AABB (0 inside) — the "player within 1.5
/// tiles" pickup test measures from the item center to the player's hitbox.
fn aabb_point_distance(pos: (f32, f32), size: (f32, f32), point: (f32, f32)) -> f32 {
    let dx = (pos.0 - point.0).max(point.0 - (pos.0 + size.0)).max(0.0);
    let dy = (pos.1 - point.1).max(point.1 - (pos.1 + size.1)).max(0.0);
    (dx * dx + dy * dy).sqrt()
}

impl Sim {
    /// Spawns an item drop at `center` with the standard small random pop
    /// impulse. Every drop source (mining, trees, pots, walls, furniture)
    /// funnels through here.
    pub(crate) fn spawn_item_drop(&mut self, item: ItemId, count: u16, center: (f32, f32)) -> u32 {
        let vel = (
            self.loot_rng
                .gen_range_f32(-ITEM_SPAWN_SPEED_X, ITEM_SPAWN_SPEED_X),
            -self
                .loot_rng
                .gen_range_f32(ITEM_SPAWN_SPEED_Y.0, ITEM_SPAWN_SPEED_Y.1),
        );
        self.spawn_item_drop_exact(item, count, center, vel, self.tick)
    }

    /// Spawn with explicit velocity and spawn tick (merges/partial pickups
    /// preserve the original timer).
    pub(crate) fn spawn_item_drop_exact(
        &mut self,
        item: ItemId,
        count: u16,
        center: (f32, f32),
        vel: (f32, f32),
        spawn_tick: u64,
    ) -> u32 {
        let (w, h) = hitbox::ITEM_DROP;
        let entity = Entity {
            pos: (center.0 - w / 2.0, center.1 - h / 2.0),
            vel,
            kind: EntityKind::ItemDrop { item, count },
            spawn_tick,
            awake: true,
        };
        let id = self.entities.insert(entity);
        let msg = spawn_message(id, &entity);
        self.broadcast_at(center.0.max(0.0) as u32, center.1.max(0.0) as u32, &msg);
        id
    }

    /// Per-tick entity systems: item-drop physics, lava destruction, the
    /// 10-minute despawn, stack merging, and player pickup.
    pub(crate) fn tick_entities(&mut self) {
        self.step_item_drops();
        self.merge_item_drops();
        self.pickup_item_drops();
    }

    fn step_item_drops(&mut self) {
        let despawn_ticks = (ITEM_DESPAWN_SECS * TICK_RATE as f32) as u64;
        let mut gone: Vec<(u32, DespawnReason)> = Vec::new();
        for (&id, e) in self.entities.map.iter_mut() {
            let before = (e.pos, e.vel);
            step_item_drop(&self.world, &mut e.pos, &mut e.vel, DT);
            if e.pos != before.0 || e.vel != before.1 {
                e.awake = true;
            }
            if self.tick.saturating_sub(e.spawn_tick) >= despawn_ticks {
                gone.push((id, DespawnReason::Despawned));
                continue;
            }
            let EntityKind::ItemDrop { item, .. } = e.kind;
            if !item.lava_immune() && touches_liquid(&self.world, e, LiquidKind::Lava) {
                gone.push((id, DespawnReason::Killed));
            }
        }
        for (id, reason) in gone {
            self.despawn_entity(id, reason);
        }
    }

    pub(crate) fn despawn_entity(&mut self, id: u32, reason: DespawnReason) {
        if self.entities.map.remove(&id).is_some() {
            // To everyone: chunk-window-filtered clients may hold a mirror
            // of this entity from an earlier subscription.
            self.broadcast(&ServerMessage::EntityDespawn { id, reason });
        }
    }

    /// Same-item drops within [`ITEM_MERGE_RADIUS`] collapse into one stack
    /// (respecting `max_stack`), implemented as despawn-both + fresh spawn
    /// so every client renders the exact merged count.
    fn merge_item_drops(&mut self) {
        let drops: Vec<(u32, ItemId, u16, (f32, f32), (f32, f32), u64, bool)> = self
            .entities
            .map
            .iter()
            .map(|(&id, e)| {
                let EntityKind::ItemDrop { item, count } = e.kind;
                (id, item, count, e.center(), e.vel, e.spawn_tick, e.awake)
            })
            .collect();
        let mut consumed = vec![false; drops.len()];
        let mut merges = Vec::new();
        for i in 0..drops.len() {
            if consumed[i] {
                continue;
            }
            for j in (i + 1)..drops.len() {
                if consumed[j] {
                    continue;
                }
                let (a, b) = (&drops[i], &drops[j]);
                // Only consider pairs where something moved this tick, so a
                // settled field of drops costs nothing.
                if !(a.6 || b.6) || a.1 != b.1 {
                    continue;
                }
                let total = a.2 as u32 + b.2 as u32;
                if total > a.1.max_stack() as u32 {
                    continue;
                }
                let (dx, dy) = (a.3 .0 - b.3 .0, a.3 .1 - b.3 .1);
                if dx * dx + dy * dy <= ITEM_MERGE_RADIUS * ITEM_MERGE_RADIUS {
                    consumed[i] = true;
                    consumed[j] = true;
                    merges.push((a.0, b.0, a.1, total as u16, a.3, a.4, a.5.min(b.5)));
                    break;
                }
            }
        }
        for (id_a, id_b, item, count, center, vel, spawn_tick) in merges {
            self.despawn_entity(id_a, DespawnReason::Despawned);
            self.despawn_entity(id_b, DespawnReason::Despawned);
            self.spawn_item_drop_exact(item, count, center, vel, spawn_tick);
        }
    }

    /// Armed drops are auto-collected by the nearest player within
    /// [`ITEM_PICKUP_RADIUS`] with inventory room. First (= nearest) pickup
    /// wins (§11); a partial fit re-spawns the remainder.
    fn pickup_item_drops(&mut self) {
        let arm_ticks = (ITEM_PICKUP_ARM_SECS * TICK_RATE as f32) as u64;
        let candidates: Vec<(u32, ItemId, u16, (f32, f32), u64)> = self
            .entities
            .map
            .iter()
            .filter(|(_, e)| self.tick.saturating_sub(e.spawn_tick) >= arm_ticks)
            .map(|(&id, e)| {
                let EntityKind::ItemDrop { item, count } = e.kind;
                (id, item, count, e.center(), e.spawn_tick)
            })
            .collect();
        for (entity_id, item, count, center, spawn_tick) in candidates {
            // Nearest player in range, ties broken by id for determinism.
            let mut best: Option<(f32, u32)> = None;
            for (&pid, p) in &self.players {
                let d = aabb_point_distance(
                    p.pos,
                    (
                        ferraria_shared::physics::PLAYER_WIDTH,
                        ferraria_shared::physics::PLAYER_HEIGHT,
                    ),
                    center,
                );
                if d <= ITEM_PICKUP_RADIUS
                    && best.is_none_or(|(bd, bid)| d < bd || (d == bd && pid < bid))
                {
                    best = Some((d, pid));
                }
            }
            let Some((_, pid)) = best else {
                continue;
            };
            let Some(p) = self.players.get_mut(&pid) else {
                continue;
            };
            let (added, changed) = add_to_inventory(&mut p.inventory, item, count);
            if added == 0 {
                continue; // no room; the drop stays for someone else
            }
            let slots: Vec<(u8, Option<ferraria_shared::items::InvSlot>)> = changed
                .into_iter()
                .map(|i| (i as u8, p.inventory[i]))
                .collect();
            let held_changed = slots.iter().any(|&(i, _)| i == p.held_slot);
            for (idx, stack) in slots {
                self.send_to(pid, &ServerMessage::SlotChanged { idx, stack });
            }
            if held_changed {
                self.broadcast_held_item(pid);
            }
            self.entities.map.remove(&entity_id);
            self.broadcast(&ServerMessage::ItemPickedUp {
                id: entity_id,
                by: pid,
            });
            if added < count {
                self.spawn_item_drop_exact(item, count - added, center, (0.0, 0.0), spawn_tick);
            }
        }
    }

    /// Snapshot batch (every 3 ticks): awake entities only, filtered per
    /// player to their chunk window.
    pub(crate) fn broadcast_entity_updates(&mut self) {
        let states: Vec<((u32, u32), EntityState)> = self
            .entities
            .map
            .iter()
            .filter(|(_, e)| e.awake)
            .map(|(&id, e)| {
                (
                    e.chunk(),
                    EntityState {
                        id,
                        pos: e.pos,
                        vel: e.vel,
                        hp: None,
                        state: 0,
                    },
                )
            })
            .collect();
        if !states.is_empty() {
            let ids: Vec<u32> = self.players.keys().copied().collect();
            for pid in ids {
                let Some(p) = self.players.get(&pid) else {
                    continue;
                };
                let entities: Vec<EntityState> = states
                    .iter()
                    .filter(|(c, _)| p.chunks.contains(c))
                    .map(|&(_, s)| s)
                    .collect();
                if !entities.is_empty() {
                    self.send_to(pid, &ServerMessage::EntityUpdate { entities });
                }
            }
        }
        for e in self.entities.map.values_mut() {
            e.awake = false;
        }
    }
}

fn touches_liquid(world: &ferraria_shared::world::World, e: &Entity, kind: LiquidKind) -> bool {
    let (w, h) = e.size();
    let (x0, y0) = (e.pos.0.floor() as i32, e.pos.1.floor() as i32);
    let (x1, y1) = (
        (e.pos.0 + w).floor() as i32,
        (e.pos.1 + h).floor() as i32,
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            if world.liquid(x, y).kind() == Some(kind) {
                return true;
            }
        }
    }
    false
}
