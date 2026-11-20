//! Project an explicitly requested immediate recall without changing FSRS cadence.
use crate::{DailyAction, LearningState};
use tempo_types::{ContentVersion, StudyDay};

/// Required practice is still blind recall, but does not advance a schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmediateRecallMode {
    Test,
    Practice,
}

impl LearningState {
    /// The application supplies a durable request and Queue gives it precedence.
    /// Learning alone decides whether the current scheduled-test contract permits
    /// a grade. A newly generated target first uses `introduce_capture`.
    pub fn immediate_recall_mode(
        &self,
        version: ContentVersion,
        today: StudyDay,
    ) -> ImmediateRecallMode {
        let available = self.daily_availability(version, today);
        if available.action == DailyAction::RequestedTest
            || available.scheduled_due.is_some_and(|day| day <= today)
        {
            ImmediateRecallMode::Test
        } else {
            ImmediateRecallMode::Practice
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DailyLearning;
    use tempo_types::StudyTime;

    #[test]
    fn new_capture_is_test_but_future_and_already_graded_recall_preserve_cadence() {
        let day = StudyDay(10);
        let version = ContentVersion(1);
        let mut state = DailyLearning::introduce_capture(
            &LearningState::default(),
            StudyTime {
                day,
                unix_ms: 1_000,
            },
        )
        .unwrap();
        assert_eq!(
            state.immediate_recall_mode(version, day),
            ImmediateRecallMode::Test
        );
        let unchanged = state.clone();
        let _ = state.immediate_recall_mode(version, day);
        assert_eq!(state, unchanged);
        state.due = Some(StudyDay(12));
        assert_eq!(
            state.immediate_recall_mode(version, day),
            ImmediateRecallMode::Practice
        );
        state.due = Some(day);
        state.last_graded_day = Some(day);
        assert_eq!(
            state.immediate_recall_mode(version, day),
            ImmediateRecallMode::Practice
        );
        assert_eq!(
            state.immediate_recall_mode(version, StudyDay(11)),
            ImmediateRecallMode::Test
        );
    }
}
