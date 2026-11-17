//! Pure learning transitions. The application owns acceptance, replay, and persistence.
use serde::{Deserialize, Serialize};
use std::fmt;
use tempo_types::{AttemptId, EventId, Grade, MissRevision, StudyDay, StudyTime};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningState {
    pub introduced: bool,
    pub due: Option<StudyDay>,
    pub repeat_pending: bool,
    pub last_credit_day: Option<StudyDay>,
    pub last_miss_day: Option<StudyDay>,
    /// Monotonic even after a successful repeat; old attempts remain identifiable.
    pub miss_revision: MissRevision,
    /// Number of accepted schedule-changing assessments, not the algorithm model.
    pub scheduled_reviews: u64,
    pub scheduler_memory: Option<SchedulerMemory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssessmentSource {
    Human {
        attempt_id: AttemptId,
        issued_miss_revision: MissRevision,
    },
    External {
        provider: String,
        event_id: EventId,
        observed_at_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssessmentInput {
    pub grade: Grade,
    pub accepted_at: StudyTime,
    pub source: AssessmentSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningEffect {
    MissRecorded,
    RepeatCleared,
    ScheduleAdvanced,
    PracticeOnly,
    StalePositiveIgnored,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub state: LearningState,
    pub effect: LearningEffect,
}

/// Opaque only at this narrow scheduler boundary; the selected algorithm validates it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerMemory {
    pub algorithm: String,
    pub schema_version: u32,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerSettings {
    pub algorithm: String,
    pub schema_version: u32,
    pub parameters: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerUpdate {
    pub due: StudyDay,
    pub memory: SchedulerMemory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LearningError {
    InvalidInterval,
    CounterOverflow,
    DayOverflow,
    AcceptanceBeforePriorLearning,
    InvalidRepeatState,
    ExternalGoodUnsupported,
    IncompatibleSchedulerMemory,
    MalformedSchedulerMemory,
    InvalidSchedule,
    InvalidLearningState,
}
impl fmt::Display for LearningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "learning contract: {self:?}")
    }
}
impl std::error::Error for LearningError {}

pub trait Scheduler {
    fn settings(&self) -> SchedulerSettings;
    fn validate_memory(&self, memory: &SchedulerMemory) -> Result<(), LearningError>;
    fn schedule(
        &self,
        prior: &LearningState,
        grade: Grade,
        accepted_at: StudyTime,
    ) -> Result<SchedulerUpdate, LearningError>;
}

pub trait LearningPolicy {
    fn settings(&self) -> SchedulerSettings;
    fn validate_state(&self, state: &LearningState) -> Result<(), LearningError>;
    fn apply(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError>;
}

/// Deliberately simple, deterministic scheduler used to exercise the port.
#[derive(Clone, Copy, Debug)]
pub struct FixedIntervalScheduler {
    pub good_interval_days: u32,
}
impl Default for FixedIntervalScheduler {
    fn default() -> Self {
        Self {
            good_interval_days: 3,
        }
    }
}
impl Scheduler for FixedIntervalScheduler {
    fn settings(&self) -> SchedulerSettings {
        SchedulerSettings {
            algorithm: "fixed_interval".into(),
            schema_version: 1,
            parameters: self.good_interval_days.to_le_bytes().to_vec(),
        }
    }
    fn validate_memory(&self, memory: &SchedulerMemory) -> Result<(), LearningError> {
        if memory.algorithm != "fixed_interval" || memory.schema_version != 1 {
            return Err(LearningError::IncompatibleSchedulerMemory);
        }
        if memory.payload.len() != 8 {
            return Err(LearningError::MalformedSchedulerMemory);
        }
        Ok(())
    }
    fn schedule(
        &self,
        prior: &LearningState,
        grade: Grade,
        accepted_at: StudyTime,
    ) -> Result<SchedulerUpdate, LearningError> {
        if self.good_interval_days == 0 {
            return Err(LearningError::InvalidInterval);
        }
        let reviews = match &prior.scheduler_memory {
            Some(memory) => {
                self.validate_memory(memory)?;
                u64::from_le_bytes(
                    memory
                        .payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| LearningError::MalformedSchedulerMemory)?,
                )
            }
            None => 0,
        };
        let reviews = reviews
            .checked_add(1)
            .ok_or(LearningError::CounterOverflow)?;
        let days = match grade {
            Grade::Good => self.good_interval_days,
            Grade::Again => 1,
        };
        Ok(SchedulerUpdate {
            due: add_days(accepted_at.day, days)?,
            memory: SchedulerMemory {
                algorithm: "fixed_interval".into(),
                schema_version: 1,
                payload: reviews.to_le_bytes().to_vec(),
            },
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct StandardLearning<S = FixedIntervalScheduler> {
    pub scheduler: S,
}
impl<S: Scheduler> LearningPolicy for StandardLearning<S> {
    fn settings(&self) -> SchedulerSettings {
        self.scheduler.settings()
    }
    fn validate_state(&self, state: &LearningState) -> Result<(), LearningError> {
        if !state.introduced
            && (state.due.is_some()
                || state.repeat_pending
                || state.last_credit_day.is_some()
                || state.last_miss_day.is_some()
                || state.miss_revision.0 != 0
                || state.scheduled_reviews != 0
                || state.scheduler_memory.is_some())
        {
            return Err(LearningError::InvalidLearningState);
        }
        if state.repeat_pending && (!state.introduced || state.last_miss_day.is_none()) {
            return Err(LearningError::InvalidRepeatState);
        }
        if state.introduced && state.due.is_none() {
            return Err(LearningError::InvalidLearningState);
        }
        if state.last_miss_day.is_some() != (state.miss_revision.0 > 0) {
            return Err(LearningError::InvalidLearningState);
        }
        if state.scheduled_reviews > 0 && state.scheduler_memory.is_none() {
            return Err(LearningError::InvalidLearningState);
        }
        if let Some(memory) = &state.scheduler_memory {
            validate_memory_identity(&self.scheduler, memory)?;
        }
        Ok(())
    }
    fn apply(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError> {
        self.validate_state(prior)?;
        let today = input.accepted_at.day;
        if prior.last_credit_day.is_some_and(|day| day > today)
            || prior.last_miss_day.is_some_and(|day| day > today)
        {
            return Err(LearningError::AcceptanceBeforePriorLearning);
        }
        if prior.repeat_pending && (!prior.introduced || prior.last_miss_day.is_none()) {
            return Err(LearningError::InvalidRepeatState);
        }
        let mut state = prior.clone();
        if input.grade == Grade::Again {
            let update = self
                .scheduler
                .schedule(prior, Grade::Again, input.accepted_at)?;
            validate_memory_identity(&self.scheduler, &update.memory)?;
            state.scheduler_memory = Some(update.memory);
            state.introduced = true;
            // Daily retry semantics are learning policy, not scheduler discretion.
            state.due = Some(add_days(today, 1)?);
            state.repeat_pending = true;
            state.last_miss_day = Some(today);
            state.miss_revision = MissRevision(
                prior
                    .miss_revision
                    .0
                    .checked_add(1)
                    .ok_or(LearningError::CounterOverflow)?,
            );
            state.scheduled_reviews = prior
                .scheduled_reviews
                .checked_add(1)
                .ok_or(LearningError::CounterOverflow)?;
            return Ok(Transition {
                state,
                effect: LearningEffect::MissRecorded,
            });
        }
        match &input.source {
            AssessmentSource::Human {
                issued_miss_revision,
                ..
            } => {
                if *issued_miss_revision != prior.miss_revision {
                    return Ok(Transition {
                        state,
                        effect: LearningEffect::StalePositiveIgnored,
                    });
                }
            }
            AssessmentSource::External { .. } => {
                return Err(LearningError::ExternalGoodUnsupported);
            }
        }
        if prior.repeat_pending && prior.last_miss_day == Some(today) {
            state.repeat_pending = false;
            state.due = Some(add_days(today, 1)?);
            return Ok(Transition {
                state,
                effect: LearningEffect::RepeatCleared,
            });
        }
        let due = !prior.introduced || prior.due.is_some_and(|day| day <= today);
        if due && prior.last_credit_day != Some(today) {
            let update = self
                .scheduler
                .schedule(prior, Grade::Good, input.accepted_at)?;
            validate_memory_identity(&self.scheduler, &update.memory)?;
            if update.due <= today {
                return Err(LearningError::InvalidSchedule);
            }
            state.introduced = true;
            state.due = Some(update.due);
            state.scheduler_memory = Some(update.memory);
            state.repeat_pending = false;
            state.last_credit_day = Some(today);
            state.scheduled_reviews = prior
                .scheduled_reviews
                .checked_add(1)
                .ok_or(LearningError::CounterOverflow)?;
            return Ok(Transition {
                state,
                effect: LearningEffect::ScheduleAdvanced,
            });
        }
        if prior.repeat_pending {
            state.repeat_pending = false;
            return Ok(Transition {
                state,
                effect: LearningEffect::RepeatCleared,
            });
        }
        Ok(Transition {
            state,
            effect: LearningEffect::PracticeOnly,
        })
    }
}

fn validate_memory_identity<S: Scheduler>(
    scheduler: &S,
    memory: &SchedulerMemory,
) -> Result<(), LearningError> {
    if memory.algorithm != scheduler.settings().algorithm {
        return Err(LearningError::IncompatibleSchedulerMemory);
    }
    scheduler.validate_memory(memory)
}

fn add_days(day: StudyDay, count: u32) -> Result<StudyDay, LearningError> {
    let count = i32::try_from(count).map_err(|_| LearningError::DayOverflow)?;
    day.0
        .checked_add(count)
        .map(StudyDay)
        .ok_or(LearningError::DayOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i32) -> StudyTime {
        StudyTime {
            day: StudyDay(day),
            unix_ms: (day as u64) * 86_400_000,
        }
    }
    fn human(grade: Grade, day: i32, revision: u64) -> AssessmentInput {
        AssessmentInput {
            grade,
            accepted_at: at(day),
            source: AssessmentSource::Human {
                attempt_id: "attempt".into(),
                issued_miss_revision: MissRevision(revision),
            },
        }
    }
    fn policy() -> StandardLearning {
        StandardLearning::default()
    }

    #[test]
    fn due_success_and_new_success_each_earn_one_daily_credit() {
        let due = LearningState {
            introduced: true,
            due: Some(StudyDay(10)),
            ..LearningState::default()
        };
        for prior in [LearningState::default(), due] {
            let result = policy().apply(&prior, &human(Grade::Good, 10, 0)).unwrap();
            assert_eq!(result.effect, LearningEffect::ScheduleAdvanced);
            assert_eq!(result.state.due, Some(StudyDay(13)));
            assert_eq!(result.state.last_credit_day, Some(StudyDay(10)));
            assert!(result.state.introduced);
            let repeat = policy()
                .apply(&result.state, &human(Grade::Good, 10, 0))
                .unwrap();
            assert_eq!(repeat.effect, LearningEffect::PracticeOnly);
            assert_eq!(repeat.state, result.state);
        }
    }

    #[test]
    fn nondue_miss_is_again_and_fresh_repeat_does_not_extend_interval() {
        let prior = LearningState {
            introduced: true,
            due: Some(StudyDay(40)),
            ..LearningState::default()
        };
        let missed = policy().apply(&prior, &human(Grade::Again, 10, 0)).unwrap();
        assert_eq!(missed.effect, LearningEffect::MissRecorded);
        assert_eq!(missed.state.due, Some(StudyDay(11)));
        assert!(missed.state.repeat_pending);
        let repeated = policy()
            .apply(&missed.state, &human(Grade::Good, 10, 1))
            .unwrap();
        assert_eq!(repeated.effect, LearningEffect::RepeatCleared);
        assert_eq!(repeated.state.due, Some(StudyDay(11)));
        assert!(!repeated.state.repeat_pending);
        assert_eq!(repeated.state.miss_revision, MissRevision(1));
        assert_eq!(
            repeated.state.scheduled_reviews,
            missed.state.scheduled_reviews
        );
    }

    #[test]
    fn stale_positive_stays_stale_after_repeat_cleared_and_day_changes() {
        let missed = policy()
            .apply(&LearningState::default(), &human(Grade::Again, 10, 0))
            .unwrap()
            .state;
        let fresh = policy()
            .apply(&missed, &human(Grade::Good, 10, 1))
            .unwrap()
            .state;
        let stale = policy().apply(&fresh, &human(Grade::Good, 11, 0)).unwrap();
        assert_eq!(stale.effect, LearningEffect::StalePositiveIgnored);
        assert_eq!(stale.state, fresh);
        let current = policy().apply(&fresh, &human(Grade::Good, 11, 1)).unwrap();
        assert_eq!(current.effect, LearningEffect::ScheduleAdvanced);
        assert_eq!(current.state.due, Some(StudyDay(14)));
    }

    #[test]
    fn external_again_uses_acceptance_day_not_old_observation() {
        let input = AssessmentInput {
            grade: Grade::Again,
            accepted_at: at(10),
            source: AssessmentSource::External {
                provider: "fixture".into(),
                event_id: "game-1".into(),
                observed_at_ms: 0,
            },
        };
        let result = policy().apply(&LearningState::default(), &input).unwrap();
        assert_eq!(result.state.due, Some(StudyDay(11)));
        assert_eq!(result.state.last_miss_day, Some(StudyDay(10)));
    }

    #[test]
    fn scheduler_is_replaceable_without_changing_learning_port() {
        let short = StandardLearning {
            scheduler: FixedIntervalScheduler {
                good_interval_days: 2,
            },
        };
        let long = StandardLearning {
            scheduler: FixedIntervalScheduler {
                good_interval_days: 7,
            },
        };
        let policies: [&dyn LearningPolicy; 2] = [&short, &long];
        let expected = [StudyDay(12), StudyDay(17)];
        for (policy, expected) in policies.into_iter().zip(expected) {
            let result = policy
                .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
                .unwrap();
            assert_eq!(result.state.due, Some(expected));
        }
        assert_ne!(short.settings(), long.settings());
    }

    #[test]
    fn overflow_and_time_travel_return_errors_without_changing_input() {
        let prior = LearningState {
            introduced: true,
            due: Some(StudyDay(11)),
            last_miss_day: Some(StudyDay(10)),
            miss_revision: MissRevision(u64::MAX),
            ..LearningState::default()
        };
        assert_eq!(
            policy().apply(&prior, &human(Grade::Again, 10, u64::MAX)),
            Err(LearningError::CounterOverflow)
        );
        assert_eq!(prior.miss_revision, MissRevision(u64::MAX));
        let future = LearningState {
            introduced: true,
            due: Some(StudyDay(15)),
            last_credit_day: Some(StudyDay(12)),
            ..LearningState::default()
        };
        assert_eq!(
            policy().apply(&future, &human(Grade::Good, 10, 0)),
            Err(LearningError::AcceptanceBeforePriorLearning)
        );
    }
    #[test]
    fn memory_schema_and_algorithm_mismatches_are_errors_not_resets() {
        let first = policy()
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        assert_eq!(
            first.state.scheduler_memory.as_ref().unwrap().payload,
            1u64.to_le_bytes()
        );
        for (algorithm, schema, payload, expected) in [
            (
                "other",
                1,
                vec![0; 8],
                LearningError::IncompatibleSchedulerMemory,
            ),
            (
                "fixed_interval",
                2,
                vec![0; 8],
                LearningError::IncompatibleSchedulerMemory,
            ),
            (
                "fixed_interval",
                1,
                vec![0; 3],
                LearningError::MalformedSchedulerMemory,
            ),
        ] {
            let mut prior = first.state.clone();
            prior.scheduler_memory = Some(SchedulerMemory {
                algorithm: algorithm.into(),
                schema_version: schema,
                payload,
            });
            assert_eq!(
                policy().apply(&prior, &human(Grade::Good, 13, 0)),
                Err(expected)
            );
        }
    }

    #[test]
    fn fresh_repeat_preserves_scheduler_memory_and_settings_are_versioned() {
        let missed = policy()
            .apply(&LearningState::default(), &human(Grade::Again, 10, 0))
            .unwrap()
            .state;
        let cleared = policy()
            .apply(&missed, &human(Grade::Good, 10, 1))
            .unwrap()
            .state;
        assert_eq!(missed.scheduler_memory, cleared.scheduler_memory);
        let settings = policy().settings();
        assert_eq!(settings.algorithm, "fixed_interval");
        assert_eq!(settings.schema_version, 1);
        assert_eq!(settings.parameters, 3u32.to_le_bytes());
    }
    #[test]
    fn restore_rejects_introduced_targets_without_due_dates_and_incoherent_miss_stamps() {
        let unscheduled = LearningState {
            introduced: true,
            ..LearningState::default()
        };
        assert_eq!(
            policy().validate_state(&unscheduled),
            Err(LearningError::InvalidLearningState)
        );
        let lost_miss = LearningState {
            introduced: true,
            due: Some(StudyDay(11)),
            last_miss_day: Some(StudyDay(10)),
            ..LearningState::default()
        };
        assert_eq!(
            policy().validate_state(&lost_miss),
            Err(LearningError::InvalidLearningState)
        );
        assert_eq!(policy().validate_state(&LearningState::default()), Ok(()));
    }
}
