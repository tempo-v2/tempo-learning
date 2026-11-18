//! Turn an accepted assessment into a proposed learning-state change.
//!
//! Start with [`AssessmentInput`] and an existing [`LearningState`]. Inject a
//! [`LearningPolicy`] to obtain a [`Transition`]. The application commits that
//! transition alongside the assessment; this crate never saves it or deduplicates
//! an event. Acceptance time is supplied explicitly, not read from a clock.
//!
//! # Where to read
//!
//! - Input: [`AssessmentInput`] and [`AssessmentSource`].
//! - Persistent facts: [`LearningState`].
//! - Output: [`Transition`] and [`LearningEffect`].
//! - Application boundary: [`LearningPolicy`] and [`LearningError`].
//! - Daily-credit/repeat rules: [`StandardLearning`].
//! - Replaceable scheduling: [`Scheduler`], [`SchedulerUpdate`],
//!   [`SchedulerMemory`], [`SchedulerSettings`], [`FixedIntervalScheduler`].
//!
//! Each concept has a named file with usage tests alongside it. The README maps
//! common questions to those files. Root imports remain the public entry point.
//!
//! # Apply one clean first attempt
//!
//! ```
//! use tempo_learning::{AssessmentInput, AssessmentSource, LearningEffect,
//!     LearningPolicy, LearningState, StandardLearning};
//! use tempo_types::{Grade, MissRevision, StudyDay, StudyTime};
//!
//! let policy: StandardLearning = StandardLearning::default();
//! let prior = LearningState::default();
//! let input = AssessmentInput {
//!     grade: Grade::Good,
//!     accepted_at: StudyTime { day: StudyDay(10), unix_ms: 864_000_000 },
//!     source: AssessmentSource::Human {
//!         attempt_id: "attempt-1".into(),
//!         issued_miss_revision: MissRevision(0),
//!     },
//! };
//! let proposed = policy.apply(&prior, &input)?;
//! assert_eq!(proposed.effect, LearningEffect::ScheduleAdvanced);
//! assert_eq!(proposed.state.due, Some(StudyDay(13)));
//! assert!(!prior.introduced); // The application still decides whether to commit.
//! # Ok::<(), tempo_learning::LearningError>(())
//! ```

mod assessment_input;
mod assessment_source;
mod calendar;
mod daily_learning;
mod fixed_interval_scheduler;
mod learning_effect;
mod learning_error;
mod learning_policy;
mod learning_state;
mod scheduler;
mod scheduler_memory;
mod scheduler_settings;
mod scheduler_update;
mod standard_learning;
mod transition;

pub use assessment_input::AssessmentInput;
pub use assessment_source::AssessmentSource;
pub use daily_learning::DailyLearning;
pub use fixed_interval_scheduler::FixedIntervalScheduler;
pub use learning_effect::LearningEffect;
pub use learning_error::LearningError;
pub use learning_policy::LearningPolicy;
pub use learning_state::LearningState;
pub use scheduler::Scheduler;
pub use scheduler_memory::SchedulerMemory;
pub use scheduler_settings::SchedulerSettings;
pub use scheduler_update::SchedulerUpdate;
pub use standard_learning::StandardLearning;
pub use transition::Transition;

#[cfg(test)]
mod test_support;
