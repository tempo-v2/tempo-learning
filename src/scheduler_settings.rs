//! Versioned scheduler configuration used to check restore compatibility.
use serde::{Deserialize, Serialize};

/// Capture this from [`crate::LearningPolicy::settings`] alongside an export.
/// The application compares it with the destination policy's settings before
/// restore. It describes configuration, while [`crate::SchedulerMemory`] holds
/// each target's evolving model state.
///
/// Algorithm identity alone is insufficient: two instances of the same algorithm
/// may use different parameters. Changing settings needs an explicit adoption or
/// migration decision, not a silent reinterpretation of exported learning.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerSettings {
    /// Scheduler identity that owns the parameter encoding.
    pub algorithm: String,
    /// Version of the parameter encoding; not a content or attempt version.
    pub schema_version: u32,
    /// Algorithm-owned bytes, compared exactly by the current restore contract.
    pub parameters: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use crate::test_support::{human, policy};
    use crate::{FixedIntervalScheduler, LearningPolicy, LearningState, StandardLearning};
    use tempo_types::Grade;
    #[test]
    fn restore_can_detect_different_parameters_for_the_same_algorithm() {
        let exported_settings = policy().settings();
        let compatible: StandardLearning = StandardLearning::default();
        let different_interval = StandardLearning {
            scheduler: FixedIntervalScheduler {
                good_interval_days: 7,
            },
        };
        assert_eq!(exported_settings, compatible.settings());
        assert_eq!(
            exported_settings.algorithm,
            different_interval.settings().algorithm
        );
        assert_ne!(exported_settings, different_interval.settings());

        // Reviewing a target changes model memory, not the policy configuration.
        let trained = policy()
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        assert!(trained.state.scheduler_memory.is_some());
        assert_eq!(exported_settings, policy().settings());
    }
}
