//! The application-facing boundary for learning decisions and restore validation.
use crate::{AssessmentInput, LearningError, LearningState, SchedulerSettings, Transition};

/// Depend on this trait when application code needs learning behavior. It accepts
/// domain values and returns proposed values, keeping persistence and replay
/// protection in the application's acceptance transaction.
///
/// The trait is object-safe, so composition may hold `Box<dyn LearningPolicy>`.
/// [`crate::StandardLearning`] supplies daily rules with a replaceable scheduler;
/// application commands do not need to know the concrete algorithm type.
pub trait LearningPolicy {
    /// Export the active scheduler configuration for exact restore compatibility checks.
    fn settings(&self) -> SchedulerSettings;
    /// Check a restored state and its algorithm memory without applying a fake grade.
    fn validate_state(&self, state: &LearningState) -> Result<(), LearningError>;
    /// Propose a transition without mutating prior state or recording acceptance.
    /// Duplicate event protection belongs to the surrounding application transaction.
    fn apply(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::human;
    use crate::{FixedIntervalScheduler, StandardLearning};
    use tempo_types::{Grade, StudyDay};
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
}
