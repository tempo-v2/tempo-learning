//! Provenance and freshness context for an accepted assessment.
use serde::{Deserialize, Serialize};
use tempo_types::{AttemptId, EventId, MissRevision};

/// Choose Human for an actual Study attempt and External for programmatic Again.
/// Human carries the miss revision captured at issuance so an old successful
/// attempt cannot cancel a newer miss. The revision remains significant after
/// the repeat is cleared. External Good is explicitly unsupported.
///
/// IDs are provenance, not an idempotency mechanism inside this pure crate.
/// The application/Store acceptance transaction must guard against replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssessmentSource {
    /// A combined repertoire miss in a completed game.
    Game {
        source: String,
        game_id: String,
        played_date: Option<String>,
        started_at_ms: Option<u64>,
        ended_at_ms: Option<u64>,
    },
    /// A terminal grade from a human exercise attempt.
    Human {
        /// Provenance identity; the application guards against duplicate acceptance.
        attempt_id: AttemptId,
        /// Miss revision captured when the attempt was issued, not when it was answered.
        issued_miss_revision: MissRevision,
    },
    /// Programmatic feedback, such as a missed repertoire move in a played game.
    External {
        /// Source namespace, for example the game provider or importer.
        provider: String,
        /// Stable source-event identity used by the application for replay protection.
        event_id: EventId,
        /// When the event occurred; scheduling uses the assessment acceptance time.
        observed_at_ms: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{at, human, policy};
    use crate::{AssessmentInput, LearningEffect, LearningError, LearningPolicy, LearningState};
    use tempo_types::Grade;
    #[test]
    fn a_human_attempt_issued_before_a_new_miss_cannot_clear_that_repeat() {
        let issued_before_miss = AssessmentSource::Human {
            attempt_id: "old-attempt".into(),
            issued_miss_revision: MissRevision(0),
        };
        let missed = policy()
            .apply(&LearningState::default(), &human(Grade::Again, 10, 0))
            .unwrap();
        let late_success = AssessmentInput {
            grade: Grade::Good,
            accepted_at: at(10),
            source: issued_before_miss,
        };
        let proposed = policy().apply(&missed.state, &late_success).unwrap();
        assert_eq!(proposed.effect, LearningEffect::StalePositiveIgnored);
        assert_eq!(proposed.state, missed.state);
        assert!(proposed.state.repeat_pending);
    }

    #[test]
    fn programmatic_feedback_cannot_award_a_human_success_credit() {
        let input = AssessmentInput {
            grade: Grade::Good,
            accepted_at: at(10),
            source: AssessmentSource::External {
                provider: "games".into(),
                event_id: "game-42:move-15".into(),
                observed_at_ms: 0,
            },
        };
        assert_eq!(
            policy().apply(&LearningState::default(), &input),
            Err(LearningError::ExternalGoodUnsupported)
        );
    }
}
