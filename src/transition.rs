//! Proposed replacement state together with its explanation.
use crate::{LearningEffect, LearningState};
use serde::{Deserialize, Serialize};

/// Consume this after [`crate::LearningPolicy::apply`]. The application includes
/// `state` and the corresponding assessment/effect in its atomic acceptance
/// operation. A returned transition is a proposal, not evidence of a commit.
///
/// The prior state is borrowed and remains unchanged. If persistence fails,
/// discard this value and retry against current facts using application replay
/// protection; Learning itself cannot decide whether an event was already saved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// Complete next state to persist if the surrounding transaction succeeds.
    pub state: LearningState,
    /// Explanation of the policy decision made for this assessment.
    pub effect: LearningEffect,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LearningPolicy;
    use crate::test_support::{human, policy};
    use tempo_types::{Grade, StudyDay};
    #[test]
    fn a_proposal_changes_the_next_call_only_when_the_caller_supplies_its_state() {
        let stored = LearningState::default();
        let input = human(Grade::Good, 10, 0);
        let proposed = policy().apply(&stored, &input).unwrap();
        assert_eq!(proposed.state.due, Some(StudyDay(13)));
        assert_eq!(stored, LearningState::default());

        // The pure function does not remember that a previous proposal was made.
        assert_eq!(policy().apply(&stored, &input).unwrap(), proposed);
        let after_acceptance = policy().apply(&proposed.state, &input).unwrap();
        assert_eq!(after_acceptance.effect, LearningEffect::PracticeOnly);
    }
}
