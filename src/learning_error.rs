//! Rejected state, incompatible algorithm data, and invalid learning transitions.
use std::fmt;

/// Handle these before committing an assessment. A failure does not mutate the
/// borrowed prior state; it is distinct from a valid no-change result such as
/// [`crate::LearningEffect::PracticeOnly`] or StalePositiveIgnored.
///
/// Memory/restore errors need compatible data or an explicit migration. Clock
/// and counter errors must be resolved by the caller; silently resetting state
/// would erase learning and weaken stale-attempt protection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LearningError {
    /// The fixed-interval fixture was configured with a zero Good interval.
    InvalidInterval,
    /// A review count or miss revision cannot be incremented.
    CounterOverflow,
    /// An interval or resulting study day does not fit the shared day type.
    DayOverflow,
    /// Acceptance predates a stored credit day or miss day.
    AcceptanceBeforePriorLearning,
    /// A repeat obligation lacks the introduction/miss facts needed to explain it.
    InvalidRepeatState,
    /// External feedback attempted to award positive human learning credit.
    ExternalGoodUnsupported,
    /// The selected scheduler cannot interpret this algorithm or schema version.
    IncompatibleSchedulerMemory,
    /// Bytes do not match the selected algorithm's supported memory encoding.
    MalformedSchedulerMemory,
    /// A scheduler proposed a Good due date that is not after acceptance day.
    InvalidSchedule,
    /// Persisted introduction, due, miss, review, or memory facts are inconsistent.
    InvalidLearningState,
    /// This target already received its ordinary Test assessment on that study day.
    DailyTestAlreadyAccepted,
}
impl fmt::Display for LearningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "learning contract: {self:?}")
    }
}
impl std::error::Error for LearningError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{human, policy};
    use crate::{LearningPolicy, LearningState};
    use tempo_types::{Grade, MissRevision, StudyDay};
    #[test]
    fn overflow_and_time_travel_return_errors_without_changing_input() {
        let prior = LearningState {
            introduced: true,
            due: Some(StudyDay(11)),
            last_miss_day: Some(StudyDay(10)),
            miss_revision: MissRevision(u64::MAX),
            ..LearningState::default()
        };
        assert_eq!(
            policy().apply(&prior, &human(Grade::Again, 10, u64::MAX)),
            Err(LearningError::CounterOverflow)
        );
        assert_eq!(prior.miss_revision, MissRevision(u64::MAX));
        let future = LearningState {
            introduced: true,
            due: Some(StudyDay(15)),
            last_credit_day: Some(StudyDay(12)),
            ..LearningState::default()
        };
        assert_eq!(
            policy().apply(&future, &human(Grade::Good, 10, 0)),
            Err(LearningError::AcceptanceBeforePriorLearning)
        );
    }
}
