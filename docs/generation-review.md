# Generation review — 2026-09-30

## Clarity follow-up — 2026-10-01

Read-only inspection of “Event Tags” found a Debug starter with one intentional
bug, two visible examples and three acceptance-check groups. One group bundled
three empty-token cases; another bundled empty input and invalid-character cases.
Those cases followed the visible spec, but the anonymous check numbers concealed
what each result represented. Input constraints also mixed caller guarantees with
rejection rules. The user expected a clearly labeled mix of Build, Debug and Test.

The TUI now explains the starting point and completion task for each mode, including
why Debug starters already pass some checks. Generation v5 requires a unique,
bounded behavior name for every check; names appear in the brief and run results.
The prompt asks for one focused scenario per check and separates input guarantees
from required malformed-input behavior. Check labels count toward the reading budget.
Existing packages keep their original check numbers and requirements. No fresh
generation or exercise execution was performed for this follow-up; semantic
alignment of the new labels with actual checks still needs generation evaluation.

## Follow-up — 2026-10-01

The five saved packages still all target boundaries. The latest, “Validation
Batches”, was requested as chunking/backend-validation: varying the scenario had
not removed batching as the operation. Fixed-order tie breaking and a small
scenario catalog were additional sources of repetition.

Generation v4 replaces shape/scenario selection with 33 curated subtopics across
10 categories. The structure takes inspiration from distinct algorithmic
operations in LeetCode problems such as [binary search](https://leetcode.com/problems/binary-search/)
and [delimiter matching](https://leetcode.com/problems/valid-parentheses/), alongside
practical parsing, configuration and state-handling topics. It does not import
LeetCode problem statements or use its difficulty labels as duration estimates.

The CLI randomly shortlists one subtopic from each of three different categories,
filtered by skill and duration. Recent topics/categories cool down and older use
reduces sampling weight. The model returns one typed ID constrained by a
request-specific schema enum, checked again locally and saved with the package.
Batching is allowed only for partitioning. Profile interests contextualize the
chosen operation rather than biasing category selection. `spar topics` exposes
the complete catalog without a model call.

This follow-up changes future generation only; existing packages and attempts are
untouched. The selected ID is still a model declaration, not proof of semantic
adherence. No fresh generation or execution evaluation was performed for this
follow-up; diversity and quality across modes/languages need subsequent evaluation.

## Original review

Scope: read-only inspection of the three locally stored generated packages, their
admission provenance and attempt outcomes. The packages were generated September
27–29; all were recorded as Apple-container validated. This review did not rerun
them or alter stored packages, drafts or profile settings.

| Rep | Findings |
| --- | --- |
| Expire a Cache Entry on Time | Legacy v1; examples embedded in a dense paragraph, with executable examples also rendered by the UI. A short boundary-debug exercise. |
| Fix the record size boundary | v2 examples duplicated verbatim in the brief. Requirements combine several behaviors; generated checks do exercise ordering, duplicates and non-mutation. Another comparison-boundary debug task. |
| Dispatch a Full Batch | v2 examples again duplicated. IDs such as `below` and `equal` diverge from the UI's `R1` convention. One threshold rule split across three requirements increases reading overhead. |

Two attempts were replaced as too large; the latest is unfinished. There are no
completed successful attempts in this sample. The scheduler therefore continuing
with its initial boundary-debug objective is expected, not evidence of failed
mastery. The actual defect was that replacement feedback never reached the model.
Changing a family name alone also provides weak protection against repetitive
exercise mechanics.

## Implemented changes

- Separate the generation DTO from the backward-compatible package reader. The
  app owns language, mode, skill, duration, stored version, interface and task text.
- Split the brief into bounded summary, input/output kind and description, and
  constraints. No multiline prose/example blocks; render examples from one source.
- Use bounded string/source/JSON-text types, enum requirement IDs, enum JSON kinds,
  required named hint stages, closed objects and bounded collections.
- Validate requirement references and order, example JSON syntax and top-level
  kinds, duplicate inputs, source sizes, reading budgets and exact family reuse.
- Derive visible assertions from examples, with recursive JSON-aware equality for
  Python so booleans cannot pass as integers. This closes a validator gap found
  during the review; it was not an observed failure in these three packages.
- Send replacement feedback and previous validation failures to generation. Keep
  the existing two-attempt cap and deterministic provider-error handling.
- Ask for fresh operations/data shapes, concise ordinary/edge examples, checks
  beyond examples, and concrete TypeScript annotations instead of `any`.
- Choose a skill-appropriate exercise shape and scenario locally. Track the last
  12 generated packages including queued/active reps, put shapes on a two-rep
  cooldown, penalize recent scenarios/pairs and break Build/Debug mode streaks.
  Store the requested design on new reps; use only conservative family-name hints
  for old packages that lack metadata. Learning objectives still take priority.
- Version the prompt/response contract in provenance as `generation-v3`, while
  converting accepted output into stored format v2.

## Limits and next evaluation

This is a small Python-only sample. It does not establish correctness, difficulty
calibration, TypeScript quality or improved generation success rates. No new model
generation was performed for this change. Nested example field types still depend
on the described contract and executable checks; the JSON-kind enum checks the
top-level value only. Prose rules cannot detect every semantic duplication or
solution spoiler. Exact family rejection cannot detect every cosmetic rename.
Design tags record what Spar requested, not a verified classification of the
model's code; semantic adherence still needs exercise-quality review.

A subsequent generation evaluation should cover all three modes in both languages,
compare reading length and repeated examples, inspect early hints for spoilers,
and confirm that intended bugs fail by assertion. Preserve execution admission:
passing the response schema is not sufficient evidence of exercise correctness.

The schema uses the closed objects, required fields, enums and supported size
bounds described in [official OpenAI Structured Outputs documentation](https://developers.openai.com/api/docs/guides/structured-outputs).
