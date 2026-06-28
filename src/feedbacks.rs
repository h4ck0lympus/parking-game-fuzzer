//! Feedbacks which assist in the solving of [`parking_game`] puzzles by interpreting data collected
//! by the [`libafl::observers::Observer`] implementations in [`crate::observers`].

use crate::input::PGInput;
use crate::observers::{FinalStateObserver, ViewFrom, ViewObserver};
use libafl::HasMetadata;
use libafl::corpus::Testcase;
use libafl::events::{Event, EventFirer, EventWithStats, ExecStats};
use libafl::executors::ExitKind;
use libafl::feedbacks::{Feedback, StateInitializer};
use libafl::monitors::stats::{AggregatorOps, UserStats, UserStatsValue};
use libafl::state::HasExecutions;
use libafl_bolts::tuples::{Append, Handle, Handled, MatchNameRef};
use libafl_bolts::{Error, Named, current_time, impl_serdeany};
use parking_game::{BoardValue, State};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::marker::PhantomData;
use std::num::NonZeroUsize;

/// Feedback which works out how far away obstacles are from each car, and which obstacles they are.
pub struct ViewFeedback<T> {
    obs: Handle<ViewObserver<T>>,
}

impl<T> ViewFeedback<T> {
    /// Create a new [`ViewFeedback`] which will interpret from the provided [`ViewObserver`].
    pub fn new(obs: &ViewObserver<T>) -> Self {
        Self { obs: obs.handle() }
    }
}

impl<S, T> StateInitializer<S> for ViewFeedback<T> {}

impl<T> Named for ViewFeedback<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_view_fb");
        &NAME
    }
}

/// Metadata which retains the view info collected by [`ViewFeedback`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewMetadata<T> {
    views: Vec<ViewFrom<T>>,
}

impl<T> ViewMetadata<T> {
    /// An iterator over what's viewable by each car. The objective car will be the first entry.
    pub fn views(&self) -> impl Iterator<Item = (NonZeroUsize, &ViewFrom<T>)> {
        self.views
            .iter()
            .enumerate()
            .map(|(i, e)| (NonZeroUsize::new(i + 1).unwrap(), e))
    }
}

impl_serdeany!(ViewMetadata<T: BoardValue + DeserializeOwned + Serialize + 'static>, <u8>, <u16>);

impl<EM, OT, S, T> Feedback<EM, PGInput, OT, S> for ViewFeedback<T>
where
    OT: MatchNameRef,
    T: BoardValue + DeserializeOwned + Serialize + 'static,
{
    fn is_interesting(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        _input: &PGInput,
        _observers: &OT,
        _exit_kind: &ExitKind,
    ) -> Result<bool, Error> {
        // TODO smarter feedback?
        Ok(false)
    }

    fn append_metadata(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        observers: &OT,
        testcase: &mut Testcase<PGInput>,
    ) -> Result<(), Error> {
        let obs = observers.get(&self.obs).unwrap();
        testcase.add_metadata(ViewMetadata {
            views: obs.views().map(|(_, e)| e).copied().collect(),
        });
        Ok(())
    }
}

/// Feedback which stashes the final state of the board after execution by
/// [`crate::executor::PGExecutor`].
pub struct FinalStateFeedback<T> {
    obs: Handle<FinalStateObserver<T>>,
}

impl<T> FinalStateFeedback<T> {
    /// Create a new [`FinalStateFeedback`] which will collect the final state from the provided
    /// [`FinalStateObserver`].
    pub fn new(obs: &FinalStateObserver<T>) -> Self {
        Self { obs: obs.handle() }
    }
}

impl<S, T> StateInitializer<S> for FinalStateFeedback<T> {}

impl<T> Named for FinalStateFeedback<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_state_fb");
        &NAME
    }
}

/// Metadata which holds the final state of the board after an execution of the associated testcase.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FinalStateMetadata<T> {
    state: State<T>,
}

impl<T> FinalStateMetadata<T> {
    /// The state contained within this metadata.
    pub fn state(&self) -> &State<T> {
        &self.state
    }
}

impl_serdeany!(FinalStateMetadata<T: BoardValue + DeserializeOwned + Serialize + 'static>, <u8>, <u16>);

impl<EM, OT, S, T> Feedback<EM, PGInput, OT, S> for FinalStateFeedback<T>
where
    OT: MatchNameRef,
    T: BoardValue + DeserializeOwned + Serialize + 'static,
{
    fn is_interesting(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        _input: &PGInput,
        _observers: &OT,
        _exit_kind: &ExitKind,
    ) -> Result<bool, Error> {
        Ok(false)
    }

    fn append_metadata(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        observers: &OT,
        testcase: &mut Testcase<PGInput>,
    ) -> Result<(), Error> {
        let obs = observers.get(&self.obs).unwrap();
        testcase.add_metadata(FinalStateMetadata{
           state: obs.final_state().unwrap().clone()
        });
        Ok(())
    }
}

/// Feedback which interprets the view data from [`ViewObserver`] to determine if the board is in a
/// solved state (i.e., when the objective vehicle sees the wall).
pub struct SolvedFeedback<T> {
    obs: Handle<ViewObserver<T>>,
}

impl<T> SolvedFeedback<T> {
    /// Create a [`SolvedFeedback`] which will interpret the result from the [`ViewObserver`].
    pub fn new(obs: &ViewObserver<T>) -> Self {
        Self { obs: obs.handle() }
    }
}

impl<S, T> StateInitializer<S> for SolvedFeedback<T> {}

impl<T> Named for SolvedFeedback<T> {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_solved");
        &NAME
    }
}

impl<EM, OT, S, T> Feedback<EM, PGInput, OT, S> for SolvedFeedback<T>
where
    OT: MatchNameRef,
{
    fn is_interesting(
        &mut self,
        _state: &mut S,
        _manager: &mut EM,
        _input: &PGInput,
        observers: &OT,
        _exit_kind: &ExitKind,
    ) -> Result<bool, Error> {
        // "the objective car sees the wall ahead of it" (i.e., no car between us and wall)
        Ok(observers
            .get(&self.obs)
            .unwrap()
            .views()
            .next()
            .unwrap() // hint: crashed on this line? your feedback in main.rs is wrong!
            .1
            .forward()
            .observed()
            .is_none())
    }
}

/// Feedback which measures and reports the crash rate of the executor.
pub struct CrashRateFeedback;

/// Metadata which tracks the crash rate of the fuzzer.
///
/// (pt.1): add the necessary `#[derive(...)]` statements for metadata.S
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CrashRateMetadata {
    // (pt.1): what fields do we need to track in the metadata?
    //  - hint: do this while implementing CrashRateFeedback::is_interesting
    pub num_crashes: u64,
}

// (pt.1): other implementation details needed for the metadata
impl_serdeany!(CrashRateMetadata);

impl<S> StateInitializer<S> for CrashRateFeedback
where
    S: HasMetadata, // (pt.1): what traits do we require on the state (S)?
{
    fn init_state(&mut self, state: &mut S) -> Result<(), Error> {
        state.add_metadata(CrashRateMetadata::default());
        Ok(())
    }
}

impl Named for CrashRateFeedback {
    fn name(&self) -> &Cow<'static, str> {
        static NAME: Cow<'static, str> = Cow::Borrowed("pg_crashrate_fb");
        &NAME
    }
}

impl<EM, I, OT, S> Feedback<EM, I, OT, S> for CrashRateFeedback
where
    EM: EventFirer<I, S>,
    S: HasMetadata + HasExecutions, // (pt.1) what traits do we require on the state (S)?
{
    fn is_interesting(
        &mut self,
        state: &mut S,
        _manager: &mut EM,
        _input: &I,
        _observers: &OT,
        exit_kind: &ExitKind,
    ) -> Result<bool, Error> {
        // (pt.1) update the number of crashes observed so far
        //  - get a mutable reference to the CrashRateMetadata from the state
        //    - hint: you may need a turbofish: https://turbo.fish/
        //  - update the number of crashes in metadata so far by checking exit_kind

        let metadata = state.metadata_mut::<CrashRateMetadata>()?;
        match exit_kind {
            ExitKind::Crash => {
                metadata.num_crashes += 1;
            }
            _ => {}
        }
        Ok(false)
    }

    fn append_metadata(
        &mut self,
        state: &mut S,
        manager: &mut EM,
        _observers: &OT,
        _testcase: &mut Testcase<I>,
    ) -> Result<(), Error> {
        // (pt.1) get the crash metadata and execution counts
        let crashes = state.metadata::<CrashRateMetadata>().unwrap().num_crashes;
        let executions = *state.executions();

        manager.fire(
            state,
            EventWithStats::new(
                Event::UpdateUserStats {
                    name: self.name().clone(),
                    value: UserStats::new(
                        // todo!("(pt.1) report the ratio of crashes to executions"),
                        UserStatsValue::Ratio(crashes, executions),
                        AggregatorOps::Avg, // if aggregated, report the average number
                    ),
                    phantom: PhantomData,
                },
                ExecStats::new(current_time(), executions),
            ),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::feedbacks::SolvedFeedback;
    use crate::input::PGInput;
    use crate::observers::{PGObserverTuple, View, ViewObserver};
    use libafl::events::SimpleEventManager;
    use libafl::executors::ExitKind;
    use libafl::feedbacks::Feedback;
    use libafl::observers::ObserversTuple;
    use libafl::state::NopState;
    use libafl_bolts::tuples::tuple_list;
    use parking_game::Direction;
    use std::error::Error;

    #[test]
    fn simple_solved() -> Result<(), Box<dyn Error>> {
        let initial = crate::parse_map::<u8>("oo...");
        let obs = ViewObserver::<u8>::default();
        let mut solved = SolvedFeedback::new(&obs);

        let mut observers = tuple_list!(obs);

        let mut state = NopState::<PGInput>::new();
        let mut mgr = SimpleEventManager::<PGInput, _, NopState<PGInput>>::printing();

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert!(solved.is_interesting(
            &mut state,
            &mut mgr,
            &nop_input,
            &observers,
            &ExitKind::Ok
        )?);

        Ok(())
    }

    #[test]
    fn simple_unsolved() -> Result<(), Box<dyn Error>> {
        let initial = crate::parse_map::<u8>("oo11.");
        let obs = ViewObserver::<u8>::default();
        let mut solved = SolvedFeedback::new(&obs);

        let mut observers = tuple_list!(obs);

        let mut state = NopState::<PGInput>::new();
        let mut mgr = SimpleEventManager::<PGInput, _, NopState<PGInput>>::printing();

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        assert!(!solved.is_interesting(
            &mut state,
            &mut mgr,
            &nop_input,
            &observers,
            &ExitKind::Ok
        )?);

        Ok(())
    }

    #[test]
    fn example_observation() -> Result<(), Box<dyn Error>> {
        let initial = crate::parse_map::<u8>(
            r#"
        ......
        ......
        ...2oo
        .332.4
        .5.2.4
        .5.664
        "#,
        );
        let obs = ViewObserver::<u8>::default();
        let mut solved = SolvedFeedback::new(&obs);

        let mut observers = tuple_list!(obs);

        let mut state = NopState::<PGInput>::new();
        let mut mgr = SimpleEventManager::<PGInput, _, NopState<PGInput>>::printing();

        let nop_input = PGInput::new(vec![]);
        observers.pre_exec_all(&mut state, &nop_input)?;
        observers.final_board_all(&initial.board()?);
        observers.post_exec_all(&mut state, &nop_input, &ExitKind::Ok)?;

        let (_car, seen) = observers.0.views().next().unwrap();
        assert_eq!(*seen.forward(), View::new(Direction::Right, None, 0));

        assert!(solved.is_interesting(
            &mut state,
            &mut mgr,
            &nop_input,
            &observers,
            &ExitKind::Ok
        )?);

        Ok(())
    }
}
