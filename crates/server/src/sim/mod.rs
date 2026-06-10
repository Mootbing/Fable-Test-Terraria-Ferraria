//! Server-side simulation systems: the 60 tps game loop ([`game`]), the
//! fluid automaton ([`fluids`]), the mine/place/build intents ([`interact`]),
//! the dropped-item entities ([`entities`]), and the live world ticks —
//! fluids cadence, falling sand, grass/sapling random ticks
//! ([`world_tick`]). Enemies, combat, and the rest of the tick pipeline
//! arrive with later PRs and plug into [`game::Sim::tick`].

pub mod entities;
pub mod fluids;
pub mod game;
pub mod interact;
pub mod world_tick;
