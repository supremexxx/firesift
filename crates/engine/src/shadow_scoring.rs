//! P3 bounded, observational candidate scoring. It runs only after v1 has
//! persisted a forecast batch and is never consulted by serving.

use std::{collections::BTreeMap, time::Instant};

use anyhow::Context;
use chrono::{Datelike as _, Weekday};
use serde_json::json;
use sha2::{Digest, Sha256};
use store::{ShadowScoreWrite, Store};

use crate::{
    candidate_artifact::{CandidateArtifact, score_with_artifact},
    config::Config,
};

pub async fn record_forecast_batch(
    store: &Store,
    config: &Config,
    computed_at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<usize> {
    if !config.shadow_scoring_enabled {
        return Ok(0);
    }
    let candidate_id = config
        .shadow_scoring_candidate_id
        .context("SHADOW_SCORING_CANDIDATE_ID is required when shadow scoring is enabled")?;
    let candidate = store
        .model_candidate_by_id_read_only(candidate_id)
        .await?
        .context("configured shadow candidate does not exist")?;
    anyhow::ensure!(
        candidate.status == "inactive" || candidate.status == "candidate",
        "shadow candidate must not be active"
    );
    let artifact: CandidateArtifact = serde_json::from_value(candidate.artifact)
        .context("shadow candidate artifact is invalid")?;
    artifact
        .validate()
        .context("shadow candidate artifact validation failed")?;
    let active = store
        .active_human_model()
        .await?
        .context("no active v1 model exists")?;
    let inputs = store
        .shadow_inputs_for_forecast(computed_at, config.shadow_scoring_limit_per_horizon)
        .await?;
    for input in &inputs {
        let date = input.valid_at.date_naive();
        let mut raw = BTreeMap::new();
        for name in [
            "wui",
            "road",
            "agri",
            "population",
            "poi",
            "power_line",
            "hist",
            "combustible",
        ] {
            if let Some(value) = input.features.get(name) {
                raw.insert(name.to_owned(), value.clone());
            }
        }
        raw.insert(
            "weekend".to_owned(),
            json!(matches!(date.weekday(), Weekday::Sat | Weekday::Sun)),
        );
        raw.insert("public_holiday".to_owned(), json!(false));
        raw.insert(
            "season_sine".to_owned(),
            json!((2.0 * std::f64::consts::PI * f64::from(date.ordinal()) / 365.25).sin()),
        );
        raw.insert(
            "season_cosine".to_owned(),
            json!((2.0 * std::f64::consts::PI * f64::from(date.ordinal()) / 365.25).cos()),
        );
        let checksum = format!("{:x}", Sha256::digest(serde_json::to_vec(&raw)?));
        let started = Instant::now();
        let result = score_with_artifact(&artifact, &raw);
        let latency = i64::try_from(started.elapsed().as_micros()).ok();
        let (candidate_value, error) = match result {
            Ok(candidate_value) => (Some(candidate_value), None),
            Err(error) => (None, Some(error.to_string())),
        };
        store
            .upsert_shadow_score(ShadowScoreWrite {
                input,
                active_model_id: active.id,
                candidate_model_id: candidate.id,
                candidate_score: candidate_value,
                candidate_error: error.as_deref(),
                feature_checksum: &checksum,
                latency_micros: latency,
                git_commit: &candidate.git_commit,
            })
            .await?;
    }
    Ok(inputs.len())
}
