//! Deterministic scheduling fixture with an explicit, versioned state encoding.
use crate::{
    LearningError, LearningState, Scheduler, SchedulerMemory, SchedulerSettings, SchedulerUpdate,
    calendar::add_days,
};
use tempo_types::{Grade, StudyTime};

/// Use this while wiring the ports or when tests need predictable due dates.
/// Good schedules `good_interval_days` ahead; Again schedules tomorrow. The
/// default Good interval is three days. This is a fixture, not an FSRS model.
///
/// Algorithm identity is `fixed_interval`, schema 1. Settings encode the interval
/// as four little-endian bytes; memory encodes a review count as eight little-endian
/// bytes. A scheduling event increments that count. StandardLearning decides when
/// an event actually reaches this scheduler, so practice and fresh same-day repeats skip it.
#[derive(Clone, Copy, Debug)]
pub struct FixedIntervalScheduler {
    /// Positive number of days from an eligible Good to the next review.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LearningPolicy;
    use crate::test_support::{at, human, policy};
    use tempo_types::StudyDay;
    #[test]
    fn scheduled_reviews_preserve_and_advance_the_algorithm_memory() {
        let first = policy()
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        let second = policy()
            .apply(&first.state, &human(Grade::Good, 13, 0))
            .unwrap();
        assert_eq!(
            first.state.scheduler_memory.unwrap().payload,
            1_u64.to_le_bytes()
        );
        assert_eq!(
            second.state.scheduler_memory.unwrap().payload,
            2_u64.to_le_bytes()
        );
        assert_eq!(second.state.due, Some(StudyDay(16)));
    }

    #[test]
    fn invalid_intervals_and_unrepresentable_due_days_fail_explicitly() {
        let invalid = FixedIntervalScheduler {
            good_interval_days: 0,
        };
        assert_eq!(
            invalid.schedule(&LearningState::default(), Grade::Good, at(10)),
            Err(LearningError::InvalidInterval)
        );
        let late = StudyTime {
            day: StudyDay(i32::MAX),
            unix_ms: 0,
        };
        assert_eq!(
            FixedIntervalScheduler::default().schedule(
                &LearningState::default(),
                Grade::Good,
                late
            ),
            Err(LearningError::DayOverflow)
        );
    }
}
