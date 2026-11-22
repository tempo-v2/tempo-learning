# tempo-learning

Learning turns an accepted performance grade into a **proposed change to a target's
learning state**. It decides whether that grade earns scheduling credit, clears a
repeat, records a miss, or only counts as practice. Its only Tempo dependency is
`tempo-types`; it receives no Store, Queue, Chess, provider, or clock handle.

## Start here

The source tree is an outline of the concepts. Each public concept lives in a
named file with purpose/usage documentation and same-file `#[cfg(test)] mod tests`
examples. `lib.rs` is the navigation page and root reexport list. Existing imports
such as `use tempo_learning::{LearningState, LearningPolicy};` are unchanged.

Read the input and output files first, then `learning_policy.rs`, then
`standard_learning.rs` when you want the actual decision rules. Input examples
construct concrete values in place. A private `test_support.rs` keeps repeated
assessment setup short in the policy invariant tests.

| When you reach for it | Concept and source | Executable usage scenario in that file |
| --- | --- | --- |
| Start a new target or load current learning | [`LearningState`](src/learning_state.rs) | `a_new_target_gains_its_first_schedule_through_the_policy`; restore-state rejection examples |
| Pass a grade and its acceptance time | [`AssessmentInput`](src/assessment_input.rs) | `external_again_uses_acceptance_day_not_old_observation` |
| Distinguish human freshness from external provenance | [`AssessmentSource`](src/assessment_source.rs) | `a_human_attempt_issued_before_a_new_miss_cannot_clear_that_repeat` |
| Explain why the same Good can have different consequences | [`LearningEffect`](src/learning_effect.rs) | `two_clean_attempts_on_one_day_can_have_different_learning_effects` |
| Consume a proposed state change before committing | [`Transition`](src/transition.rs) | `a_proposal_changes_the_next_call_only_when_the_caller_supplies_its_state` |
| Inject learning behavior into application commands | [`LearningPolicy`](src/learning_policy.rs) | `scheduler_is_replaceable_without_changing_learning_port` |
| Understand daily credit, misses, repeats, and stale attempts | [`StandardLearning`](src/standard_learning.rs) | `nondue_miss_is_again_and_fresh_repeat_does_not_extend_interval`; daily/stale scenarios |
| Implement another scheduling algorithm | [`Scheduler`](src/scheduler.rs) | `a_new_algorithm_reuses_the_existing_daily_credit_and_repeat_rules` with a complete test implementation |
| Return both a due date and model state | [`SchedulerUpdate`](src/scheduler_update.rs) | `a_credited_success_installs_the_due_date_and_memory_from_one_update` |
| Preserve and validate each target's algorithm state | [`SchedulerMemory`](src/scheduler_memory.rs) | `memory_schema_and_algorithm_mismatches_are_errors_not_resets` |
| Compare export/restore configuration | [`SchedulerSettings`](src/scheduler_settings.rs) | `restore_can_detect_different_parameters_for_the_same_algorithm` |
| Use deterministic scheduling while wiring components | [`FixedIntervalScheduler`](src/fixed_interval_scheduler.rs) | `scheduled_reviews_preserve_and_advance_the_algorithm_memory` |
| Handle an invalid transition without mutating stored state | [`LearningError`](src/learning_error.rs) | `overflow_and_time_travel_return_errors_without_changing_input` |

The private `calendar.rs` helper performs checked day arithmetic. It does not read
a clock. The scheduler and the daily policy both need that arithmetic, but neither
needs a general-purpose utility or application-context object.

## Production Daily Study

Use [`DailyLearning`](src/daily_learning.rs) for the installed FSRS profile.
`assess_game` accepts a validated Game Again and returns the model update,
corrective obligation and preserved human Test cadence as one transition.
`apply_game_again` is its legacy model hook.
`LearningPolicy::assess_test` is the installed profile's Test entry point. It
uses the issued attempt's miss revision and time to apply the first assessment
under the daily cadence. `repair` accepts ungraded
guided work, and `Correction::remaining_recalls` projects the remaining recalls.
Portable legacy requests enter through `DailyLearning::restore_portable` for
compatibility without a new grade.

`StandardLearning` and the fixed-interval examples below describe the standard
and fixture profiles.

## Follow one standard-profile assessment

1. The application loads a `LearningState` for the target. New targets start at
   `LearningState::default()`: unintroduced, no due date, and no scheduler memory.
2. Study supplies a terminal `Grade`, or the application accepts programmatic
   Again. The application creates `AssessmentInput` with acceptance time and
   `AssessmentSource` provenance. Human provenance includes the miss revision
   captured when the attempt was issued.
3. `LearningPolicy::apply` validates prior state and decides the effect. When a
   scheduling event is appropriate, `StandardLearning` calls the injected
   `Scheduler` for a `SchedulerUpdate` containing both date and model memory.
4. Learning returns `Transition { state, effect }`. The borrowed prior state stays
   unchanged. The application includes the proposed state, assessment, and other
   acceptance writes in one atomic transaction.

A grade describes performance. An effect describes its learning consequence.
Learning does not decide whether a chess answer is correct. The scheduler does not
decide whether an extra practice attempt deserves another daily credit.

## Standard-profile outcomes

| Accepted input and context | Effect and proposed state |
| --- | --- |
| Again, even before the scheduled due day | `MissRecorded`: introduce the target, update scheduler memory, increment miss revision, create a repeat obligation, and set due tomorrow. |
| Fresh same-day Good after a miss | `RepeatCleared`: clear the obligation, keep tomorrow due, and preserve scheduler memory without another schedule advance. |
| Fresh eligible Good with no credit yet today | `ScheduleAdvanced`: install the scheduler's date/memory and record today's positive credit. |
| Fresh Good outside scheduling eligibility | Usually `PracticeOnly`: unchanged learning. An outstanding repeat can instead be cleared without a schedule advance. |
| Human Good whose issued miss revision differs from current state | `StalePositiveIgnored`: unchanged learning, even after a repeat was cleared or the day changed. |
| External Good | `ExternalGoodUnsupported`: programmatic feedback cannot award human success credit. |

The monotonic miss revision does not reset when a repeat clears. For a delayed
game scan, acceptance day determines scheduling; observation time remains
provenance. Invalid memory/state, overflow, and backwards acceptance days are
explicit errors.

## Policy versus scheduler

Application code depends on the object-safe `LearningPolicy` trait and can hold a
`Box<dyn LearningPolicy>`. `StandardLearning<S>` supplies the daily rules while
`S: Scheduler` supplies algorithm behavior. The same-file Scheduler example
implements a different algorithm and demonstrates that its due interval changes
while repeat and daily-credit rules continue to apply.

`SchedulerMemory` is per-target evolving model state. `SchedulerSettings` is
configuration captured for export/restore compatibility. Store preserves their
bytes; Queue, Study, and UI do not decode them. The selected algorithm validates
memory identity, schema, and payload. Unknown or malformed data is rejected
rather than silently reset. Restore also compares exact settings before mutation.
Each algorithm uses its own codec and validation.

The current `FixedIntervalScheduler` is a fixture, not FSRS. Its default Good
interval is three days. Algorithm identity is `fixed_interval`, schema 1;
parameters encode the interval in four little-endian bytes, and memory encodes a
review count in eight little-endian bytes. Learning's tomorrow/repeat/credit rules
remain outside that algorithm.

## Acceptance remains an application responsibility

Calling the pure policy twice does not deduplicate an event. `AttemptId` and
`EventId` are provenance here; the application/Store transaction guards replay,
one terminal assessment per attempt, and atomic persistence. A returned Transition
does not mean any write committed. Restore uses `validate_state`, not an invented
grade, to check records before accepting them.

## Run the examples

Run these from the parent development workspace. Filter by a module when you want
to learn one concept; the full package command also runs the compiling crate-doc
example.

```sh
cargo test -p tempo-learning assessment_source::tests
cargo test -p tempo-learning
cargo test -p tempo-learning --doc
cargo clippy -p tempo-learning --all-targets -- -D warnings
cargo doc -p tempo-learning --no-deps --open
```

The focused tests require no database or browser. Application acceptance separately
checks atomic commit, replay, and restore equivalence. The exact Tempo manifest
dependency is unchanged; local workspace patches resolve this unreleased scaffold.
See `.planning/ARCHITECTURE.md` and `.planning/INTERFACES.md` for component ownership
and cross-repository checks.


## Required immediate recalls

`LearningState::immediate_recall_mode(version, today)` is a pure projection for an
explicit missed-game recall request. It returns Test only when the existing daily
availability allows a due or requested Test; otherwise it returns Practice. This
lets Queue present a missed tactic immediately without granting another scheduled
assessment on the same day or moving a future schedule forward.

A newly generated puzzle first uses `DailyLearning::introduce_capture`, the existing
ungraded introduction due today. The application commits that transition with the
new content and request. Existing puzzle Learning is preserved. Store owns event
uniqueness and request fulfillment; Queue owns precedence; Learning does not fetch
games, inspect detector labels or invent a human grade from a game observation.
