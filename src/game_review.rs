//! Daily review obligations derived from game evidence, without scheduling a grade.

use crate::{LearningError, LearningState, calendar::add_days};
use serde::{Deserialize, Serialize};
use tempo_types::{ContentVersion, StudyDay, StudyTime};

/// Deviation chronology for the active answer, supplied by the evidence caller.
/// Bounds describe when any actual move could have occurred.
#[derive(Clone, Copy)]
pub struct GameMiss {
    pub answer_version: ContentVersion,
    pub earliest_ms: Option<u64>,
    pub latest_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameReview {
    pub answer_version: ContentVersion,
    pub occurred_by_ms: u64,
    pub actionable_day: StudyDay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameReviewEffect {
    RecordedOnly,
    RequestedReview,
    AlreadyAddressed,
    PrioritizedTeaching,
    StaleContent,
}

impl LearningState {
    /// Project a target-wide daily gate. Legacy current-version facts are still
    /// valid if a portable snapshot predates the separate target-wide day.
    pub fn can_accept_daily_test(&self, day: StudyDay) -> bool {
        self.last_graded_day
            .or(self.last_review_at.map(|at| at.day))
            .is_none_or(|previous| day > previous)
    }

    /// Classify an active-version miss using only durable learning and game time.
    pub fn game_miss_effect(&self, miss: GameMiss) -> GameReviewEffect {
        if !self.introduced {
            return GameReviewEffect::PrioritizedTeaching;
        }
        let (Some(earliest), Some(latest), Some(taught)) =
            (miss.earliest_ms, miss.latest_ms, self.taught_at)
        else {
            return GameReviewEffect::RecordedOnly;
        };
        if earliest > latest || earliest <= taught.unix_ms {
            return GameReviewEffect::RecordedOnly;
        }
        if self
            .last_assessed_test_issue_ms
            .is_some_and(|issue| issue > latest)
        {
            return GameReviewEffect::AlreadyAddressed;
        }
        GameReviewEffect::RequestedReview
    }

    /// Merge evidence without changing scheduler memory, due date or study credit.
    /// The caller has already established active target/version and learner identity.
    pub fn observe_game_miss(&mut self, miss: GameMiss) -> Result<GameReviewEffect, LearningError> {
        let effect = self.game_miss_effect(miss);
        match effect {
            GameReviewEffect::PrioritizedTeaching => self.game_teaching_relevance = true,
            GameReviewEffect::RequestedReview => {
                let taught = self
                    .taught_at
                    .expect("applicable game miss follows teaching");
                let latest = miss
                    .latest_ms
                    .expect("applicable game miss has a time bound");
                let prior_day = self
                    .last_graded_day
                    .or(self.last_review_at.map(|at| at.day));
                let earliest_day = prior_day.map_or(taught.day, |day| day.max(taught.day));
                let occurred_by_ms = self
                    .game_review
                    .filter(|review| review.answer_version == miss.answer_version)
                    .map_or(latest, |review| latest.max(review.occurred_by_ms));
                self.game_review = Some(GameReview {
                    answer_version: miss.answer_version,
                    occurred_by_ms,
                    actionable_day: add_days(earliest_day, 1)?,
                });
            }
            _ => {}
        }
        Ok(effect)
    }

    /// Called on the same proposed state that will commit the first Test assessment.
    pub(crate) fn record_assessed_test(
        &mut self,
        version: ContentVersion,
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
        if let Some(review) = &mut self.game_review {
            if review.answer_version != version || issued.unix_ms > review.occurred_by_ms {
                self.game_review = None;
            } else {
                review.actionable_day = add_days(accepted.day, 1)?;
            }
        }
        Ok(())
    }
}
