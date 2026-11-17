//! Daily-credit, repeat, and freshness rules around an injected scheduler.
use crate::{
    AssessmentInput, AssessmentSource, FixedIntervalScheduler, LearningEffect, LearningError,
    LearningPolicy, LearningState, Scheduler, SchedulerMemory, SchedulerSettings, Transition,
    calendar::add_days,
};
use tempo_types::{Grade, MissRevision};

/// Use this as the ordinary learning policy. Supply a [`Scheduler`] to choose
/// interval/model behavior while retaining the following acceptance rules:
///
/// - Again records a miss immediately, increments miss revision, and makes a repeat
///   pending with a due date of tomorrow, even if the target was not due.
/// - A human Good must match the current miss revision. Old successes remain stale
///   after repeats clear. External Good is rejected.
/// - A fresh same-day repeat clears the obligation without scheduler advancement.
/// - An eligible clean Good earns at most one positive scheduling credit per day;
///   other clean successes are practice.
///
/// Call through [`LearningPolicy`]. Its `validate_state` operation also supports
/// restore without fabricating an assessment. All calls are pure proposals;
/// application acceptance owns atomicity and event replay protection.
#[derive(Clone, Debug, Default)]
pub struct StandardLearning<S = FixedIntervalScheduler> {
    /// Algorithm used only when the daily policy approves a scheduling event.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{human, policy};
    use tempo_types::StudyDay;
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
}
