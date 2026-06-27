//! Fuzzer for [`parking_game`] puzzles. This is meant as an exercise for learning how to use
//! LibAFL, and potentially not great for "real" applications, if they exist.

pub mod executor;
pub mod feedbacks;
pub mod input;
pub mod mutators;
pub mod observers;
pub mod stages;

use crate::input::PGInput;
use libafl::corpus::{Corpus, InMemoryCorpus};
use libafl::events::SimpleEventManager;
use libafl::feedbacks::{CombinedFeedback, CrashFeedback, MaxMapFeedback, NewHashFeedback};
use libafl::monitors::{self, SimpleMonitor};
use libafl::schedulers::QueueScheduler;
use libafl::stages::StdMutationalStage;
use libafl::state::{HasSolutions, StdState};
use libafl::{
    Evaluator, Fuzzer, StdFuzzer, feedback_and, feedback_and_fast, feedback_not, feedback_or,
};
use libafl_bolts::rands::StdRand;
use libafl_bolts::tuples::tuple_list;
use parking_game::{BoardValue, Car, Orientation, Position, State};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::error::Error;
use std::fmt::Debug;
use std::iter::Map;
use std::{env, fs};

use self::feedbacks::{CrashRateFeedback, SolvedFeedback};
use self::mutators::PGRandMutator;
use self::observers::{FinalStateObserver, ViewObserver};

/// Parses a map with the following rules:
/// 1. Empty spaces are denoted with `.`.
/// 2. The car which must be moved to the objective is referenced with `o`. This will be index 1.
/// 3. All other cars are uniquely named. They will be indexed in lexicographical order.
/// 4. All cars are at least length 2.
///
/// Any map not following this pattern is not guaranteed to be parsed correctly.
fn parse_map<T>(map: &str) -> State<T>
where
    T: BoardValue + TryFrom<usize>,
    T::Error: Debug,
{
    let map = map.trim_ascii();
    let rows = map.lines().count().try_into().unwrap();
    let cols = map.lines().next().unwrap().len().try_into().unwrap();

    let mut cars: HashMap<char, (Position<T>, Orientation, T)> = HashMap::new();

    let mut prev = None;
    for (ridx, row) in map.lines().enumerate() {
        let ridx = ridx.try_into().unwrap();
        let row = row.trim_ascii();
        for (cidx, col) in row.chars().enumerate() {
            let cidx: T = cidx.try_into().unwrap();
            match (prev, col) {
                (Some(car), next) => {
                    match cars.entry(car) {
                        Entry::Occupied(mut e) => {
                            let entry = e.get_mut();
                            assert!(
                                entry.0.row() == &ridx || entry.0.column() == &(cidx - T::one())
                            );
                            entry.2 += T::one();
                        }
                        Entry::Vacant(e) => {
                            if car == next {
                                // same car: we are in the same row, init with left-right
                                e.insert((
                                    (ridx, cidx - T::one()).into(),
                                    Orientation::LeftRight,
                                    T::one(),
                                ));
                            } else {
                                // different car: different row, init with up-down
                                e.insert((
                                    (ridx, cidx - T::one()).into(),
                                    Orientation::UpDown,
                                    T::one(),
                                ));
                            }
                        }
                    }
                    prev = if next == '.' { None } else { Some(next) };
                }
                (None, '.') => {
                    // do nothing
                }
                (None, next) => {
                    prev = Some(next);
                }
            }
        }

        if let Some(car) = prev.take() {
            match cars.entry(car) {
                Entry::Occupied(mut e) => {
                    let entry = e.get_mut();
                    assert!(entry.0.row() == &ridx || entry.0.column() == &(cols - T::one()));
                    entry.2 += T::one();
                }
                Entry::Vacant(e) => {
                    // this has to be up-down orientation: we haven't seen it earlier
                    e.insert((
                        (ridx, cols - T::one()).into(),
                        Orientation::UpDown,
                        T::one(),
                    ));
                }
            }
        }
    }

    let mut state = State::empty((rows, cols)).unwrap();
    let mut inserted = Vec::new();
    inserted.push(('o', cars.remove(&'o').unwrap()));
    inserted.extend(cars);
    inserted[1..].sort_by_key(|(name, _)| *name); // lexographical sort

    let mut board = state.board_mut().unwrap();
    for (_name, (position, orientation, len)) in inserted {
        board
            .add_car(position, Car::new(len, orientation).unwrap())
            .unwrap();
    }
    drop(board);

    state
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args_os()
        .nth(1)
        .expect("Provide the path to the desired map.");
    // adjust u8 to u16 as necessary
    // for the maps in `maps/`, you only need u8; for larger maps, you may need to increase this
    // maps with side lengths >255 are not supported (also: where did you get them? :D)
    let mut init = parse_map::<u8>(&fs::read_to_string(path).unwrap());

    println!("Attempting to solve:");
    println!("{}", init.board().unwrap());

    // (pt.1): create a ViewObserver with ViewObserver::<u8>::default()
    // this creates a view observer for a map which is indexed by u8s
    let view_observer = ViewObserver::<u8>::default();

    // (pt.1): create a FinalStateObserver with its default method for a map indexed by u8s
    let final_state_observer = FinalStateObserver::<u8>::default();

    // (pt.1): create a feedback which will add an entry to the corpus if we see a new state
    //  - this feedback should first check that the target has **not** "crashed"
    //  - iff so, we should check if this is a newly observed state by checking its hash
    //    - hint: look at https://docs.rs/libafl/latest/libafl/feedbacks/index.html
    //      - is there a feedback which checks for new hashes?
    //    - hint: check https://docs.rs/libafl/latest/libafl/index.html#macros for combining feedbacks
    //    - hint: check https://github.com/AFLplusplus/LibAFL/tree/main/fuzzers for examples
    // (pt.1): after implementing CrashRateFeedback, add it here at an appropriate place
    //  - you should see a failure rate of >80% for tokyo1.map, >95% for tokyo36.map
    //  - hint: consider the order of the feedback evaluation; where would be best to put this?
    // TODO(pt.2): make the feedback compatible with PGTailMutator
    //  - for the tail mutator to work, we need to stash the view data
    //  - what feedback does this? how do we combine it with the existing feedbacks?
    // TODO(pt.3): make the feedback compatible with snapshot fuzzing
    //  - the tail mutator makes re-executing the input redundant for prefix of moves
    //  - what feedback stashes the final state? how do we combine it with the existing feedbacks?
    let new_state_feedback = NewHashFeedback::new(&final_state_observer);
    let valid_new_state = feedback_and_fast!(feedback_not!(CrashFeedback::new()), new_state_feedback);
    let mut feedback = feedback_or!(
        CrashRateFeedback,
        valid_new_state,
    );

    // (pt.1): create an objective which will determine if the puzzle is solved
    //  - this feedback should first check that the target has **not** "crashed"
    //  - then, we should check if the puzzle is solved
    //    - hint: this is mostly the same as setting up the feedback
    let mut objective = feedback_and_fast!(
        feedback_not!(CrashFeedback::new()),
        SolvedFeedback::new(&view_observer)
    );

    // sets up the state and storage for preserved inputs and the solutions
    let mut state = StdState::new(
        StdRand::new(),
        InMemoryCorpus::<PGInput>::new(),
        InMemoryCorpus::new(),
        &mut feedback,
        &mut objective,
    )?;

    // (pt.1): create a PGRandMutator with &init
    let mutator = PGRandMutator::new(&init);
    // TODO(pt.2): replace it with a PGTailMutator

    // (pt.1): create an executor and pass your observers to it
    //  - provide the view and final state observers
    //  - hint: in LibAFL, lists of differing types are created with the `tuple_list` macro
    //    - extra: what does this macro do?
    //    - extra: why do we format lists of data of different types like this?
    let mut executor =
        executor::PGExecutor::new(init, tuple_list!(view_observer, final_state_observer));

    // (pt.1): create a fuzzer which uses a queue scheduler and the provided feedback/objective
    //  - see: https://docs.rs/libafl/latest/libafl/fuzzer/struct.StdFuzzer.html
    //  - extra: could we make a better scheduler for this?
    let scheduler = QueueScheduler::new();
    let mut fuzzer = StdFuzzer::new(scheduler, feedback, objective);

    // (pt.1): create a list of stages to be used by the fuzzer
    //  - for this fuzzer, we only need one stage: one that mutates and executes the input
    //  - hint: look at https://docs.rs/libafl/latest/libafl/stages/index.html
    //    - is there a (concrete) type which does this? which is suitable for our use case?
    //  - hint: the stages are of differing types; how do we construct this for LibAFL?
    let mut stages = tuple_list!(StdMutationalStage::new(mutator));
    // (pt.1): simple printing manager; you can use alternatives if you want to try them out!
    let monitor = SimpleMonitor::new(|s| println!("{s}"));
    let mut manager = SimpleEventManager::new(monitor);

    // (pt.1): evaluate an input with no moves
    //  - for the mutator to work correctly, we need an existing input!
    //  - evaluating an input will add it to the corpus and all relevant metadata for us
    //  - see: https://docs.rs/libafl/latest/libafl/fuzzer/trait.Evaluator.html
    //    - what variable from earlier implements this?
    //  - hint: how do we make an input with no moves?
    let initial_input = PGInput::default(); // input with no moves
    fuzzer.evaluate_input(&mut state, &mut executor, &mut manager, &initial_input);

    // (pt.1): loop and fuzz until we have a solution
    //  - we don't need to fuzz forever; just until we find an input that gets the puzzle solved
    //  - hint: how do we access the solutions in the state?
    //  - hint: how do we know if there are any solutions?
    //  - hint: what fuzz method would be most appropriate?
    //    - see: https://docs.rs/libafl/latest/libafl/fuzzer/trait.Fuzzer.html
    while state.solutions().count() == 0 {
        fuzzer.fuzz_one(
            &mut stages, 
            &mut executor,
            &mut state, 
            &mut manager
        )?;
    }

    // get the last input and print out the moves!
    let idx = state
        .solutions()
        .last()
        .expect("Should have had a solution!");
    let tc = state.solutions().get(idx).unwrap().borrow();
    let moves = tc.input().as_ref().unwrap().moves();
    println!("{} moves: {:?}", moves.len(), moves);

    Ok(())
}
