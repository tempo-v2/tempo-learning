//! Replace the scheduling algorithm without replacing daily learning rules.
use crate::{LearningError, LearningState, SchedulerMemory, SchedulerSettings, SchedulerUpdate};
use tempo_types::{Grade, StudyTime};

/// Implement this when adding a scheduling algorithm, then inject it into
/// [`crate::StandardLearning`]. The policy decides *whether* an assessment should
/// change the schedule; this port proposes a due day and updated model memory.
/// Normal application code calls [`crate::LearningPolicy`], not this lower-level port.
///
/// Keep configuration and memory deterministic for the same supplied inputs.
/// Validate your own algorithm/schema and decode only your own payload. Return
/// errors for incompatibility or malformed bytes; do not silently reset the model.
/// The scheduler receives no clock, storage handle, content, or queue.
pub trait Scheduler {
    /// Describe the configuration required to interpret/export compatible learning.
    fn settings(&self) -> SchedulerSettings;
    /// Validate an existing model envelope without inventing an assessment.
    fn validate_memory(&self, memory: &SchedulerMemory) -> Result<(), LearningError>;
    /// Propose a date and new memory for one policy-approved scheduling event.
    /// Good must land after acceptance day; Learning enforces its own Again due rule.
    fn schedule(
        &self,
        prior: &LearningState,
        grade: Grade,
        accepted_at: StudyTime,
    ) -> Result<SchedulerUpdate, LearningError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::human;
    use crate::{LearningEffect, LearningPolicy, StandardLearning};
    use tempo_types::StudyDay;
    // A stateless example algorithm: its versioned memory payload is empty.
    struct WeeklyScheduler;

    impl Scheduler for WeeklyScheduler {
        fn settings(&self) -> SchedulerSettings {
            SchedulerSettings {
                algorithm: "example_weekly".into(),
                schema_version: 1,
                parameters: vec![],
            }
        }

        fn validate_memory(&self, memory: &SchedulerMemory) -> Result<(), LearningError> {
            if memory.algorithm != "example_weekly" || memory.schema_version != 1 {
                return Err(LearningError::IncompatibleSchedulerMemory);
            }
            if !memory.payload.is_empty() {
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
            if let Some(memory) = &prior.scheduler_memory {
                self.validate_memory(memory)?;
            }
            let days = match grade {
                Grade::Good => 7,
                Grade::Again => 1,
            };
            let day = accepted_at
                .day
                .0
                .checked_add(days)
                .ok_or(LearningError::DayOverflow)?;
            Ok(SchedulerUpdate {
                due: StudyDay(day),
                memory: SchedulerMemory {
                    algorithm: "example_weekly".into(),
                    schema_version: 1,
                    payload: vec![],
                },
            })
        }
    }

    #[test]
    fn a_new_algorithm_reuses_the_existing_daily_credit_and_repeat_rules() {
        let policy = StandardLearning {
            scheduler: WeeklyScheduler,
        };
        let first = policy
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        assert_eq!(first.state.due, Some(StudyDay(17)));
        assert_eq!(
            first.state.scheduler_memory.as_ref().unwrap().algorithm,
            "example_weekly"
        );
        let practice = policy
            .apply(&first.state, &human(Grade::Good, 10, 0))
            .unwrap();
        assert_eq!(practice.effect, LearningEffect::PracticeOnly);
        let miss = policy
            .apply(&first.state, &human(Grade::Again, 10, 0))
            .unwrap();
        assert_eq!(miss.state.due, Some(StudyDay(11)));
        assert!(miss.state.repeat_pending);
        let repeat = policy
            .apply(&miss.state, &human(Grade::Good, 10, 1))
            .unwrap();
        assert_eq!(repeat.effect, LearningEffect::RepeatCleared);
        assert_eq!(repeat.state.due, Some(StudyDay(11)));
    }
}
