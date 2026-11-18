//! Everything Learning needs from one accepted performance assessment.
use crate::AssessmentSource;
use serde::{Deserialize, Serialize};
use tempo_types::{Grade, StudyTime};

/// Build this after Study produces a terminal assessment or the application
/// accepts programmatic Again. Pass it with prior state to
/// [`crate::LearningPolicy::apply`]. It carries no chess answer, question content,
/// or storage handle: Learning interprets performance, not move correctness.
///
/// Acceptance time determines the scheduling day. For a delayed game scan,
/// source observation time remains provenance and does not backdate learning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssessmentInput {
    /// Semantic result supplied by the grader or accepted external feedback.
    pub grade: Grade,
    /// Explicit application acceptance time used for scheduling decisions.
    pub accepted_at: StudyTime,
    /// Provenance and, for human attempts, the captured freshness revision.
    pub source: AssessmentSource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{at, policy};
    use crate::{LearningPolicy, LearningState};
    use tempo_types::StudyDay;
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
}
