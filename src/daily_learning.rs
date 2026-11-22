//! Opening lessons and scheduled tests use distinct learning transitions.
use crate::{
    AssessmentInput, AssessmentSource, Correction, LearningEffect, LearningError, LearningPolicy,
    LearningState, RestoredGameObligation, SchedulerMemory, SchedulerSettings, Transition,
    calendar::add_days, game_review::CORRECTIVE_RECALLS,
};
use fsrs::{DEFAULT_PARAMETERS, FSRS, MemoryState};
use tempo_types::{ContentVersion, Grade, MissRevision, StudyTime};

const ALGORITHM: &str = "fsrs-6.6.2";
const MEMORY_FORMAT: u32 = 1;
const RETENTION: f32 = 0.90;
const MAX_INTERVAL_DAYS: u32 = 36_500;

#[derive(Default)]
pub struct DailyLearning {
    model: FSRS,
}

impl DailyLearning {
    /// Acknowledge a new or revised answer and schedule its first blind test.
    pub fn teach(prior: &LearningState, at: StudyTime) -> Result<LearningState, LearningError> {
        Self::introduce_ungraded(prior, at, add_days(at.day, 1)?)
    }

    /// Entering a personal tactic is an ungraded introduction with immediate recall due.
    pub fn introduce_capture(
        prior: &LearningState,
        at: StudyTime,
    ) -> Result<LearningState, LearningError> {
        Self::introduce_ungraded(prior, at, at.day)
    }

    fn introduce_ungraded(
        prior: &LearningState,
        at: StudyTime,
        due: tempo_types::StudyDay,
    ) -> Result<LearningState, LearningError> {
        if prior.introduced || prior.taught_at.is_some() || prior.scheduler_memory.is_some() {
            return Err(LearningError::InvalidLearningState);
        }
        check_time(prior, at)?;
        let mut state = prior.clone();
        state.introduced = true;
        state.first_introduced.get_or_insert(at.day);
        state.taught_at = Some(at);
        state.game_teaching_relevance = false;
        if state.restored_game_obligation == Some(RestoredGameObligation::Teaching) {
            state.restored_game_obligation = None;
        }
        state.due = Some(due);
        Ok(state)
    }

    /// Keep lifetime admission when a different answer supersedes this version.
    pub fn changed_answer(prior: &LearningState) -> LearningState {
        LearningState {
            first_introduced: prior.first_introduced,
            last_graded_day: prior.last_human_test_day(),
            game_introduction: prior.game_introduction,
            miss_revision: prior.miss_revision,
            ..LearningState::default()
        }
    }

    /// Recover the old one-Test or teaching-priority obligation from a portable
    /// snapshot. A complete corrective sequence already covers the pending request.
    /// Use only for portable data, where the original game cannot be replayed.
    pub fn restore_portable(
        state: &mut LearningState,
        version: ContentVersion,
    ) -> Result<(), LearningError> {
        let legacy = match (state.game_review, state.game_teaching_relevance) {
            (Some(review), false) => Some(RestoredGameObligation::Review(review)),
            (None, true) => Some(RestoredGameObligation::Teaching),
            (None, false) => None,
            (Some(_), true) => return Err(LearningError::InvalidLearningState),
        };
        if matches!(state.restored_game_obligation.or(legacy),
            Some(RestoredGameObligation::Review(review)) if review.answer_version != version)
        {
            return Err(LearningError::InvalidLearningState);
        }
        if let Some(obligation) = legacy {
            if state.restored_game_obligation.is_some()
                || !valid_restored_obligation(state, obligation)
            {
                return Err(LearningError::InvalidLearningState);
            }
            if state.correction.is_none() {
                state.restored_game_obligation = Some(obligation);
            }
            state.game_review = None;
            state.game_teaching_relevance = false;
        }
        Ok(())
    }

    /// Accept the first assessment of an ordinary daily Test using its saved issue time.
    pub(crate) fn transition_test(
        prior: &LearningState,
        input: &AssessmentInput,
        version: ContentVersion,
        issued_at: StudyTime,
        assess: impl FnOnce() -> Result<Transition, LearningError>,
    ) -> Result<Transition, LearningError> {
        let AssessmentSource::Human {
            issued_miss_revision,
            ..
        } = input.source
        else {
            return Err(LearningError::InvalidLearningState);
        };
        if input.grade == Grade::Good && issued_miss_revision != prior.miss_revision {
            return Ok(Transition {
                state: prior.clone(),
                effect: LearningEffect::StalePositiveIgnored,
            });
        }
        if !prior.can_accept_daily_test(input.accepted_at.day) {
            return Err(LearningError::DailyTestAlreadyAccepted);
        }
        let mut transition = assess()?;
        transition
            .state
            .record_assessed_test(issued_at, input.accepted_at)?;
        if let Some(RestoredGameObligation::Review(review)) =
            &mut transition.state.restored_game_obligation
        {
            if review.answer_version != version || issued_at.unix_ms > review.occurred_by_ms {
                transition.state.restored_game_obligation = None;
            } else {
                review.actionable_day = add_days(input.accepted_at.day, 1)?;
            }
        }
        if let Some(mut correction) = prior.correction.clone() {
            match input.grade {
                Grade::Again => {
                    Self::start_correction(
                        &mut transition.state,
                        version,
                        correction.previous_move,
                        false,
                    )?;
                }
                Grade::Good => {
                    if correction
                        .next_recall_day
                        .is_none_or(|day| day > input.accepted_at.day)
                    {
                        return Err(LearningError::InvalidLearningState);
                    }
                    correction.clean_recalls += 1;
                    correction.next_recall_day = Some(add_days(input.accepted_at.day, 1)?);
                    transition.state.correction =
                        (correction.remaining_recalls() > 0).then_some(correction);
                }
            }
        }
        Ok(transition)
    }

    /// Production Game Again, including the corrective obligation and human-Test cadence.
    pub fn assess_game(
        policy: &dyn LearningPolicy,
        prior: &LearningState,
        input: &AssessmentInput,
        version: ContentVersion,
        previous_move: Option<String>,
    ) -> Result<Transition, LearningError> {
        if !matches!(input.source, AssessmentSource::Game { .. }) || input.grade != Grade::Again {
            return Err(LearningError::InvalidLearningState);
        }
        let mut transition = policy.apply_game_again(prior, input)?;
        transition.state.restored_game_obligation = None;
        transition.state.last_graded_day = prior.last_human_test_day();
        transition.state.game_introduction |= prior.first_introduced.is_none();
        let awaiting_introduction = !prior.introduced
            || prior
                .correction
                .as_ref()
                .is_some_and(|correction| correction.awaiting_introduction);
        Self::start_correction(
            &mut transition.state,
            version,
            previous_move,
            awaiting_introduction,
        )?;
        transition.effect = LearningEffect::MissRecorded;
        Ok(transition)
    }

    fn start_correction(
        state: &mut LearningState,
        answer_version: ContentVersion,
        previous_move: Option<String>,
        awaiting_introduction: bool,
    ) -> Result<(), LearningError> {
        state.miss_revision.0 = state
            .miss_revision
            .0
            .checked_add(1)
            .ok_or(LearningError::CounterOverflow)?;
        state.correction = Some(Correction {
            answer_version,
            clean_recalls: 0,
            next_recall_day: None,
            previous_move,
            awaiting_introduction,
        });
        Ok(())
    }

    pub fn repair(
        prior: &LearningState,
        version: ContentVersion,
        revision: MissRevision,
        at: StudyTime,
    ) -> Result<LearningState, LearningError> {
        let mut state = prior.clone();
        if revision == prior.miss_revision
            && let Some(correction) = &mut state.correction
            && correction.answer_version == version
            && correction.next_recall_day.is_none()
        {
            check_time(prior, at)?;
            correction.next_recall_day = Some(add_days(at.day, 1)?);
            correction.awaiting_introduction = false;
        }
        Ok(state)
    }
}

impl LearningPolicy for DailyLearning {
    fn settings(&self) -> SchedulerSettings {
        SchedulerSettings {
            algorithm: ALGORITHM.into(),
            schema_version: MEMORY_FORMAT,
            parameters: RETENTION
                .to_le_bytes()
                .into_iter()
                .chain(MAX_INTERVAL_DAYS.to_le_bytes())
                .chain(
                    DEFAULT_PARAMETERS
                        .iter()
                        .flat_map(|value| value.to_le_bytes()),
                )
                .collect(),
        }
    }

    fn validate_state(&self, state: &LearningState) -> Result<(), LearningError> {
        let reviewed = state.last_review_at.is_some();
        if state.introduced != state.due.is_some()
            || state.introduced != (reviewed || state.taught_at.is_some())
            || (state.introduced && state.first_introduced.is_none())
            || reviewed != (state.scheduled_reviews != 0)
            || reviewed != state.scheduler_memory.is_some()
            || state.last_assessed_test_issue_ms.is_some_and(|issue| {
                state
                    .last_review_at
                    .is_none_or(|review| issue > review.unix_ms)
            })
            || state
                .last_graded_day
                .zip(state.last_review_at)
                .is_some_and(|(day, review)| day > review.day)
            || state.correction.as_ref().is_some_and(|correction| {
                !reviewed
                    || state.miss_revision.0 == 0
                    || correction.clean_recalls >= CORRECTIVE_RECALLS
                    || correction
                        .next_recall_day
                        .zip(state.last_review_at)
                        .is_some_and(|(day, review)| day <= review.day)
                    || (correction.next_recall_day.is_none() && correction.clean_recalls != 0)
                    || (correction.awaiting_introduction && correction.next_recall_day.is_some())
            })
            || state.restored_game_obligation.is_some_and(|obligation| {
                state.correction.is_some() || !valid_restored_obligation(state, obligation)
            })
            || state.repeat_pending
            || state.last_credit_day.is_some()
            || state.last_miss_day.is_some()
        {
            return Err(LearningError::InvalidLearningState);
        }
        if let Some(memory) = &state.scheduler_memory {
            decode_memory(memory)?;
        }
        Ok(())
    }

    fn assess_test(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
        version: ContentVersion,
        issued_at: StudyTime,
    ) -> Result<Transition, LearningError> {
        Self::transition_test(prior, input, version, issued_at, || {
            self.assess_model(prior, input)
        })
    }

    fn apply_game_again(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError> {
        if !matches!(input.source, AssessmentSource::Game { .. }) || input.grade != Grade::Again {
            return Err(LearningError::InvalidLearningState);
        }
        self.assess_model(prior, input)
    }
}

impl DailyLearning {
    fn assess_model(
        &self,
        prior: &LearningState,
        input: &AssessmentInput,
    ) -> Result<Transition, LearningError> {
        self.validate_state(prior)?;
        let at = input.accepted_at;
        check_time(prior, at)?;
        let elapsed = prior
            .last_review_at
            .map_or(0, |last| at.day.0.abs_diff(last.day.0));
        let memory = prior
            .scheduler_memory
            .as_ref()
            .map(decode_memory)
            .transpose()?;
        let updates = self
            .model
            .next_states(memory, RETENTION, elapsed)
            .map_err(|_| LearningError::InvalidSchedule)?;
        let result = match input.grade {
            Grade::Again => updates.again,
            Grade::Good => updates.good,
        };
        if !result.interval.is_finite() || result.interval <= 0.0 {
            return Err(LearningError::InvalidSchedule);
        }
        let days = (result.interval.round().max(1.0) as u32).min(MAX_INTERVAL_DAYS);
        let mut state = prior.clone();
        state.introduced = true;
        state.first_introduced.get_or_insert(at.day);
        state.last_review_at = Some(at);
        state.due = Some(add_days(at.day, days)?);
        state.scheduled_reviews = prior
            .scheduled_reviews
            .checked_add(1)
            .ok_or(LearningError::CounterOverflow)?;
        state.scheduler_memory = Some(SchedulerMemory {
            algorithm: ALGORITHM.into(),
            schema_version: MEMORY_FORMAT,
            payload: result
                .memory
                .stability
                .to_le_bytes()
                .into_iter()
                .chain(result.memory.difficulty.to_le_bytes())
                .collect(),
        });
        Ok(Transition {
            state,
            effect: LearningEffect::ScheduleAdvanced,
        })
    }
}

fn valid_restored_obligation(state: &LearningState, obligation: RestoredGameObligation) -> bool {
    match obligation {
        RestoredGameObligation::Review(review) => {
            state.taught_at.is_some_and(|taught| {
                review.occurred_by_ms > taught.unix_ms && review.actionable_day > taught.day
            }) && state
                .last_assessed_test_issue_ms
                .is_none_or(|issue| issue <= review.occurred_by_ms)
        }
        RestoredGameObligation::Teaching => !state.introduced || state.correction.is_some(),
    }
}

fn check_time(prior: &LearningState, at: StudyTime) -> Result<(), LearningError> {
    if prior.first_introduced.is_some_and(|day| day > at.day)
        || [prior.taught_at, prior.last_review_at]
            .into_iter()
            .flatten()
            .any(|last| last.day > at.day || last.unix_ms > at.unix_ms)
    {
        return Err(LearningError::AcceptanceBeforePriorLearning);
    }
    Ok(())
}

fn decode_memory(memory: &SchedulerMemory) -> Result<MemoryState, LearningError> {
    if memory.algorithm != ALGORITHM || memory.schema_version != MEMORY_FORMAT {
        return Err(LearningError::IncompatibleSchedulerMemory);
    }
    let [a, b, c, d, e, f, g, h] = memory.payload.as_slice() else {
        return Err(LearningError::MalformedSchedulerMemory);
    };
    let stability = f32::from_le_bytes([*a, *b, *c, *d]);
    let difficulty = f32::from_le_bytes([*e, *f, *g, *h]);
    if !stability.is_finite() || stability <= 0.0 || !(1.0..=10.0).contains(&difficulty) {
        return Err(LearningError::MalformedSchedulerMemory);
    }
    Ok(MemoryState {
        stability,
        difficulty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{at, human};
    use tempo_types::StudyDay;

    #[test]
    fn learning_lifecycle_test_enforces_cadence_without_using_attempt_identity() {
        let daily = DailyLearning::default();
        let policy: &dyn LearningPolicy = &daily;
        let prior = DailyLearning::teach(&LearningState::default(), at(10)).unwrap();
        let mut input = human(Grade::Good, 11, 0);
        let result = policy
            .assess_test(&prior, &input, ContentVersion(1), at(11))
            .unwrap();
        input.source = AssessmentSource::Human {
            attempt_id: "renamed-attempt".into(),
            issued_miss_revision: prior.miss_revision,
        };
        assert_eq!(
            policy.assess_test(&prior, &input, ContentVersion(1), at(11)),
            Ok(result.clone())
        );
        assert_eq!(result.effect, LearningEffect::ScheduleAdvanced);
        assert_eq!(result.state.scheduled_reviews, 1);
        assert_eq!(
            policy.assess_test(&result.state, &input, ContentVersion(1), at(11)),
            Err(LearningError::DailyTestAlreadyAccepted)
        );
    }

    #[test]
    fn learning_lifecycle_uses_exact_correction_revision_and_preserves_fsrs_on_repair() {
        let daily = DailyLearning::default();
        let policy: &dyn LearningPolicy = &daily;
        let version = ContentVersion(1);
        let prior = DailyLearning::teach(&LearningState::default(), at(10)).unwrap();
        let mut game = AssessmentInput {
            grade: Grade::Again,
            accepted_at: at(11),
            source: AssessmentSource::Game {
                source: "pgn".into(),
                game_id: "first".into(),
                played_date: None,
                started_at_ms: None,
                ended_at_ms: None,
            },
        };
        let miss = DailyLearning::assess_game(policy, &prior, &game, version, None).unwrap();
        game.source = AssessmentSource::Game {
            source: "renamed-source".into(),
            game_id: "renamed-game".into(),
            played_date: Some("2026-11-20".into()),
            started_at_ms: Some(1),
            ended_at_ms: Some(2),
        };
        assert_eq!(
            DailyLearning::assess_game(policy, &prior, &game, version, None),
            Ok(miss.clone())
        );
        assert_eq!(miss.effect, LearningEffect::MissRecorded);
        assert_eq!(miss.state.miss_revision, MissRevision(1));

        let stale = policy.assess_test(&miss.state, &human(Grade::Good, 12, 0), version, at(10));
        assert_eq!(
            stale,
            Ok(Transition {
                state: miss.state.clone(),
                effect: LearningEffect::StalePositiveIgnored,
            })
        );
        assert_eq!(
            DailyLearning::repair(&miss.state, version, MissRevision(0), at(12)).unwrap(),
            miss.state
        );
        assert_eq!(
            DailyLearning::repair(&miss.state, ContentVersion(2), MissRevision(1), at(12)).unwrap(),
            miss.state
        );
        let repaired =
            DailyLearning::repair(&miss.state, version, MissRevision(1), at(12)).unwrap();
        assert_eq!(
            repaired.correction.unwrap().next_recall_day,
            Some(StudyDay(13))
        );
        assert_eq!(repaired.scheduled_reviews, miss.state.scheduled_reviews);
        assert_eq!(repaired.scheduler_memory, miss.state.scheduler_memory);
    }

    #[test]
    fn learning_lifecycle_distinguishes_ungraded_entry_and_changed_answer() {
        let prior = LearningState::default();
        let taught = DailyLearning::teach(&prior, at(10)).unwrap();
        let captured = DailyLearning::introduce_capture(&prior, at(10)).unwrap();
        assert_eq!(taught.due, Some(StudyDay(11)));
        assert_eq!(captured.due, Some(StudyDay(10)));
        for introduction in [taught, captured] {
            assert_eq!(introduction.first_introduced, Some(StudyDay(10)));
            assert_eq!(introduction.scheduled_reviews, 0);
            assert!(introduction.scheduler_memory.is_none());
            let changed = DailyLearning::changed_answer(&introduction);
            assert_eq!(changed.first_introduced, introduction.first_introduced);
            assert!(!changed.introduced);
            assert_eq!(changed.due, None);
            assert!(changed.scheduler_memory.is_none());
        }
    }
}
