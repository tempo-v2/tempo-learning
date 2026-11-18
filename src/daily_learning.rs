//! Opening lessons and scheduled tests use distinct learning transitions.
use crate::{
    AssessmentInput, AssessmentSource, LearningEffect, LearningError, LearningPolicy,
    LearningState, SchedulerMemory, SchedulerSettings, Transition, calendar::add_days,
};
use fsrs::{DEFAULT_PARAMETERS, FSRS, MemoryState};
use tempo_types::{ContentVersion, Grade, StudyTime};

const ALGORITHM: &str = "fsrs-6.6.2";
const MEMORY_FORMAT: u32 = 1;
const RETENTION: f32 = 0.90;
const MAX_INTERVAL_DAYS: u32 = 36_500;

#[derive(Default)]
pub struct DailyLearning {
    model: FSRS,
}

impl DailyLearning {
    /// Acknowledge a new or revised answer and schedule its first blind test.
    pub fn teach(prior: &LearningState, at: StudyTime) -> Result<LearningState, LearningError> {
        Self::introduce_ungraded(prior, at, add_days(at.day, 1)?)
    }

    /// Entering a personal tactic is an ungraded introduction with immediate recall due.
    pub fn introduce_capture(
        prior: &LearningState,
        at: StudyTime,
    ) -> Result<LearningState, LearningError> {
        Self::introduce_ungraded(prior, at, at.day)
    }

    fn introduce_ungraded(
        prior: &LearningState,
        at: StudyTime,
        due: tempo_types::StudyDay,
    ) -> Result<LearningState, LearningError> {
        if prior.introduced || prior.taught_at.is_some() || prior.scheduler_memory.is_some() {
            return Err(LearningError::InvalidLearningState);
        }
        check_time(prior, at)?;
        let mut state = prior.clone();
        state.introduced = true;
        state.first_introduced.get_or_insert(at.day);
        state.taught_at = Some(at);
        state.game_teaching_relevance = false;
        state.due = Some(due);
        Ok(state)
    }

    /// Keep lifetime admission when a different answer supersedes this version.
    pub fn changed_answer(prior: &LearningState) -> LearningState {
        LearningState {
            first_introduced: prior.first_introduced,
            last_graded_day: prior
                .last_graded_day
                .or(prior.last_review_at.map(|at| at.day)),
            ..LearningState::default()
        }
    }

    /// Accept the first assessment of an ordinary daily Test using its saved issue time.
    pub fn assess_test(
        policy: &dyn LearningPolicy,
        prior: &LearningState,
        input: &AssessmentInput,
        version: ContentVersion,
        issued_at: StudyTime,
    ) -> Result<Transition, LearningError> {
        if !prior.can_accept_daily_test(input.accepted_at.day) {
            return Err(LearningError::DailyTestAlreadyAccepted);
        }
        let mut transition = policy.apply(prior, input)?;
        transition
            .state
            .record_assessed_test(version, issued_at, input.accepted_at)?;
        Ok(transition)
    }
}

impl LearningPolicy for DailyLearning {
    fn settings(&self) -> SchedulerSettings {
        SchedulerSettings {
            algorithm: ALGORITHM.into(),
            schema_version: MEMORY_FORMAT,
            parameters: RETENTION
                .to_le_bytes()
                .into_iter()
                .chain(MAX_INTERVAL_DAYS.to_le_bytes())
                .chain(
                    DEFAULT_PARAMETERS
                        .iter()
                        .flat_map(|value| value.to_le_bytes()),
                )
                .collect(),
        }
    }

    fn validate_state(&self, state: &LearningState) -> Result<(), LearningError> {
        let reviewed = state.last_review_at.is_some();
        if state.introduced != state.due.is_some()
            || state.introduced != (reviewed || state.taught_at.is_some())
            || (state.introduced && state.first_introduced.is_none())
            || reviewed != (state.scheduled_reviews != 0)
            || reviewed != state.scheduler_memory.is_some()
            || (state.game_teaching_relevance && state.introduced)
            || state.last_assessed_test_issue_ms.is_some_and(|issue| {
                state
                    .last_review_at
                    .is_none_or(|review| issue > review.unix_ms)
            })
            || state
                .last_graded_day
                .zip(state.last_review_at)
                .is_some_and(|(day, review)| day != review.day)
            || state.game_review.is_some_and(|request| {
                state.taught_at.is_none_or(|taught| {
                    request.occurred_by_ms <= taught.unix_ms || request.actionable_day <= taught.day
                }) || state
                    .last_assessed_test_issue_ms
                    .is_some_and(|issue| issue > request.occurred_by_ms)
            })
            || state.repeat_pending
            || state.last_credit_day.is_some()
            || state.last_miss_day.is_some()
            || state.miss_revision.0 != 0
        {
            return Err(LearningError::InvalidLearningState);
        }
        if let Some(memory) = &state.scheduler_memory {
            decode_memory(memory)?;
        }
        Ok(())
    }

    fn apply(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError> {
        self.validate_state(prior)?;
        if !matches!(input.source, AssessmentSource::Human { .. }) {
            return Err(LearningError::InvalidLearningState);
        }
        let at = input.accepted_at;
        check_time(prior, at)?;
        let elapsed = prior
            .last_review_at
            .map_or(0, |last| at.day.0.abs_diff(last.day.0));
        let memory = prior
            .scheduler_memory
            .as_ref()
            .map(decode_memory)
            .transpose()?;
        let updates = self
            .model
            .next_states(memory, RETENTION, elapsed)
            .map_err(|_| LearningError::InvalidSchedule)?;
        let result = match input.grade {
            Grade::Again => updates.again,
            Grade::Good => updates.good,
        };
        if !result.interval.is_finite() || result.interval <= 0.0 {
            return Err(LearningError::InvalidSchedule);
        }
        let days = (result.interval.round().max(1.0) as u32).min(MAX_INTERVAL_DAYS);
        let mut state = prior.clone();
        state.introduced = true;
        state.first_introduced.get_or_insert(at.day);
        state.last_review_at = Some(at);
        state.due = Some(add_days(at.day, days)?);
        state.scheduled_reviews = prior
            .scheduled_reviews
            .checked_add(1)
            .ok_or(LearningError::CounterOverflow)?;
        state.scheduler_memory = Some(SchedulerMemory {
            algorithm: ALGORITHM.into(),
            schema_version: MEMORY_FORMAT,
            payload: result
                .memory
                .stability
                .to_le_bytes()
                .into_iter()
                .chain(result.memory.difficulty.to_le_bytes())
                .collect(),
        });
        Ok(Transition {
            state,
            effect: LearningEffect::ScheduleAdvanced,
        })
    }
}

fn check_time(prior: &LearningState, at: StudyTime) -> Result<(), LearningError> {
    if prior.first_introduced.is_some_and(|day| day > at.day)
        || [prior.taught_at, prior.last_review_at]
            .into_iter()
            .flatten()
            .any(|last| last.day > at.day || last.unix_ms > at.unix_ms)
    {
        return Err(LearningError::AcceptanceBeforePriorLearning);
    }
    Ok(())
}

fn decode_memory(memory: &SchedulerMemory) -> Result<MemoryState, LearningError> {
    if memory.algorithm != ALGORITHM || memory.schema_version != MEMORY_FORMAT {
        return Err(LearningError::IncompatibleSchedulerMemory);
    }
    let [a, b, c, d, e, f, g, h] = memory.payload.as_slice() else {
        return Err(LearningError::MalformedSchedulerMemory);
    };
    let stability = f32::from_le_bytes([*a, *b, *c, *d]);
    let difficulty = f32::from_le_bytes([*e, *f, *g, *h]);
    if !stability.is_finite() || stability <= 0.0 || !(1.0..=10.0).contains(&difficulty) {
        return Err(LearningError::MalformedSchedulerMemory);
    }
    Ok(MemoryState {
        stability,
        difficulty,
    })
}
