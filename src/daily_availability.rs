//! Learning facts used to select an activity for the current answer version.
use crate::{LearningState, RestoredGameObligation};
use tempo_types::{ContentVersion, Grade, StudyDay};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DailyObligation {
    None,
    Repair,
    CorrectiveRecall { day: StudyDay },
    RestoredTeaching,
    RestoredReview { day: StudyDay },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DailyAction {
    None,
    RequestedTest,
    Teach,
    Repair,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyAvailability {
    pub introduced_this_version: bool,
    pub first_introduced: Option<StudyDay>,
    pub scheduled_due: Option<StudyDay>,
    pub obligation: DailyObligation,
    pub action: DailyAction,
    pub corrective_recalls_remaining: u8,
}

/// Learning's assessment of a current repertoire answer for connected Study.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepertoireRecall {
    Standalone,
    Familiar,
    Reviewable,
}

impl DailyAvailability {
    /// Supply the last applied human Test grade for this answer version, if known.
    /// Queue still determines whether independently due work can be selected.
    pub fn repertoire_recall(&self, last_test: Option<Grade>, today: StudyDay) -> RepertoireRecall {
        if self.obligation != DailyObligation::None || last_test == Some(Grade::Again) {
            return RepertoireRecall::Standalone;
        }
        if self.introduced_this_version
            && last_test == Some(Grade::Good)
            && self.scheduled_due.is_none_or(|due| due > today)
        {
            return RepertoireRecall::Familiar;
        }
        RepertoireRecall::Reviewable
    }
}

impl LearningState {
    /// Read availability without changing any scheduling, cadence or correction state.
    pub fn daily_availability(
        &self,
        answer_version: ContentVersion,
        today: StudyDay,
    ) -> DailyAvailability {
        let may_test = self.can_accept_daily_test(today);
        let correction = self
            .correction
            .as_ref()
            .filter(|correction| correction.answer_version == answer_version);
        let obligation = match (correction, self.restored_game_obligation) {
            (Some(correction), _) => match correction.next_recall_day {
                Some(day) => DailyObligation::CorrectiveRecall { day },
                None => DailyObligation::Repair,
            },
            (None, Some(RestoredGameObligation::Teaching)) => DailyObligation::RestoredTeaching,
            (None, Some(RestoredGameObligation::Review(review))) => {
                DailyObligation::RestoredReview {
                    day: review.actionable_day,
                }
            }
            (None, None) => DailyObligation::None,
        };
        let action = match obligation {
            DailyObligation::Repair => DailyAction::Repair,
            DailyObligation::RestoredTeaching => DailyAction::Teach,
            DailyObligation::CorrectiveRecall { day } | DailyObligation::RestoredReview { day }
                if may_test && day <= today =>
            {
                DailyAction::RequestedTest
            }
            _ => DailyAction::None,
        };
        DailyAvailability {
            introduced_this_version: self.introduced
                && !correction.is_some_and(|correction| correction.awaiting_introduction),
            first_introduced: self.first_introduced,
            scheduled_due: self.due.filter(|_| may_test && correction.is_none()),
            obligation,
            action,
            corrective_recalls_remaining: correction
                .map_or(0, |correction| correction.remaining_recalls()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Correction, GameReview};

    #[test]
    fn a_future_current_correction_fences_scheduler_due_until_recall_is_actionable() {
        let version = ContentVersion(1);
        let mut state = LearningState {
            introduced: true,
            first_introduced: Some(StudyDay(1)),
            due: Some(StudyDay(4)),
            correction: Some(Correction {
                answer_version: version,
                clean_recalls: 1,
                awaiting_introduction: false,
                next_recall_day: Some(StudyDay(5)),
                previous_move: None,
            }),
            ..LearningState::default()
        };
        let waiting = state.daily_availability(version, StudyDay(4));
        assert_eq!(
            waiting.obligation,
            DailyObligation::CorrectiveRecall { day: StudyDay(5) }
        );
        assert_eq!(waiting.action, DailyAction::None);
        assert_eq!(waiting.scheduled_due, None);
        assert_eq!(waiting.corrective_recalls_remaining, 2);
        assert_eq!(
            state.daily_availability(version, StudyDay(5)).action,
            DailyAction::RequestedTest
        );
        state.last_graded_day = Some(StudyDay(5));
        assert_eq!(
            state.daily_availability(version, StudyDay(5)).action,
            DailyAction::None
        );
        let stale = state.daily_availability(ContentVersion(2), StudyDay(6));
        assert_eq!(stale.obligation, DailyObligation::None);
        assert_eq!(stale.scheduled_due, Some(StudyDay(4)));
    }

    #[test]
    fn repair_and_legacy_teaching_are_ungated_and_legacy_review_respects_daily_cadence() {
        let version = ContentVersion(1);
        let mut state = LearningState {
            last_graded_day: Some(StudyDay(4)),
            restored_game_obligation: Some(RestoredGameObligation::Teaching),
            ..LearningState::default()
        };
        assert_eq!(
            state.daily_availability(version, StudyDay(4)).action,
            DailyAction::Teach
        );
        state.restored_game_obligation = Some(RestoredGameObligation::Review(GameReview {
            answer_version: version,
            occurred_by_ms: 1,
            actionable_day: StudyDay(4),
        }));
        assert_eq!(
            state.daily_availability(version, StudyDay(4)).action,
            DailyAction::None
        );
        assert_eq!(
            state.daily_availability(version, StudyDay(5)).action,
            DailyAction::RequestedTest
        );
        state.correction = Some(Correction {
            answer_version: version,
            clean_recalls: 0,
            awaiting_introduction: true,
            next_recall_day: None,
            previous_move: None,
        });
        assert_eq!(
            state.daily_availability(version, StudyDay(4)).action,
            DailyAction::Repair
        );
    }
}
