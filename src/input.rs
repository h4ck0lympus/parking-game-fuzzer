//! Input representations for fuzzing of [`parking_game`] puzzles.

use libafl::inputs::Input;
use parking_game::Direction;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

/// An input for solving the parking game problems.
#[derive(Debug, Default, Clone, Hash, Deserialize, Serialize)]
pub struct PGInput {
    moves: Vec<(NonZeroUsize, Direction)>,
}

// input format - ((car1, dir1), (car, dir) ... ) 
impl PGInput {
    /// Create a new [`PGInput`] from the provided sequence of moves.
    pub fn new(moves: Vec<(NonZeroUsize, Direction)>) -> Self {
        Self { moves }
    }

    /// The moves contained within this inputs.
    ///
    /// This is stored as a sequence of pairs of (1) car that is moved and (2) which direction.
    pub fn moves(&self) -> &[(NonZeroUsize, Direction)] {
        &self.moves
    }

    /// A mutable reference to the sequence of moves in this input, for use in mutators.
    pub fn moves_mut(&mut self) -> &mut Vec<(NonZeroUsize, Direction)> {
        &mut self.moves
    }
}

// Make it compatible with LibAFL!
impl Input for PGInput {}
