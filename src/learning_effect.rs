//! The policy meaning of an assessment, distinct from its grade.
use serde::{Deserialize, Serialize};

/// Read this from [`crate::Transition`] to explain what happened to learning.
/// The same Good grade can advance a schedule, clear a repeat, count as practice,
/// or be ignored as stale. Persist the returned state; do not infer it from this
/// label or treat every Good as another scheduling credit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearningEffect {
    /// Again introduced/updated the target and created a repeat obligation.
    MissRecorded,
    /// A fresh successful attempt cleared a repeat without advancing the scheduler.
    RepeatCleared,
    /// An eligible success earned a daily credit and installed a scheduler update.
    ScheduleAdvanced,
    /// Success earned no additional scheduling credit; learning state is unchanged.
    PracticeOnly,
    /// The attempt predates the current miss revision; its success changes no state.
    StalePositiveIgnored,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{human, policy};
    use crate::{LearningPolicy, LearningState};
    use tempo_types::Grade;
    #[test]
    fn two_clean_attempts_on_one_day_can_have_different_learning_effects() {
        let first = policy()
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        let extra_practice = policy()
            .apply(&first.state, &human(Grade::Good, 10, 0))
            .unwrap();
        let label = |effect| match effect {
            LearningEffect::ScheduleAdvanced => "Scheduled review earned",
            LearningEffect::PracticeOnly => "Additional practice",
            _ => "Other learning effect",
        };
        assert_eq!(label(first.effect), "Scheduled review earned");
        assert_eq!(label(extra_practice.effect), "Additional practice");
        assert_eq!(extra_practice.state, first.state);
    }
}
