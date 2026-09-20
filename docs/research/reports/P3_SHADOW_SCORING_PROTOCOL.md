# P3 — Shadow scoring protocol

## Hypothesis

Because the offline 2025 comparison favoured the inactive candidate over v1,
we will score a bounded sample of the same live forecast cells with both
models. The candidate remains observational: v1 is always the served model.

## Fixed design

- Disabled unless `SHADOW_SCORING_ENABLED=true`.
- An explicit `SHADOW_SCORING_CANDIDATE_ID` is required; no "latest" model is
  selected implicitly.
- At most 100 highest v1 scores per horizon per completed forecast batch.
- Rows are written only to `ml.model_shadow_scores`; no API or alert query
  reads that table.

## Measures

Primary: later verified ignition outcomes for the scored cells. Secondary:
candidate/v1 score disagreement, candidate failure rate, write latency, and
coverage by horizon. Guardrails: v1 HTTP responses, alert selection, and
forecast completion must remain unchanged; any candidate failure is recorded
but cannot fail a forecast cycle.

## Decision rule

Do not promote or activate the candidate from early observations. Review only
after a pre-declared sufficient live outcome sample, with stable guardrails and
independent ground-truth review. Turning the flag off stops new rows without
changing v1 or deleting prior observations.
