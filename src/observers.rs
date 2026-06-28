//! Observers which collect data from [`crate::executor::PGExecutor`] executions.

use crate::input::PGInput;
use libafl::observers::{Observer, ObserverWithHashField};
use libafl_bolts::{Error, Named};
use parking_game::{Board, BoardValue, Direction, Orientation, Position, State};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::num::NonZeroUsize;
use std::ops::Deref;

/// An [`Observer`] compatible with [`crate::executor::PGExecutor`].
pub trait PGObserver<T> {
    /// Passes the final board state to the observer, called after [`Observer::pre_exec`] and before
    /// [`Observer::post_exec`] if the execution completes normally. Does nothing by default so this
    /// may be implemented easily for existing observer types.
    #[allow(unused_variables)]
    fn final_board(&mut self, board: &Board<impl Deref<Target = State<T>>, T>) {
        // do nothing
    }
}

/// Utility trait for marking [`libafl_bolts::tuples::tuple_list`]s as "all PG observers".
///
/// This is one of the ways that LibAFL ensures that different components of the fuzzer is
/// compatible. When we have many components of differing types that we want to put together, we
/// encode them in a _tuple list_, like `(a, (b, (c, ())))`. When encoded like this, we can check
/// that the [generic bounds](https://doc.rust-lang.org/rust-by-example/generics/bounds.html) of
/// every item in the list are upheld. This trait makes it possible to pass a board to all observers
/// in a tuple list -- so long as all of `a`, `b`, and `c` all implement [`PGObserver`].
pub trait PGObserverTuple<T> {
    /// Iterate all boards contained here and pass the provided board.
    fn final_board_all(&mut self, board: &Board<impl Deref<Target = State<T>>, T>);
}

impl<T> PGObserverTuple<T> for () {
    fn final_board_all(&mut self, _board: &Board<impl Deref<Target = State<T>>, T>) {
        // this is the end of the list, so we're done
    }
}

// Remember: the list looks like `(a, (b, (c, ())))`.
// This effectively iterates over `a`, `b`, and `c`, executing their final_board implementations.
impl<T, Head, Tail> PGObserverTuple<T> for (Head, Tail)
where
    Head: PGObserver<T>,
    Tail: PGObserverTuple<T>,
{
    fn final_board_all(&mut self, board: &Board<impl Deref<Target = State<T>>, T>) {
        self.0.final_board(board);
        self.1.final_board_all(board);
    }
}

/// Observer which stashes the final state of the board after an execution.
#[derive(Debug, Deserialize, Serialize)]
pub struct FinalStateObserver<T> {
    final_state: Option<State<T>>,
}

impl<T> FinalStateObserver<T> {
    /// The final state observed -- if it exists (which, it will not if there is an error!).
    pub fn final_state(&self) -> Option<&State<T>> {
        self.final_state.as_ref()
    }
}

impl<T> Default for FinalStateObserver<T> {
    fn default() -> Self {
        Self { final_state: None }
    }
}

impl<T> Named for FinalStateObserver<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_final_state");
        &NAME
    }
}

impl<S, T> Observer<PGInput, S> for FinalStateObserver<T> {
    fn flush(&mut self) -> Result<(), Error> {
        self.final_state = None;
        Ok(())
    }

    fn pre_exec(&mut self, _state: &mut S, _input: &PGInput) -> Result<(), Error> {
        self.final_state = None;
        Ok(())
    }
}

impl<T> PGObserver<T> for FinalStateObserver<T>
where
    T: Clone,
{
    fn final_board(&mut self, board: &Board<impl Deref<Target = State<T>>, T>) {
        self.final_state = Some(board.state().clone());
    }
}

impl<T> ObserverWithHashField for FinalStateObserver<T>
where
    T: Hash,
{
    fn hash(&self) -> Option<u64> {
        if let Some(final_state) = &self.final_state {
            let mut hasher = DefaultHasher::new();
            // build a hash which uniquely identifies the state
            //  - remember, not all parts of the state need to be hashed to identify it uniquely
            //  - only hash the parts which are necessary to distinguish the states
            final_state.hash(&mut hasher);
            Some(hasher.finish())
        } else {
            None
        }
    }
}

/// View from a car in a potential direction of travel. Useful for knowing where a car can move.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize, Serialize)]
pub struct View<T> {
    direction: Direction,
    observed: Option<NonZeroUsize>,
    distance: T,
}

impl<T> View<T> {
    /// Create a new [`View`] in the given direction.
    pub fn new(direction: Direction, observed: Option<NonZeroUsize>, distance: T) -> Self {
        Self {
            direction,
            observed,
            distance,
        }
    }

    /// The direction of this view.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The car observed in this view, or [`None`] if we don't see a car (i.e. the closest thing is
    /// a wall).
    pub fn observed(&self) -> Option<NonZeroUsize> {
        self.observed
    }

    /// The distance from us to the obstacle (in terms of how many moves we can make before
    /// colliding with the obstacle).
    pub fn distance(&self) -> &T {
        &self.distance
    }

    /// The distance, but mutable.
    pub fn distance_mut(&mut self) -> &mut T {
        &mut self.distance
    }
}

/// The view from a car, forward and backward.
#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
pub struct ViewFrom<T> {
    backward: View<T>,
    forward: View<T>,
}

impl<T> ViewFrom<T> {
    /// The view behind the car (if the car is oriented [`Orientation::LeftRight`], the view will
    /// have [`Direction::Left`]; otherwise, it will be [`Direction::Up`].
    pub fn backward(&self) -> &View<T> {
        &self.backward
    }

    /// The view ahead of the car (if the car is oriented [`Orientation::LeftRight`], the view will
    /// have [`Direction::Right`]; otherwise, it will be [`Direction::Down`].
    pub fn forward(&self) -> &View<T> {
        &self.forward
    }
}

/// An observer which collects [`View`] information for each car.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ViewObserver<T> {
    views: Vec<ViewFrom<T>>,
}

impl<T> ViewObserver<T> {
    /// An iterator over the views. The objective car will be the first.
    pub fn views(&self) -> impl Iterator<Item = (NonZeroUsize, &ViewFrom<T>)> {
        self.views
            .iter()
            .enumerate()
            .map(|(i, e)| (NonZeroUsize::new(i + 1).unwrap(), e))
    }
}

impl<T> Named for ViewObserver<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_view");
        &NAME
    }
}

impl<S, T> Observer<PGInput, S> for ViewObserver<T> {
    fn flush(&mut self) -> Result<(), Error> {
        self.views.clear();
        Ok(())
    }

    fn pre_exec(&mut self, _state: &mut S, _input: &PGInput) -> Result<(), Error> {
        self.views.clear();
        Ok(())
    }
}

/// Returns the number of units that the car in this position could potentially move in the
/// provided direction.
fn step_until_seen<T: BoardValue>(
    board: &Board<impl Deref<Target = State<T>>, T>,
    from: Position<T>,
    direction: Direction,
) -> View<T> {
    // this is our car, and not an obstacle!
    let car = board.get(from).unwrap().unwrap();
    let mut offset = match direction {
        Direction::Up | Direction::Left => T::one(),
        Direction::Down | Direction::Right => *board.state().cars()[car.get() - 1].1.length(),
    };
    let mut distance = T::zero();
    // find the obstacle first encountered in the direction provided
    //  - hint: you can use `position.shift(...)` to get a position at a given offset
    //    - check return values for both `position.shift(...)` and `board.get(...)` for gotchas
    //  - hint: you can increment offset with `offset += T::one()`, likewise with distance
    //  - hint: an obstacle directly adjacent should be considered as zero units away
    //  - this method is _extensively_ tested in simple_observation
    loop {
        let Some(new_position) = from.shift(direction, offset) else {
            return View::new(direction, None, distance);
        };

        match board.get(new_position) {
            None => {
                // outside board
                return View::new(direction, None, distance);
            }

            Some(None) => {
                // square is empty
                offset += T::one();
                distance += T::one();
            }

            Some(Some(car_idx)) => {
                // some car has occupied this
                return View::new(direction, Some(car_idx), distance);
            }
        }
    }
}

impl<T> PGObserver<T> for ViewObserver<T>
where
    T: BoardValue,
{
    fn final_board(&mut self, board: &Board<impl Deref<Target = State<T>>, T>) {
        for (position, car) in board.state().cars().iter().copied() {
            let backward = match car.orientation() {
                Orientation::UpDown => Direction::Up,
                Orientation::LeftRight => Direction::Left,
            };

            let forward = step_until_seen(board, position, -backward);
            let backward = step_until_seen(board, position, backward);

            self.views.push(ViewFrom { backward, forward });
        }
    }
}

#[cfg(test)]
mod test {
    use crate::input::PGInput;
    use crate::observers::{FinalStateObserver, PGObserverTuple, View, ViewObserver};
    use libafl::executors::ExitKind;
    use libafl::observers::{ObserverWithHashField, ObserversTuple};
    use libafl::state::NopState;
    use libafl_bolts::tuples::{Handled, tuple_list};
    use parking_game::Direction;
    use std::error::Error;
    use std::num::NonZeroUsize;

    #[test]
    fn simple_observation() -> Result<(), Box<dyn Error>> {
        let initial = crate::parse_map::<u8>("33oo22.");
        let obs = ViewObserver::<u8>::default();
        let handle = obs.handle();

        let mut observers = tuple_list!(obs);

        let mut state = NopState::<PGInput>::new();

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert_eq!(
            observers.0.views().next().unwrap().1.backward,
            View {
                direction: Direction::Left,
                observed: NonZeroUsize::new(3),
                distance: 0
            }
        );
        assert_eq!(
            observers.0.views().next().unwrap().1.forward,
            View {
                direction: Direction::Right,
                observed: NonZeroUsize::new(2),
                distance: 0
            }
        );

        let initial = crate::parse_map::<u8>("oo.");

        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert_eq!(
            observers.0.views().next().unwrap().1.backward,
            View {
                direction: Direction::Left,
                observed: None,
                distance: 0
            }
        );
        assert_eq!(
            observers.0.views().next().unwrap().1.forward,
            View {
                direction: Direction::Right,
                observed: None,
                distance: 1
            }
        );

        let initial = crate::parse_map::<u8>(
            r#"
            3
            3
            o
            o
            2
            2
            .
            "#,
        );

        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert_eq!(
            observers.0.views().next().unwrap().1.backward,
            View {
                direction: Direction::Up,
                observed: NonZeroUsize::new(3),
                distance: 0
            }
        );
        assert_eq!(
            observers.0.views().next().unwrap().1.forward,
            View {
                direction: Direction::Down,
                observed: NonZeroUsize::new(2),
                distance: 0
            }
        );

        let initial = crate::parse_map::<u8>(
            r#"
            o
            o
            .
            "#,
        );

        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert_eq!(
            observers.0.views().next().unwrap().1.backward,
            View {
                direction: Direction::Up,
                observed: None,
                distance: 0
            }
        );
        assert_eq!(
            observers.0.views().next().unwrap().1.forward,
            View {
                direction: Direction::Down,
                observed: None,
                distance: 1
            }
        );

        Ok(())
    }

    #[test]
    fn distinguish_states() -> Result<(), Box<dyn Error>> {
        let initial = crate::parse_map::<u8>("33oo22.");
        let obs = FinalStateObserver::<u8>::default();
        let handle = obs.handle();

        let mut observers = tuple_list!(obs);

        let mut state = NopState::<PGInput>::new();

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        let first_hash = observers.0.hash().unwrap();

        // same as above, but 2 is shifted right
        let initial = crate::parse_map::<u8>("33oo.22");

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        let second_hash = observers.0.hash().unwrap();

        assert_ne!(first_hash, second_hash);

        Ok(())
    }
}
