# Generation review — 2026-09-30

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
