# tempo-learning

Pure learning transitions for Tempo 2. Its only Tempo dependency is `tempo-types`.
It receives an assessment and prior state, then proposes the next state. It has
no store, queue, chess, provider, browser, or wall-clock dependency.

## Public contract

| Interface | Responsibility |
| --- | --- |
| `LearningPolicy::apply(&LearningState, &AssessmentInput)` | Return a `Transition { state, effect }` without mutating the supplied state. |
| `LearningPolicy::settings()` | Return versioned scheduler parameters for export and restore compatibility. |
| `LearningPolicy::validate_state(&LearningState)` | Validate state coherence and scheduler memory without fabricating a grade. Restore calls this before mutation. |
| `Scheduler::schedule(&LearningState, Grade, StudyTime)` | Return `SchedulerUpdate { due, memory }`; the scheduler can update its model as well as the due date. |
| `Scheduler::validate_memory(&SchedulerMemory)` | Validate the selected algorithm's memory schema and encoding. |

`LearningState` owns introduced/due/repeat facts, last credit and miss days, a
monotonic `miss_revision`, an accepted schedule-event counter, and optional
scheduler memory. `AssessmentInput` carries a grade, acceptance time and source:

- Human: attempt ID and the miss revision captured when that attempt was issued.
- External: provider, stable source-event ID and observation timestamp. External
  Again uses acceptance time for scheduling; observation time remains provenance.

`StandardLearning<S>` implements the object-safe `LearningPolicy` through an
injected `Scheduler`. Application code can use `Box<dyn LearningPolicy>` and
replace the algorithm at composition time.

## Invariants implemented now

- Again is accepted even when a target is not due. It introduces the target,
  updates scheduler memory, advances the miss revision, creates a repeat
  obligation and makes the target due tomorrow.
- A fresh clean same-day repeat clears the obligation and remains due tomorrow.
  It preserves scheduler memory and does not earn another schedule advance.
- An eligible Good earns at most one daily positive credit. Non-due success is
  practice. A newer day permits a new credit when due.
- A human Good whose issued miss revision differs from current state has no
  positive effect, including after a repeat was cleared. The miss revision never
  resets when a repeat clears.
- Unsupported external Good, incompatible memory, malformed state, counter/day
  overflow and backwards acceptance days are explicit errors.

Persistence, replay and one terminal assessment per attempt remain application /
Store responsibilities. Calling this pure function twice does not deduplicate an
event; the application must apply it within the guarded acceptance transaction.

## Stateful scheduler replacement

`SchedulerMemory { algorithm, schema_version, payload }` and
`SchedulerSettings { algorithm, schema_version, parameters }` are narrowly scoped,
versioned envelopes owned by the scheduler. Store preserves their bytes; Queue,
Study and UI do not interpret them. A future FSRS adapter can retain its model
state and parameters without adding FSRS fields to unrelated component contracts.
The chosen scheduler validates its memory, and restore compares exact settings
before accepting it. Unknown algorithms, versions or malformed bytes are errors;
there is no silent reset.

The current `FixedIntervalScheduler { good_interval_days }` is a fixture, not
FSRS. It uses algorithm `fixed_interval`, schema version 1, a four-byte
little-endian interval parameter, and eight-byte little-endian review-count
memory. Its default Good interval is three days. Learning's tomorrow/credit/repeat
rules remain outside that replaceable algorithm. Supporting a different
algorithm still requires implementing its codec/validation and migration policy;
the envelope does not claim cross-algorithm memory compatibility.

## Checks and development

Run `cargo test -p tempo-learning` from the parent development workspace. Focused
tests cover daily credit, non-due misses, fresh repeats, stale success, external
Again, scheduler substitution, invalid memory and overflow/time handling. They
need no database or browser. Application acceptance must additionally prove
atomic commit, replay and restore equivalence with these same interfaces.

The manifest requests the exact Tempo dependency version. Local workspace patches
resolve the unreleased scaffold without inheriting package metadata from a parent.
