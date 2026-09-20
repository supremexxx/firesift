-- P3: observational candidate scoring. These rows never participate in serving.
CREATE TABLE ml.model_shadow_scores (
    id BIGSERIAL PRIMARY KEY,
    scored_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    computed_at TIMESTAMPTZ NOT NULL,
    valid_at TIMESTAMPTZ NOT NULL,
    horizon TEXT NOT NULL,
    h3 BIGINT NOT NULL,
    active_model_id BIGINT NOT NULL REFERENCES human_model_versions(id) ON DELETE RESTRICT,
    candidate_model_id BIGINT NOT NULL REFERENCES ml.model_candidate_registry(id) ON DELETE RESTRICT,
    active_score DOUBLE PRECISION NOT NULL,
    candidate_score DOUBLE PRECISION,
    score_diff DOUBLE PRECISION,
    candidate_error TEXT,
    feature_checksum TEXT NOT NULL,
    latency_micros BIGINT,
    git_commit TEXT NOT NULL,
    CHECK ((candidate_score IS NULL) = (candidate_error IS NOT NULL)),
    CHECK (score_diff IS NULL OR candidate_score IS NOT NULL),
    UNIQUE (h3, computed_at, horizon, candidate_model_id)
);
CREATE INDEX model_shadow_scores_scored_at_idx ON ml.model_shadow_scores(scored_at DESC);
COMMENT ON TABLE ml.model_shadow_scores IS 'Append-only observational scores; never read by serving or alert selection.';
