//! Durable learning facts for one target, independent of its content family.
use crate::{Correction, GameReview, SchedulerMemory};
use serde::{Deserialize, Serialize};
use tempo_types::{MissRevision, StudyDay, StudyTime};

/// Load one of these before applying an assessment, and persist the replacement
/// state returned in [`crate::Transition`]. Target identity belongs to the caller's
/// record/key; this value contains only that target's learning facts.
///
/// Use `LearningState::default()` for a newly imported target. It is unintroduced,
/// unscheduled, and has no algorithm memory. For restored records, call
/// [`crate::LearningPolicy::validate_state`] before accepting them. A public struct
/// literal alone does not establish that a persisted state is coherent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningState {
    /// Whether the current version has been introduced.
    pub introduced: bool,
    /// Lifetime first introduction, retained across content changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_introduced: Option<StudyDay>,
    /// Ungraded introduction of the current version by teaching or personal capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taught_at: Option<StudyTime>,
    /// Last model assessment for this version, including Game Again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_review_at: Option<StudyTime>,
    /// Newest issuance covered by an accepted Test of this answer version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assessed_test_issue_ms: Option<u64>,
    /// Latest Test acceptance day across answer versions of this target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_graded_day: Option<StudyDay>,
    /// The one coalesced review obligation for this answer version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_review: Option<GameReview>,
    /// This unseen answer was encountered in a deviation. Teaching consumes relevance.
    #[serde(default, skip_serializing_if = "is_false")]
    pub game_teaching_relevance: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<Correction>,
    /// A first introduction through game repair is outside the ordinary quota.
    #[serde(default, skip_serializing_if = "is_false")]
    pub game_introduction: bool,
    /// Scheduled test day for the current version.
    pub due: Option<StudyDay>,
    /// Outstanding repeat obligation; Queue reads a copied fact to select repeats.
    pub repeat_pending: bool,
    /// Last day that earned a positive schedule advance; enforces one daily credit.
    pub last_credit_day: Option<StudyDay>,
    /// Acceptance day of the latest miss; observation time is not used here.
    pub last_miss_day: Option<StudyDay>,
    /// Monotonic even after a successful repeat; old attempts remain identifiable.
    pub miss_revision: MissRevision,
    /// Number of accepted schedule-changing assessments, not the algorithm model.
    pub scheduled_reviews: u64,
    /// Algorithm-owned state retained with the due date, or None before scheduling.
    pub scheduler_memory: Option<SchedulerMemory>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::policy;
    use crate::{
        AssessmentInput, AssessmentSource, LearningEffect, LearningError, LearningPolicy,
        StandardLearning,
    };
    use tempo_types::{Grade, StudyTime};
    #[test]
    fn a_new_target_gains_its_first_schedule_through_the_policy() {
        let imported = LearningState::default();
        let first_attempt = AssessmentInput {
            grade: Grade::Good,
            accepted_at: StudyTime {
                day: StudyDay(10),
                unix_ms: 864_000_000,
            },
            source: AssessmentSource::Human {
                attempt_id: "first-tactic-attempt".into(),
                issued_miss_revision: imported.miss_revision,
            },
        };
        let policy: StandardLearning = StandardLearning::default();
        let proposed = policy.apply(&imported, &first_attempt).unwrap();
        assert_eq!(proposed.effect, LearningEffect::ScheduleAdvanced);
        assert!(proposed.state.introduced);
        assert_eq!(proposed.state.due, Some(StudyDay(13)));
        assert_eq!(policy.validate_state(&proposed.state), Ok(()));
        assert_eq!(imported, LearningState::default());
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
