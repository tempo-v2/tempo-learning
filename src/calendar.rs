//! Checked study-day arithmetic shared by policy rules and the fixture scheduler.
//! This helper receives days explicitly and never reads a wall clock.
use crate::LearningError;
use tempo_types::StudyDay;

pub(crate) fn add_days(day: StudyDay, count: u32) -> Result<StudyDay, LearningError> {
    let count = i32::try_from(count).map_err(|_| LearningError::DayOverflow)?;
    day.0
        .checked_add(count)
        .map(StudyDay)
        .ok_or(LearningError::DayOverflow)
}
