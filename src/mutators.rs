//! Mutators for [`PGInput`]s -- so you can fuzz [`parking_game`] puzzles!

use crate::feedbacks::{CrashRateMetadata, ViewMetadata};
use crate::input::PGInput;
use crate::observers::ViewFrom;
use libafl::corpus::{CorpusId, Testcase};
use libafl::mutators::{MutationResult, Mutator};
use libafl::state::{HasCurrentTestcase, HasRand};
use libafl::{Error, HasMetadata};
use libafl_bolts::Named;
use libafl_bolts::rands::Rand;
use libafl_bolts::tuples::{Append, tuple_list};
use parking_game::{BoardValue, Direction, State};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::borrow::Cow;
use std::fs::metadata;
use std::marker::PhantomData;
use std::num::NonZeroUsize;

/// Randomly mutate the moves -- at any point with anything.
///
/// (pt.1): explain PGRandMutator's weaknesses in a comment.
/// rand mutator's weakness is that it's putting moves at random valid idx but it's not changing the moves
/// or removing any moves at existing idx. Also adding moves without checking validity, can
/// invalidate the input.
pub struct PGRandMutator<T> {
    count: usize,
    phantom: PhantomData<T>,
}

impl<T> PGRandMutator<T> {
    /// Construct a [`PGRandMutator`] for the given state.
    pub fn new(state: &State<T>) -> Self {
        Self {
            count: state.cars().len(),
            phantom: PhantomData,
        }
    }
}

impl<T> Named for PGRandMutator<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_rand");
        &NAME
    }
}

impl<S, T> Mutator<PGInput, S> for PGRandMutator<T>
where
    S: HasRand + HasCurrentTestcase<PGInput>,
    T: BoardValue + DeserializeOwned + Serialize + 'static,
{
    fn mutate(&mut self, state: &mut S, input: &mut PGInput) -> Result<MutationResult, Error> {
        // select a random car
        // because of the formatting of the car numbering, this is a little clunky
        // I've done this for you because this is my fault :)
        let car = NonZeroUsize::new(
            state
                .rand_mut()
                .below(NonZeroUsize::new(self.count).unwrap())
                + 1,
        )
        .unwrap();

        // insert a random move at a random position
        //  - first, pick a random index in the moves using `state.rand_mut().below(...)`
        //  - second, pick a random direction using `state.rand_mut().choose(...)`
        //  - finally, insert the (car, direction) tuple at the generated index
        let num_positions = input.moves().len() + 1;
        let rand_idx = state
            .rand_mut()
            .below(NonZeroUsize::new(num_positions).unwrap());

        let directions = &[
            Direction::Up,
            Direction::Down,
            Direction::Left,
            Direction::Right,
        ];
        let rand_dir: Direction = state.rand_mut().choose(directions).cloned().unwrap();

        input.moves_mut().insert(rand_idx, (car, rand_dir));

        Ok(MutationResult::Mutated)
    }

    fn post_exec(&mut self, _state: &mut S, _new_corpus_id: Option<CorpusId>) -> Result<(), Error> {
        // nothing to do?
        Ok(())
    }
}

/// Mutator which adds a _valid_ move to the end of the sequence. Only valid when used as the only
/// mutator and when [`crate::feedbacks::ViewMetadata`] is available on the mutated testcase.
pub struct PGTailMutator<T> {
    phantom: PhantomData<T>,
}

impl<T> PGTailMutator<T> {
    /// Create a new mutator for the provided state.
    pub fn new(_state: &State<T>) -> Self {
        Self {
            phantom: PhantomData,
        }
    }
}

impl<T> Named for PGTailMutator<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_tail");
        &NAME
    }
}

impl<S, T> Mutator<PGInput, S> for PGTailMutator<T>
where
    S: HasRand + HasCurrentTestcase<PGInput>,
    T: BoardValue + DeserializeOwned + Serialize + 'static,
{
    fn mutate(&mut self, state: &mut S, input: &mut PGInput) -> Result<MutationResult, Error> {
        // (pt.2): build a tail mutator which only utilizes valid mutations
        //  - first, get the current testcase and extract the metadata for its views
        //  - second, build a list of choices for mutation
        //    - this should include each possible direction of movement for each car at each
        //      possible distance (remember both forward and backward!)
        //    - hint: `T` is generic, but you can check if it is zero with `.is_zero()` and
        //      decrement it with `-= T::one()`
        //      - remember not to mutate the metadata in place! this will affect future iterations
        //    - `drop(...)` the testcase after use so that you can mutably use the state again
        //  - finally, select from this list randomly with `state.rand_mut().choose(...)` and apply
        //    the mutation with `.push()` (potentially multiple times for `T > 1`)
        let current_testcase = state.current_testcase()?;
        let metadata = current_testcase.metadata::<ViewMetadata<T>>()?;
        let mut choices: Vec<(NonZeroUsize, Direction, T)> = Vec::new();
        for (car, view_from) in metadata.views() {
            for view in [view_from.backward(), view_from.forward()] {
                let mut distance = *view.distance();

                while !distance.is_zero() {
                    choices.push((car, view.direction(), distance));
                    distance -= T::one();
                }
            }
        }
        drop(current_testcase); // drop it so we can mutate state

        if choices.is_empty() {
            return Ok(MutationResult::Skipped);
        }

        let &(car, direction, mut distance) = state.rand_mut().choose(&choices).unwrap();

        while !distance.is_zero() {
            input.moves_mut().push((car, direction));
            distance -= T::one();
        }
        Ok(MutationResult::Mutated)
    }

    fn post_exec(&mut self, _state: &mut S, _new_corpus_id: Option<CorpusId>) -> Result<(), Error> {
        // nothing to do?
        Ok(())
    }
}
