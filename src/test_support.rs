//! Readable input builders for policy invariant tests. Input-type usage examples
//! construct their own values when the fields are the point of the example.
use crate::{AssessmentInput, AssessmentSource, StandardLearning};
use tempo_types::{Grade, MissRevision, StudyDay, StudyTime};

pub(crate) fn at(day: i32) -> StudyTime {
    StudyTime {
        day: StudyDay(day),
        unix_ms: (day as u64) * 86_400_000,
    }
}

pub(crate) fn human(grade: Grade, day: i32, revision: u64) -> AssessmentInput {
    AssessmentInput {
        grade,
        accepted_at: at(day),
        source: AssessmentSource::Human {
            attempt_id: "attempt".into(),
            issued_miss_revision: MissRevision(revision),
        },
    }
}

pub(crate) fn policy() -> StandardLearning {
    StandardLearning::default()
}
