//! Executor implementation for [`parking_game`] puzzles.

use crate::input::PGInput;
use crate::observers::PGObserverTuple;
use libafl::executors::{Executor, ExitKind, HasObservers};
use libafl::state::{HasCurrentTestcase, HasExecutions};
use libafl_bolts::Error;
use libafl_bolts::tuples::RefIndexable;
use parking_game::{BoardValue, State};

/// Executor which advances the state by "running" the move sequence provided.
pub struct PGExecutor<T, OT> {
    initial: State<T>,
    observers: OT,
}

impl<T, OT> PGExecutor<T, OT> {
    /// Create a new executor for the provided state with the provided observers.
    pub fn new(initial: State<T>, observers: OT) -> Self {
        Self { initial, observers }
    }
}

impl<T, OT> PGExecutor<T, OT> {
    /// The initial state which this executor is going to advance.
    pub fn initial(&self) -> &State<T> {
        &self.initial
    }
}

// This allows other components to interact with the executors observers, when necessary.
impl<T, OT> HasObservers for PGExecutor<T, OT> {
    type Observers = OT;

    fn observers(&self) -> RefIndexable<&Self::Observers, Self::Observers> {
        RefIndexable::from(&self.observers)
    }

    fn observers_mut(&mut self) -> RefIndexable<&mut Self::Observers, Self::Observers> {
        RefIndexable::from(&mut self.observers)
    }
}

impl<EM, OT, S, T, Z> Executor<EM, PGInput, S, Z> for PGExecutor<T, OT>
where
    OT: PGObserverTuple<T>,
    S: HasExecutions + HasCurrentTestcase<PGInput>,
    T: BoardValue,
{
    fn run_target(
        &mut self,
        _fuzzer: &mut Z,
        state: &mut S,
        _mgr: &mut EM,
        input: &PGInput,
    ) -> Result<ExitKind, Error> {
        // first: increment the executions for tracking how many times we've run so far
        *state.executions_mut() += 1;

        // this state is game state
        let (mut state, moves) = (|| {
            // this is a closure which allows us to do better control flow
            // you can `return` values in this block to assign them to the variables above
            // clippy will complain about this for now, but you'll need it later

            // TODO(pt.3): load the snapshot from the testcase so we don't have to replay moves
            //  - how do we access the snapshot?
            //    - hint: we mutated from the current testcase in the state
            //    - hint: how do we access the metadata describing the snapshot from the testcase?
            //      - hint: you may need a turbofish operator: https://turbo.fish/
            //  - make sure to check that the snapshot is valid
            //    - the prefix of moves are the same
            //    - the returned sequence of moves is after that prefix (use the slice operator)

            // create a local copy of the initial instance and get the moves we're about to apply
            Ok::<_, Error>((self.initial.clone(), input.moves()))
        })()?;
        // load the game board from the state, or return an error if there's something wrong
        let mut board = state
            .board_mut()
            .map_err(|e| Error::illegal_state(e.to_string()))?;

        // apply the moves in sequence
        //  - check the docs for how to apply moves to a board
        //    - see: https://docs.rs/parking-game/latest/parking_game/struct.Board.html
        //  - if an error occurs during a move, return `Ok(ExitKind::Crash)`.
        for &(car, dir) in input.moves() {
            if board.shift_car(car, dir).is_err() {
                return Ok(ExitKind::Crash);
            }
        }
        // TODO(pt.3): add a microsecond delay *after each move* to simulate cost:
        // sleep(Duration::from_micros(1));

        // send the final board to all the observers
        self.observers.final_board_all(&board);

        // indicate successful execution
        Ok(ExitKind::Ok)
    }
}

#[cfg(test)]
mod test {
    use crate::executor::PGExecutor;
    use crate::input::PGInput;
    use crate::observers::FinalStateObserver;
    use libafl::NopFuzzer;
    use libafl::events::SimpleEventManager;
    use libafl::executors::{Executor, ExitKind, HasObservers};
    use libafl::observers::ObserversTuple;
    use libafl::state::NopState;
    use libafl_bolts::tuples::tuple_list;
    use parking_game::Direction;
    use std::error::Error;
    use std::num::NonZeroUsize;

    #[test]
    fn simple_run_check() -> Result<(), Box<dyn Error>> {
        // initialize and parse map
        let initial = crate::parse_map::<u8>(
            r#"
        oo.
        .22
        "#,
        );
        let mut executor =
            PGExecutor::new(initial, tuple_list!(FinalStateObserver::<u8>::default()));

        let mut fuzzer = NopFuzzer::new();
        let mut state = NopState::<PGInput>::new();
        let mut mgr = SimpleEventManager::<PGInput, _, NopState<PGInput>>::printing();

        let first = PGInput::new(vec![(NonZeroUsize::new(1).unwrap(), Direction::Right)]);
        executor.observers_mut().pre_exec_all(&mut state, &first)?;
        let kind = executor.run_target(&mut fuzzer, &mut state, &mut mgr, &first)?;
        executor
            .observers_mut()
            .post_exec_all(&mut state, &first, &kind)?;

        assert_eq!(ExitKind::Ok, kind);
        assert_eq!(
            1,
            *executor.observers.0.final_state().as_ref().unwrap().cars()[0]
                .0
                .column()
        );

        let second = PGInput::new(vec![(NonZeroUsize::new(1).unwrap(), Direction::Down)]);
        executor.observers_mut().pre_exec_all(&mut state, &second)?;
        let kind = executor.run_target(&mut fuzzer, &mut state, &mut mgr, &second)?;
        executor
            .observers_mut()
            .post_exec_all(&mut state, &second, &kind)?;

        assert_eq!(ExitKind::Crash, kind);
        assert!(executor.observers.0.final_state().is_none());

        Ok(())
    }
}
