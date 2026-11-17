//! Due date and model memory returned together by a scheduler.
use crate::SchedulerMemory;
use serde::{Deserialize, Serialize};
use tempo_types::StudyDay;

/// Return this when implementing [`crate::Scheduler::schedule`]. A date alone
/// would lose a stateful algorithm's updated model, so the port keeps both values
/// together. Ordinary application callers receive a [`crate::Transition`] instead.
///
/// [`crate::StandardLearning`] validates memory and installs both values for an
/// eligible Good. On Again, it installs memory but enforces its own tomorrow-due
/// rule. A successful repeat may bypass the scheduler altogether.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerUpdate {
    /// Proposed due day; an accepted Good must schedule strictly after today.
    pub due: StudyDay,
    /// Updated model state belonging to the selected scheduler.
    pub memory: SchedulerMemory,
}

#[cfg(test)]
mod tests {
    use crate::test_support::{at, human};
    use crate::{
        FixedIntervalScheduler, LearningPolicy, LearningState, Scheduler, StandardLearning,
    };
    use tempo_types::Grade;
    #[test]
    fn a_credited_success_installs_the_due_date_and_memory_from_one_update() {
        let scheduler = FixedIntervalScheduler {
            good_interval_days: 7,
        };
        let prior = LearningState::default();
        let update = scheduler.schedule(&prior, Grade::Good, at(10)).unwrap();
        let learning = StandardLearning { scheduler };
        let proposed = learning.apply(&prior, &human(Grade::Good, 10, 0)).unwrap();
        assert_eq!(proposed.state.due, Some(update.due));
        assert_eq!(proposed.state.scheduler_memory, Some(update.memory));
    }
}
