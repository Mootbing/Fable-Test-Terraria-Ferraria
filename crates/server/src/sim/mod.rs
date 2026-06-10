//! Server-side simulation systems: the 60 tps game loop ([`game`]) and the
//! fluid automaton ([`fluids`]). Enemies, combat, and the rest of the tick
//! pipeline arrive with later PRs and plug into [`game::Sim::tick`].

pub mod fluids;
pub mod game;
