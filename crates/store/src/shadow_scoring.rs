//! P3 observational storage. These helpers are intentionally separate from
//! serving reads: no API route consumes `ml.model_shadow_scores`.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;

use crate::{Store, StoreError};

#[derive(Clone, Debug)]
pub struct ShadowInput {
    pub h3: i64,
    pub computed_at: DateTime<Utc>,
    pub valid_at: DateTime<Utc>,
    pub horizon: String,
    pub active_score: f64,
    pub features: Value,
}

#[derive(Clone, Debug)]
pub struct ShadowScoreWrite<'a> {
    pub input: &'a ShadowInput,
    pub active_model_id: i64,
    pub candidate_model_id: i64,
    pub candidate_score: Option<f64>,
    pub candidate_error: Option<&'a str>,
    pub feature_checksum: &'a str,
    pub latency_micros: Option<i64>,
    pub git_commit: &'a str,
}

impl Store {
    /// Selects only the highest v1 scores for one completed forecast batch.
    /// This bounded query prevents P3 from becoming a full duplicate pipeline.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounded observational query fails.
    pub async fn shadow_inputs_for_forecast(
        &self,
        computed_at: DateTime<Utc>,
        limit_per_horizon: i64,
    ) -> Result<Vec<ShadowInput>, StoreError> {
        let rows = sqlx::query(
            "WITH ranked AS (
               SELECT r.h3, r.computed_at, r.valid_at, r.horizon, r.score, c.features,
                      ROW_NUMBER() OVER (PARTITION BY r.horizon ORDER BY r.score DESC, r.h3) AS rank
               FROM risk_scores r JOIN cell_static c ON c.h3 = r.h3
               WHERE r.computed_at = $1
             )
             SELECT h3, computed_at, valid_at, horizon, score, features
             FROM ranked WHERE rank <= $2 ORDER BY horizon, rank",
        )
        .bind(computed_at)
        .bind(limit_per_horizon)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ShadowInput {
                    h3: row.try_get("h3")?,
                    computed_at: row.try_get("computed_at")?,
                    valid_at: row.try_get("valid_at")?,
                    horizon: row.try_get("horizon")?,
                    active_score: f64::from(row.try_get::<f32, _>("score")?),
                    features: row.try_get("features")?,
                })
            })
            .collect()
    }

    /// Upserts one observational score. The active score is never updated or served here.
    ///
    /// # Errors
    ///
    /// Returns an error if the observational write fails.
    pub async fn upsert_shadow_score(&self, row: ShadowScoreWrite<'_>) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO ml.model_shadow_scores
             (computed_at, valid_at, horizon, h3, active_model_id, candidate_model_id,
              active_score, candidate_score, score_diff, candidate_error, feature_checksum,
              latency_micros, git_commit)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
             ON CONFLICT (h3, computed_at, horizon, candidate_model_id) DO NOTHING",
        )
        .bind(row.input.computed_at)
        .bind(row.input.valid_at)
        .bind(&row.input.horizon)
        .bind(row.input.h3)
        .bind(row.active_model_id)
        .bind(row.candidate_model_id)
        .bind(row.input.active_score)
        .bind(row.candidate_score)
        .bind(
            row.candidate_score
                .map(|score| score - row.input.active_score),
        )
        .bind(row.candidate_error)
        .bind(row.feature_checksum)
        .bind(row.latency_micros)
        .bind(row.git_commit)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
