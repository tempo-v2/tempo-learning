//! Corrective learning and compatibility with legacy game requests.

use crate::{LearningError, LearningState};
use serde::{Deserialize, Serialize};
use tempo_types::{ContentVersion, StudyDay, StudyTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameReview {
    pub answer_version: ContentVersion,
    pub occurred_by_ms: u64,
    pub actionable_day: StudyDay,
}

/// A portable legacy obligation whose saved facts do not identify a raw game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoredGameObligation {
    Review(GameReview),
    Teaching,
}

pub(crate) const CORRECTIVE_RECALLS: u8 = 3;

/// The active answer version's corrective sequence. A missing recall day
/// means the learner still needs to reproduce the shown move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Correction {
    pub answer_version: ContentVersion,
    pub clean_recalls: u8,
    pub awaiting_introduction: bool,
    pub next_recall_day: Option<StudyDay>,
    pub previous_move: Option<String>,
}

impl Correction {
    pub fn remaining_recalls(&self) -> u8 {
        CORRECTIVE_RECALLS.saturating_sub(self.clean_recalls)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameReviewEffect {
    RecordedOnly,
    RequestedReview,
    AlreadyAddressed,
    PrioritizedTeaching,
    StaleContent,
    AgainApplied,
    ChoiceRequired,
}

impl LearningState {
    /// Project a target-wide daily gate. Legacy current-version facts are still
    /// valid if a portable snapshot predates the separate target-wide day.
    pub fn can_accept_daily_test(&self, day: StudyDay) -> bool {
        self.last_human_test_day()
            .is_none_or(|previous| day > previous)
    }

    pub(crate) fn last_human_test_day(&self) -> Option<StudyDay> {
        self.last_graded_day.or_else(|| {
            (self.miss_revision.0 == 0)
                .then_some(self.last_review_at)
                .flatten()
                .map(|at| at.day)
        })
    }

    /// Called on the same proposed state that will commit the first Test assessment.
    pub(crate) fn record_assessed_test(
        &mut self,
        issued: StudyTime,
        accepted: StudyTime,
    ) -> Result<(), LearningError> {
        if issued.unix_ms > accepted.unix_ms || issued.day > accepted.day {
            return Err(LearningError::AcceptanceBeforePriorLearning);
        }
        self.last_graded_day = Some(accepted.day);
        self.last_assessed_test_issue_ms = Some(
            self.last_assessed_test_issue_ms
                .map_or(issued.unix_ms, |old| old.max(issued.unix_ms)),
        );
        Ok(())
    }
}
