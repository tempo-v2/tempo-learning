//! Versioned algorithm state retained with a target's learning record.
use serde::{Deserialize, Serialize};

/// Persist these bytes as part of [`crate::LearningState`] and pass them back to
/// the same compatible scheduler. Only that scheduler decodes the payload.
/// Store preserves it without interpreting it. Queue and Study do not receive
/// scheduler memory; the app projects only the learning facts each needs.
///
/// This narrowly scoped envelope lets a future algorithm keep model state
/// without adding its fields to unrelated domain contracts. An unknown algorithm,
/// unsupported schema, or malformed payload is an error, never an implicit reset.
/// Use [`crate::LearningPolicy::validate_state`] on restored learning records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerMemory {
    /// Decoder identity owned by the scheduler implementation.
    pub algorithm: String,
    /// Version of that algorithm's memory encoding.
    pub schema_version: u32,
    /// Opaque encoded model state; only the selected algorithm validates its meaning.
    pub payload: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{human, policy};
    use crate::{LearningError, LearningPolicy, LearningState};
    use tempo_types::Grade;
    #[test]
    fn memory_schema_and_algorithm_mismatches_are_errors_not_resets() {
        let first = policy()
            .apply(&LearningState::default(), &human(Grade::Good, 10, 0))
            .unwrap();
        assert_eq!(
            first.state.scheduler_memory.as_ref().unwrap().payload,
            1u64.to_le_bytes()
        );
        for (algorithm, schema, payload, expected) in [
            (
                "other",
                1,
                vec![0; 8],
                LearningError::IncompatibleSchedulerMemory,
            ),
            (
                "fixed_interval",
                2,
                vec![0; 8],
                LearningError::IncompatibleSchedulerMemory,
            ),
            (
                "fixed_interval",
                1,
                vec![0; 3],
                LearningError::MalformedSchedulerMemory,
            ),
        ] {
            let mut prior = first.state.clone();
            prior.scheduler_memory = Some(SchedulerMemory {
                algorithm: algorithm.into(),
                schema_version: schema,
                payload,
            });
            assert_eq!(
                policy().apply(&prior, &human(Grade::Good, 13, 0)),
                Err(expected)
            );
        }
    }
}
