//! Immutable BLUE daily forecast bulletins and read-only evidence views.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, NaiveDate, TimeZone as _, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::{Store, StoreError};

const BULLETIN_HOUR_UTC: u32 = 6;
const ALERT_THRESHOLD: f32 = 0.65;
const CRITICAL_THRESHOLD: f32 = 0.75;
const PROACTIVE_EVIDENCE_LIMIT: usize = 16;
const PERSISTENT_EVIDENCE_QUOTA: usize = 4;
const NEW_THRESHOLD_EVIDENCE_QUOTA: usize = 4;
const ACCELERATION_EVIDENCE_QUOTA: usize = 4;
const TERRITORIAL_EVIDENCE_QUOTA: usize = 4;
const REACTIVE_EVIDENCE_LIMIT: i64 = 4;
const STRONG_ACCELERATION_DELTA: f32 = 0.08;

#[derive(Clone, Debug)]
pub struct BlueForecastContext {
    pub environment: String,
    pub application_revision: String,
    pub application_image: String,
    pub application_image_digest: String,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct BlueForecastBulletinRow {
    pub id: String,
    pub logical_id: String,
    pub bulletin_date: NaiveDate,
    pub scheduled_for: DateTime<Utc>,
    pub issued_at: DateTime<Utc>,
    pub forecast_batch_computed_at: DateTime<Utc>,
    #[serde(skip_serializing)]
    pub forecast_source: String,
    pub model_version_id: i64,
    pub application_revision: String,
    pub forecast_cell_count: i64,
    pub mapped_cell_count: i64,
    pub unmapped_cell_count: i64,
    pub commune_count: i64,
    pub alerts_24h: i64,
    pub alerts_48h: i64,
    pub checksum: Option<String>,
    pub status: String,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct BlueForecastAlertRow {
    pub id: String,
    pub bulletin_id: String,
    pub bulletin_date: NaiveDate,
    pub issued_at: DateTime<Utc>,
    pub insee_code: String,
    pub commune_name: String,
    pub department_code: Option<String>,
    pub horizon: String,
    pub valid_at: DateTime<Utc>,
    pub alert_index: f32,
    pub max_score: f32,
    pub mean_score: f32,
    pub physical_at_peak: f32,
    pub human_at_peak: f32,
    pub evaluated_cell_count: i64,
    pub elevated_cell_count: i64,
    pub critical_cell_count: i64,
    pub risk_level: String,
    pub top_factors: Value,
    pub evaluation_status: String,
    pub evidence_count: i64,
}

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct BlueEvidenceCaseRow {
    pub id: String,
    pub bulletin_id: String,
    pub bulletin_date: NaiveDate,
    pub insee_code: String,
    pub commune_name: String,
    pub department_code: Option<String>,
    pub daily_rank: i16,
    pub selection_score: f32,
    pub selection_reason: String,
    pub alert_24h_id: Option<String>,
    pub alert_24h_index: Option<f32>,
    pub alert_24h_valid_at: Option<DateTime<Utc>>,
    pub alert_48h_id: Option<String>,
    pub alert_48h_index: Option<f32>,
    pub alert_48h_valid_at: Option<DateTime<Utc>>,
    pub research_after: DateTime<Utc>,
    pub review_stage: String,
    pub stage_attempt_count: i16,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub provisional_verdict: String,
    pub provisional_confidence: Option<f32>,
    pub provisional_summary: Option<String>,
    pub provisional_observed_event_at: Option<DateTime<Utc>>,
    pub provisional_observed_location: Option<String>,
    pub provisional_completed_at: Option<DateTime<Utc>>,
    pub status: String,
    pub verdict: String,
    pub confidence: Option<f32>,
    pub summary: Option<String>,
    pub observed_event_at: Option<DateTime<Utc>>,
    pub observed_location: Option<String>,
    pub model: Option<String>,
    pub attempt_count: i16,
    pub completed_at: Option<DateTime<Utc>>,
    pub sources: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlueRateMetric {
    pub numerator: i64,
    pub denominator: i64,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlueHorizonPerformance {
    pub eligible_cases: i64,
    pub reviewed_cases: i64,
    pub pending_cases: i64,
    pub observed_signals: i64,
    pub no_evidence_found: i64,
    pub inconclusive: i64,
    pub evidence_sources: i64,
    pub review_coverage: BlueRateMetric,
    pub observed_signal_rate: BlueRateMetric,
    pub observed_signal_rate_at_5: BlueRateMetric,
    pub observed_signal_rate_at_10: BlueRateMetric,
    pub observed_signal_rate_at_20: BlueRateMetric,
    pub mean_score_reviewed: Option<f64>,
    pub mean_score_observed: Option<f64>,
    pub mean_confidence: Option<f64>,
    pub mean_lead_time_hours: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BlueBulletinPerformanceRow {
    pub bulletin_id: String,
    pub bulletin_date: NaiveDate,
    pub selected_cases: i64,
    pub reviewed_24h: i64,
    pub reviewed_48h: i64,
    pub observed_24h: i64,
    pub observed_48h: i64,
    pub evidence_sources: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BluePerformanceSummary {
    pub generated_at: DateTime<Utc>,
    pub period_start: Option<NaiveDate>,
    pub period_end: Option<NaiveDate>,
    pub bulletin_count: i64,
    pub selected_case_count: i64,
    pub hours_24: BlueHorizonPerformance,
    pub hours_48: BlueHorizonPerformance,
    pub bulletins: Vec<BlueBulletinPerformanceRow>,
    pub unavailable_metrics: Vec<&'static str>,
    pub methodology: &'static str,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct BluePerformanceCase {
    bulletin_id: String,
    bulletin_date: NaiveDate,
    issued_at: DateTime<Utc>,
    daily_rank: i16,
    score_24h: Option<f32>,
    score_48h: Option<f32>,
    provisional_verdict: String,
    provisional_confidence: Option<f32>,
    provisional_observed_event_at: Option<DateTime<Utc>>,
    provisional_completed_at: Option<DateTime<Utc>>,
    verdict: String,
    confidence: Option<f32>,
    observed_event_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    sources_24h: i64,
    sources_48h: i64,
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct BlueEvidenceClaim {
    pub id: String,
    pub bulletin_id: String,
    pub bulletin_date: NaiveDate,
    pub issued_at: DateTime<Utc>,
    pub insee_code: String,
    pub commune_name: String,
    pub department_code: Option<String>,
    pub daily_rank: i16,
    pub selection_score: f32,
    pub selection_reason: String,
    pub trigger_observed_at: Option<DateTime<Utc>>,
    pub alert_24h_index: Option<f32>,
    pub alert_24h_valid_at: Option<DateTime<Utc>>,
    pub alert_48h_index: Option<f32>,
    pub alert_48h_valid_at: Option<DateTime<Utc>>,
    pub review_horizon: String,
    pub attempt_count: i16,
    pub stage_attempt_count: i16,
}

#[derive(Clone, Debug)]
pub struct BlueEvidenceSourceInput {
    pub url: String,
    pub title: String,
    pub published_at: Option<DateTime<Utc>>,
    pub excerpt: Option<String>,
    pub domain: String,
    pub relation_strength: String,
}

#[derive(Clone, Debug)]
pub struct BlueEvidenceResult {
    pub verdict: String,
    pub confidence: f32,
    pub summary: String,
    pub observed_event_at: Option<DateTime<Utc>>,
    pub observed_location: Option<String>,
    pub response_id: String,
    pub raw_response: Value,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub web_search_count: i64,
    pub sources: Vec<BlueEvidenceSourceInput>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct BlueEvidenceCandidate {
    bulletin_id: String,
    insee_code: String,
    commune_name: String,
    department_code: Option<String>,
    selection_score: f32,
    previous_score: Option<f32>,
    recently_selected: bool,
    alert_24h_id: Option<String>,
    alert_48h_id: Option<String>,
    research_24h: Option<DateTime<Utc>>,
    research_48h: Option<DateTime<Utc>>,
}

const BULLETIN_COLUMNS: &str = "id::text,logical_id,bulletin_date,scheduled_for,issued_at,
     forecast_batch_computed_at,forecast_source,model_version_id,application_revision,
     forecast_cell_count,mapped_cell_count,unmapped_cell_count,commune_count,
     alerts_24h,alerts_48h,checksum,status,published_at";

impl Store {
    /// Captures the first complete batch issued after 06:00 UTC as the day's
    /// immutable BLUE bulletin. Before 06:00 this is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error when provenance, horizons, commune coverage or
    /// publication integrity is incomplete.
    #[allow(clippy::too_many_lines)]
    pub async fn capture_blue_daily_bulletin(
        &self,
        computed_at: DateTime<Utc>,
        forecast_source: &str,
        context: &BlueForecastContext,
    ) -> Result<Option<BlueForecastBulletinRow>, StoreError> {
        if forecast_source.trim().is_empty()
            || context.environment.trim().is_empty()
            || context.application_revision.trim().is_empty()
            || context.application_image.trim().is_empty()
            || context.application_image_digest.trim().is_empty()
        {
            return Err(StoreError::SnapshotContract(
                "BLUE bulletin provenance is incomplete".to_owned(),
            ));
        }
        let bulletin_date = computed_at.date_naive();
        let scheduled_for = Utc.from_utc_datetime(&bulletin_date.and_time(
            chrono::NaiveTime::from_hms_opt(BULLETIN_HOUR_UTC, 0, 0).ok_or_else(|| {
                StoreError::SnapshotContract("invalid BLUE issue hour".to_owned())
            })?,
        ));
        if computed_at < scheduled_for {
            return Ok(None);
        }
        let logical_id = format!("blue-daily-{}", bulletin_date.format("%Y-%m-%d"));
        if let Some(row) = self.blue_bulletin_by_logical_id(&logical_id).await? {
            return if row.status == "published" {
                Ok(Some(row))
            } else {
                Err(StoreError::SnapshotContract(format!(
                    "BLUE bulletin {logical_id} has status {}",
                    row.status
                )))
            };
        }
        let model_version_id: i64 = sqlx::query_scalar(
            "SELECT id FROM human_model_versions WHERE active ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StoreError::SnapshotContract("no active BLUE model".to_owned()))?;
        let coverage_mask_id: String = sqlx::query_scalar(
            "SELECT id::text FROM observability.coverage_masks
             WHERE family='operational_aoi' AND status='published'
             ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| StoreError::SnapshotContract("no published coverage mask".to_owned()))?;
        let (cells_24h, cells_48h): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(DISTINCT h3) FILTER (WHERE horizon='hours_24'),
                    COUNT(DISTINCT h3) FILTER (WHERE horizon='hours_48')
             FROM risk_scores WHERE computed_at=$1",
        )
        .bind(computed_at)
        .fetch_one(&self.pool)
        .await?;
        if cells_24h == 0 || cells_24h != cells_48h {
            return Err(StoreError::SnapshotContract(format!(
                "incomplete BLUE horizons: +24 h={cells_24h}, +48 h={cells_48h}"
            )));
        }
        let id: Option<String> = sqlx::query_scalar(
            "INSERT INTO blue.forecast_bulletins(
                logical_id,bulletin_date,scheduled_for,issued_at,forecast_batch_computed_at,
                forecast_source,model_version_id,application_revision,application_image,
                application_image_digest,environment,coverage_mask_id,forecast_cell_count,
                unmapped_cell_count,aggregation_contract)
             VALUES($1,$2,$3,$4,$4,$5,$6,$7,$8,$9,$10,$11::uuid,$12,$12,
                jsonb_build_object('version','commune-p95-v1','alert_threshold',$13,
                    'critical_threshold',$14,'interpretation',
                    'relative_vigilance_index_not_calibrated_fire_probability'))
             ON CONFLICT(logical_id) DO NOTHING RETURNING id::text",
        )
        .bind(&logical_id)
        .bind(bulletin_date)
        .bind(scheduled_for)
        .bind(computed_at)
        .bind(forecast_source)
        .bind(model_version_id)
        .bind(&context.application_revision)
        .bind(&context.application_image)
        .bind(&context.application_image_digest)
        .bind(&context.environment)
        .bind(&coverage_mask_id)
        .bind(cells_24h)
        .bind(ALERT_THRESHOLD)
        .bind(CRITICAL_THRESHOLD)
        .fetch_optional(&self.pool)
        .await?;
        let Some(id) = id else {
            return self
                .blue_bulletin_by_logical_id(&logical_id)
                .await
                .map(|row| row.filter(|item| item.status == "published"));
        };
        match self
            .fill_and_publish_blue_bulletin(&id, computed_at, cells_24h)
            .await
        {
            Ok(row) => Ok(Some(row)),
            Err(error) => {
                let _ = sqlx::query(
                    "UPDATE blue.forecast_bulletins SET status='failed'
                     WHERE id=$1::uuid AND status='building'",
                )
                .bind(&id)
                .execute(&self.pool)
                .await;
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn fill_and_publish_blue_bulletin(
        &self,
        id: &str,
        computed_at: DateTime<Utc>,
        forecast_cell_count: i64,
    ) -> Result<BlueForecastBulletinRow, StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "CREATE TEMP TABLE blue_forecast_aggregate ON COMMIT DROP AS
             SELECT b.insee_code,b.name commune_name,b.department_code,
                COUNT(DISTINCT r.h3)::bigint evaluated_cell_count,
                MAX(r.valid_at) FILTER (WHERE r.horizon='hours_24') valid_at_24,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY r.score)
                    FILTER (WHERE r.horizon='hours_24')::real p95_24,
                MAX(r.score) FILTER (WHERE r.horizon='hours_24')::real max_24,
                AVG(r.score) FILTER (WHERE r.horizon='hours_24')::real mean_24,
                (array_agg(r.physical ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_24'))[1]::real physical_24,
                (array_agg(r.human ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_24'))[1]::real human_24,
                (array_agg(r.factors ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_24'))[1] factors_24,
                COUNT(*) FILTER (WHERE r.horizon='hours_24' AND r.score>=0.65)::bigint elevated_24,
                COUNT(*) FILTER (WHERE r.horizon='hours_24' AND r.score>=0.75)::bigint critical_24,
                MAX(r.valid_at) FILTER (WHERE r.horizon='hours_48') valid_at_48,
                percentile_cont(0.95) WITHIN GROUP (ORDER BY r.score)
                    FILTER (WHERE r.horizon='hours_48')::real p95_48,
                MAX(r.score) FILTER (WHERE r.horizon='hours_48')::real max_48,
                AVG(r.score) FILTER (WHERE r.horizon='hours_48')::real mean_48,
                (array_agg(r.physical ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_48'))[1]::real physical_48,
                (array_agg(r.human ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_48'))[1]::real human_48,
                (array_agg(r.factors ORDER BY r.score DESC)
                    FILTER (WHERE r.horizon='hours_48'))[1] factors_48,
                COUNT(*) FILTER (WHERE r.horizon='hours_48' AND r.score>=0.65)::bigint elevated_48,
                COUNT(*) FILTER (WHERE r.horizon='hours_48' AND r.score>=0.75)::bigint critical_48
             FROM reference.commune_boundaries b
             JOIN reference.commune_h3_cells c ON c.insee_code=b.insee_code
             JOIN risk_scores r ON r.h3=c.h3 AND r.computed_at=$1
                AND r.horizon IN ('hours_24','hours_48')
             GROUP BY b.insee_code,b.name,b.department_code
             HAVING COUNT(*) FILTER (WHERE r.horizon='hours_24')>0
                AND COUNT(*) FILTER (WHERE r.horizon='hours_48')>0",
        )
        .bind(computed_at)
        .execute(&mut *tx)
        .await?;
        let (commune_count, mapped_cells): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),COALESCE(SUM(evaluated_cell_count),0)::bigint
             FROM blue_forecast_aggregate",
        )
        .fetch_one(&mut *tx)
        .await?;
        if commune_count == 0 || mapped_cells * 100 < forecast_cell_count * 99 {
            return Err(StoreError::SnapshotContract(format!(
                "commune coverage is {mapped_cells}/{forecast_cell_count} cells"
            )));
        }
        sqlx::query(
            "INSERT INTO blue.forecast_index_archives(
                bulletin_id,commune_codes,commune_count,code_order_checksum,
                p95_24h,max_24h,p95_48h,max_48h)
             SELECT $1::uuid,array_agg(insee_code ORDER BY insee_code),COUNT(*),
                encode(digest(string_agg(insee_code,',' ORDER BY insee_code),'sha256'),'hex'),
                string_agg(float4send(p95_24),''::bytea ORDER BY insee_code),
                string_agg(float4send(max_24),''::bytea ORDER BY insee_code),
                string_agg(float4send(p95_48),''::bytea ORDER BY insee_code),
                string_agg(float4send(max_48),''::bytea ORDER BY insee_code)
             FROM blue_forecast_aggregate",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO blue.forecast_alerts(
                bulletin_id,insee_code,commune_name,department_code,horizon,valid_at,
                alert_index,max_score,mean_score,physical_at_peak,human_at_peak,
                evaluated_cell_count,elevated_cell_count,critical_cell_count,risk_level,top_factors)
             SELECT $1::uuid,a.insee_code,a.commune_name,a.department_code,v.horizon,v.valid_at,
                v.alert_index,v.max_score,v.mean_score,v.physical,v.human,a.evaluated_cell_count,
                v.elevated,v.critical,CASE WHEN v.alert_index>=0.75 THEN 'critical'
                ELSE 'elevated' END,v.factors
             FROM blue_forecast_aggregate a CROSS JOIN LATERAL (VALUES
                ('hours_24',valid_at_24,p95_24,max_24,mean_24,physical_24,human_24,elevated_24,critical_24,factors_24),
                ('hours_48',valid_at_48,p95_48,max_48,mean_48,physical_48,human_48,elevated_48,critical_48,factors_48)
             ) v(horizon,valid_at,alert_index,max_score,mean_score,physical,human,elevated,critical,factors)
             WHERE v.alert_index>=0.65",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO blue.forecast_evaluations(alert_id,status)
             SELECT id,'pending' FROM blue.forecast_alerts WHERE bulletin_id=$1::uuid",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        let checksum: String = sqlx::query_scalar(
            "SELECT encode(digest(p95_24h||max_24h||p95_48h||max_48h,'sha256'),'hex')
             FROM blue.forecast_index_archives WHERE bulletin_id=$1::uuid",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE blue.forecast_bulletins SET mapped_cell_count=$2,
                unmapped_cell_count=forecast_cell_count-$2,commune_count=$3,
                alerts_24h=(SELECT COUNT(*) FROM blue.forecast_alerts WHERE bulletin_id=$1::uuid AND horizon='hours_24'),
                alerts_48h=(SELECT COUNT(*) FROM blue.forecast_alerts WHERE bulletin_id=$1::uuid AND horizon='hours_48'),
                checksum=$4,status='published',published_at=NOW()
             WHERE id=$1::uuid AND status='building'",
        )
        .bind(id)
        .bind(mapped_cells)
        .bind(commune_count)
        .bind(checksum)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.blue_bulletin(id)
            .await?
            .ok_or_else(|| StoreError::InvalidPersistedCount(0))
    }

    async fn blue_bulletin_by_logical_id(
        &self,
        logical_id: &str,
    ) -> Result<Option<BlueForecastBulletinRow>, StoreError> {
        let query =
            format!("SELECT {BULLETIN_COLUMNS} FROM blue.forecast_bulletins WHERE logical_id=$1");
        sqlx::query_as(&query)
            .bind(logical_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    /// Reads one immutable BLUE bulletin.
    ///
    /// # Errors
    ///
    /// Returns a database error when the read cannot be completed.
    pub async fn blue_bulletin(
        &self,
        id: &str,
    ) -> Result<Option<BlueForecastBulletinRow>, StoreError> {
        let query =
            format!("SELECT {BULLETIN_COLUMNS} FROM blue.forecast_bulletins WHERE id=$1::uuid");
        sqlx::query_as(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    /// Reads the latest published BLUE bulletin.
    ///
    /// # Errors
    ///
    /// Returns a database error when the read cannot be completed.
    pub async fn latest_blue_bulletin(
        &self,
    ) -> Result<Option<BlueForecastBulletinRow>, StoreError> {
        let query = format!(
            "SELECT {BULLETIN_COLUMNS} FROM blue.forecast_bulletins
             WHERE status='published' ORDER BY bulletin_date DESC LIMIT 1"
        );
        sqlx::query_as(&query)
            .fetch_optional(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    /// Lists recent published bulletins.
    ///
    /// # Errors
    ///
    /// Returns a database error when the read cannot be completed.
    pub async fn list_blue_bulletins(
        &self,
        limit: i64,
    ) -> Result<Vec<BlueForecastBulletinRow>, StoreError> {
        let query = format!(
            "SELECT {BULLETIN_COLUMNS} FROM blue.forecast_bulletins
             WHERE status='published' ORDER BY bulletin_date DESC LIMIT $1"
        );
        sqlx::query_as(&query)
            .bind(limit.clamp(1, 366))
            .fetch_all(&self.pool)
            .await
            .map_err(StoreError::from)
    }

    /// Lists readable alerts for one bulletin.
    ///
    /// # Errors
    ///
    /// Returns a database error when the read cannot be completed.
    pub async fn list_blue_alerts(
        &self,
        bulletin_id: &str,
        horizon: Option<&str>,
        limit: i64,
    ) -> Result<Vec<BlueForecastAlertRow>, StoreError> {
        sqlx::query_as(
            "SELECT a.id::text,a.bulletin_id::text,b.bulletin_date,b.issued_at,
                a.insee_code,a.commune_name,a.department_code,a.horizon,a.valid_at,
                a.alert_index,a.max_score,a.mean_score,a.physical_at_peak,a.human_at_peak,
                a.evaluated_cell_count,a.elevated_cell_count,a.critical_cell_count,
                a.risk_level,a.top_factors,e.status evaluation_status,e.evidence_count
             FROM blue.forecast_alerts a JOIN blue.forecast_bulletins b ON b.id=a.bulletin_id
             JOIN blue.forecast_evaluations e ON e.alert_id=a.id
             WHERE a.bulletin_id=$1::uuid AND ($2::text IS NULL OR a.horizon=$2)
             ORDER BY a.alert_index DESC,a.commune_name LIMIT $3",
        )
        .bind(bulletin_id)
        .bind(horizon)
        .bind(limit.clamp(1, 10_000))
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    /// Reads one alert and its current evidence status.
    ///
    /// # Errors
    ///
    /// Returns a database error when the read cannot be completed.
    pub async fn blue_alert(&self, id: &str) -> Result<Option<BlueForecastAlertRow>, StoreError> {
        sqlx::query_as(
            "SELECT a.id::text,a.bulletin_id::text,b.bulletin_date,b.issued_at,
                a.insee_code,a.commune_name,a.department_code,a.horizon,a.valid_at,
                a.alert_index,a.max_score,a.mean_score,a.physical_at_peak,a.human_at_peak,
                a.evaluated_cell_count,a.elevated_cell_count,a.critical_cell_count,
                a.risk_level,a.top_factors,e.status evaluation_status,e.evidence_count
             FROM blue.forecast_alerts a JOIN blue.forecast_bulletins b ON b.id=a.bulletin_id
             JOIN blue.forecast_evaluations e ON e.alert_id=a.id WHERE a.id=$1::uuid",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    /// Ensures a deterministic and geographically diversified proactive
    /// selection for a bulletin. Four of the twenty daily slots remain
    /// available for signal-triggered reactive reviews.
    ///
    /// # Errors
    ///
    /// Returns a database error when the selection cannot be persisted.
    pub async fn ensure_blue_evidence_cases(
        &self,
        bulletin_id: &str,
        limit: i64,
    ) -> Result<u64, StoreError> {
        let selection_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM blue.evidence_cases WHERE bulletin_id=$1::uuid
             )",
        )
        .bind(bulletin_id)
        .fetch_one(&self.pool)
        .await?;
        if selection_exists {
            return Ok(0);
        }
        let candidates: Vec<BlueEvidenceCandidate> = sqlx::query_as(
            "WITH current_bulletin AS (
                SELECT id,issued_at,bulletin_date
                FROM blue.forecast_bulletins WHERE id=$1::uuid
             ), previous_bulletin AS (
                SELECT previous.id
                FROM current_bulletin current
                JOIN LATERAL (
                    SELECT id FROM blue.forecast_bulletins
                    WHERE status='published' AND issued_at<current.issued_at
                    ORDER BY issued_at DESC LIMIT 1
                ) previous ON TRUE
             ), previous_scores AS (
                SELECT a.insee_code,MAX(a.alert_index) previous_score
                FROM blue.forecast_alerts a
                WHERE a.bulletin_id=(SELECT id FROM previous_bulletin)
                GROUP BY a.insee_code
             ), recent_selections AS (
                SELECT c.insee_code,MAX(b.bulletin_date) last_selected_date
                FROM blue.evidence_cases c
                JOIN blue.forecast_bulletins b ON b.id=c.bulletin_id
                WHERE b.issued_at<(SELECT issued_at FROM current_bulletin)
                GROUP BY c.insee_code
             ), per_commune AS (
                SELECT a.bulletin_id,a.insee_code,MAX(a.commune_name) commune_name,
                    MAX(a.department_code) department_code,MAX(a.alert_index) selection_score,
                    MAX(p.previous_score) previous_score,
                    COALESCE(MAX(r.last_selected_date)>=MAX(current.bulletin_date)-3,FALSE)
                        recently_selected,
                    (array_agg(a.id ORDER BY a.alert_index DESC)
                        FILTER (WHERE a.horizon='hours_24'))[1] alert_24h_id,
                    (array_agg(a.id ORDER BY a.alert_index DESC)
                        FILTER (WHERE a.horizon='hours_48'))[1] alert_48h_id,
                    MAX(a.valid_at) FILTER (WHERE a.horizon='hours_24') research_24h,
                    MAX(a.valid_at) FILTER (WHERE a.horizon='hours_48') research_48h
                FROM blue.forecast_alerts a
                JOIN current_bulletin current ON current.id=a.bulletin_id
                LEFT JOIN previous_scores p ON p.insee_code=a.insee_code
                LEFT JOIN recent_selections r ON r.insee_code=a.insee_code
                WHERE a.bulletin_id=$1::uuid
                GROUP BY a.bulletin_id,a.insee_code
             )
             SELECT bulletin_id::text,insee_code,commune_name,department_code,
                selection_score,previous_score,recently_selected,
                alert_24h_id::text,alert_48h_id::text,research_24h,research_48h
             FROM per_commune",
        )
        .bind(bulletin_id)
        .fetch_all(&self.pool)
        .await?;
        let selected = select_blue_evidence_candidates(
            &candidates,
            usize::try_from(limit.clamp(1, 20)).unwrap_or(PROACTIVE_EVIDENCE_LIMIT),
        );
        let mut tx = self.pool.begin().await?;
        let mut inserted = 0_u64;
        for (position, (candidate, selection_reason)) in selected.into_iter().enumerate() {
            let daily_rank = i16::try_from(position + 1).map_err(|_| {
                StoreError::SnapshotContract("BLUE evidence rank overflow".to_owned())
            })?;
            inserted += sqlx::query(
                "INSERT INTO blue.evidence_cases(
                    bulletin_id,insee_code,commune_name,department_code,daily_rank,
                    selection_score,selection_reason,alert_24h_id,alert_48h_id,
                    research_after,review_stage)
                 VALUES($1::uuid,$2,$3,$4,$5,$6,$7,$8::uuid,$9::uuid,
                    COALESCE($10,$11)+INTERVAL '3 hours',
                    CASE WHEN $8::uuid IS NOT NULL THEN 'hours_24' ELSE 'hours_48' END)
                 ON CONFLICT(bulletin_id,insee_code) DO NOTHING",
            )
            .bind(&candidate.bulletin_id)
            .bind(&candidate.insee_code)
            .bind(&candidate.commune_name)
            .bind(&candidate.department_code)
            .bind(daily_rank)
            .bind(candidate.selection_score)
            .bind(selection_reason)
            .bind(&candidate.alert_24h_id)
            .bind(&candidate.alert_48h_id)
            .bind(candidate.research_24h)
            .bind(candidate.research_48h)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok(inserted)
    }

    /// Adds at most four immediate reviews when a newly observed thermal
    /// signal intersects a high-risk forecast outside the proactive sample.
    /// The total daily evidence set remains capped at twenty cases.
    ///
    /// # Errors
    ///
    /// Returns a database error when reactive selection cannot be persisted.
    pub async fn ensure_blue_reactive_evidence_cases(
        &self,
        bulletin_id: &str,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            "WITH capacity AS (
                SELECT GREATEST(0,20-COUNT(*))::bigint available,
                    GREATEST(0,$2-COUNT(*) FILTER (
                        WHERE selection_reason='reactive_signal'))::bigint reactive_available,
                    COALESCE(MAX(daily_rank),0)::smallint current_rank
                FROM blue.evidence_cases WHERE bulletin_id=$1::uuid
             ), signal_candidates AS (
                SELECT DISTINCT ON (o.insee_code)
                    o.id observation_id,o.insee_code,o.commune_name,o.department_code,
                    o.occurred_at,MAX(m.forecast_score) OVER (PARTITION BY o.insee_code) selection_score
                FROM blue.ground_truth_matches m
                JOIN blue.ground_truth_observations o ON o.id=m.observation_id
                WHERE m.bulletin_id=$1::uuid AND o.evidence_class='satellite_signal'
                  AND m.classification='signal_covered'
                  AND NOT EXISTS (
                      SELECT 1 FROM blue.evidence_cases c
                      WHERE c.bulletin_id=m.bulletin_id AND c.insee_code=o.insee_code
                  )
                ORDER BY o.insee_code,o.occurred_at DESC,m.forecast_score DESC
             ), eligible AS (
                SELECT s.*,
                    (SELECT id FROM blue.forecast_alerts a
                     WHERE a.bulletin_id=$1::uuid AND a.insee_code=s.insee_code
                       AND a.horizon='hours_24' ORDER BY alert_index DESC LIMIT 1) alert_24h_id,
                    (SELECT id FROM blue.forecast_alerts a
                     WHERE a.bulletin_id=$1::uuid AND a.insee_code=s.insee_code
                       AND a.horizon='hours_48' ORDER BY alert_index DESC LIMIT 1) alert_48h_id,
                    (SELECT valid_at FROM blue.forecast_alerts a
                     WHERE a.bulletin_id=$1::uuid AND a.insee_code=s.insee_code
                       AND a.horizon='hours_24' ORDER BY alert_index DESC LIMIT 1) valid_24h,
                    (SELECT valid_at FROM blue.forecast_alerts a
                     WHERE a.bulletin_id=$1::uuid AND a.insee_code=s.insee_code
                       AND a.horizon='hours_48' ORDER BY alert_index DESC LIMIT 1) valid_48h
                FROM signal_candidates s
             ), ranked AS (
                SELECT e.*,ROW_NUMBER() OVER (
                    ORDER BY selection_score DESC,occurred_at DESC,insee_code
                ) reactive_rank
                FROM eligible e
             )
             INSERT INTO blue.evidence_cases(
                bulletin_id,insee_code,commune_name,department_code,daily_rank,
                selection_score,selection_reason,trigger_observation_id,
                alert_24h_id,alert_48h_id,research_after,review_stage)
             SELECT $1::uuid,r.insee_code,r.commune_name,r.department_code,
                (c.current_rank+r.reactive_rank)::smallint,r.selection_score,'reactive_signal',
                r.observation_id,r.alert_24h_id,r.alert_48h_id,NOW(),
                CASE WHEN r.valid_24h IS NOT NULL AND r.occurred_at<=r.valid_24h
                    THEN 'hours_24' ELSE 'hours_48' END
             FROM ranked r CROSS JOIN capacity c
             WHERE r.reactive_rank<=LEAST(c.available,c.reactive_available)
               AND (r.alert_24h_id IS NOT NULL OR r.alert_48h_id IS NOT NULL)
             ON CONFLICT(bulletin_id,insee_code) DO NOTHING",
        )
        .bind(bulletin_id)
        .bind(REACTIVE_EVIDENCE_LIMIT)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Lists the selected evidence cases and their cited sources.
    ///
    /// # Errors
    ///
    /// Returns a database error when the cases cannot be read.
    pub async fn list_blue_evidence_cases(
        &self,
        bulletin_id: &str,
    ) -> Result<Vec<BlueEvidenceCaseRow>, StoreError> {
        sqlx::query_as(
            "SELECT c.id::text,c.bulletin_id::text,b.bulletin_date,c.insee_code,
                c.commune_name,c.department_code,c.daily_rank,c.selection_score,c.selection_reason,
                a24.id::text alert_24h_id,a24.alert_index alert_24h_index,
                a24.valid_at alert_24h_valid_at,a48.id::text alert_48h_id,
                a48.alert_index alert_48h_index,a48.valid_at alert_48h_valid_at,
                c.research_after,c.review_stage,c.stage_attempt_count,c.next_attempt_at,
                c.provisional_verdict,
                c.provisional_confidence,c.provisional_summary,
                c.provisional_observed_event_at,c.provisional_observed_location,
                c.provisional_completed_at,c.status,c.verdict,c.confidence,c.summary,
                c.observed_event_at,c.observed_location,c.model,c.attempt_count,c.completed_at,
                COALESCE((SELECT jsonb_agg(jsonb_build_object(
                    'url',s.url,'title',s.title,'published_at',s.published_at,
                    'excerpt',s.excerpt,'domain',s.domain,
                    'relation_strength',s.relation_strength,
                    'review_horizon',r.review_horizon) ORDER BY s.id)
                 FROM blue.evidence_runs r JOIN blue.evidence_sources s ON s.run_id=r.id
                 WHERE r.case_id=c.id AND NOT EXISTS (
                    SELECT 1 FROM blue.evidence_invalidations i WHERE i.run_id=r.id
                 )),'[]'::jsonb) sources
             FROM blue.evidence_cases c
             JOIN blue.forecast_bulletins b ON b.id=c.bulletin_id
             LEFT JOIN blue.forecast_alerts a24 ON a24.id=c.alert_24h_id
             LEFT JOIN blue.forecast_alerts a48 ON a48.id=c.alert_48h_id
             WHERE c.bulletin_id=$1::uuid ORDER BY c.daily_rank",
        )
        .bind(bulletin_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    /// Builds an honest, read-only performance summary from completed evidence reviews.
    /// Missing evidence is kept distinct from a confirmed absence of fire.
    ///
    /// # Errors
    ///
    /// Returns a database error when the summary inputs cannot be read.
    pub async fn blue_performance_summary(
        &self,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) -> Result<BluePerformanceSummary, StoreError> {
        let (bulletin_count, period_start, period_end): (
            i64,
            Option<NaiveDate>,
            Option<NaiveDate>,
        ) = sqlx::query_as(
            "SELECT COUNT(*)::bigint,MIN(bulletin_date),MAX(bulletin_date)
             FROM blue.forecast_bulletins
             WHERE status='published'
               AND ($1::date IS NULL OR bulletin_date >= $1)
               AND ($2::date IS NULL OR bulletin_date <= $2)",
        )
        .bind(from)
        .bind(to)
        .fetch_one(&self.pool)
        .await?;
        let rows: Vec<BluePerformanceCase> = sqlx::query_as(
            "SELECT c.bulletin_id::text,b.bulletin_date,b.issued_at,c.daily_rank,
                a24.alert_index score_24h,a48.alert_index score_48h,
                c.provisional_verdict,c.provisional_confidence,
                c.provisional_observed_event_at,c.provisional_completed_at,
                c.verdict,c.confidence,c.observed_event_at,c.completed_at,
                COALESCE((SELECT COUNT(*) FROM blue.evidence_runs r
                    JOIN blue.evidence_sources s ON s.run_id=r.id
                    WHERE r.case_id=c.id AND r.review_horizon='hours_24'
                      AND NOT EXISTS (SELECT 1 FROM blue.evidence_invalidations i
                          WHERE i.run_id=r.id)),0)::bigint sources_24h,
                COALESCE((SELECT COUNT(*) FROM blue.evidence_runs r
                    JOIN blue.evidence_sources s ON s.run_id=r.id
                    WHERE r.case_id=c.id AND r.review_horizon='hours_48'
                      AND NOT EXISTS (SELECT 1 FROM blue.evidence_invalidations i
                          WHERE i.run_id=r.id)),0)::bigint sources_48h
             FROM blue.evidence_cases c
             JOIN blue.forecast_bulletins b ON b.id=c.bulletin_id AND b.status='published'
             LEFT JOIN blue.forecast_alerts a24 ON a24.id=c.alert_24h_id
             LEFT JOIN blue.forecast_alerts a48 ON a48.id=c.alert_48h_id
             WHERE ($1::date IS NULL OR b.bulletin_date >= $1)
               AND ($2::date IS NULL OR b.bulletin_date <= $2)
             ORDER BY b.bulletin_date DESC,c.daily_rank",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(build_blue_performance_summary(
            bulletin_count,
            period_start,
            period_end,
            &rows,
        ))
    }

    /// Atomically claims one due evidence case for the automatic reviewer.
    ///
    /// # Errors
    ///
    /// Returns a database error when a due case cannot be claimed.
    pub async fn claim_blue_evidence_case(&self) -> Result<Option<BlueEvidenceClaim>, StoreError> {
        sqlx::query_as(
            "WITH due AS (
                SELECT id FROM blue.evidence_cases
                WHERE status IN ('pending','retry_due') AND attempt_count < 6
                  AND stage_attempt_count < 3 AND review_stage IN ('hours_24','hours_48')
                  AND COALESCE(next_attempt_at,research_after) <= NOW()
                ORDER BY COALESCE(next_attempt_at,research_after),daily_rank
                FOR UPDATE SKIP LOCKED LIMIT 1
             ), claimed AS (
                UPDATE blue.evidence_cases c SET status='researching',
                    attempt_count=c.attempt_count+1,last_attempt_at=NOW(),updated_at=NOW()
                    ,stage_attempt_count=c.stage_attempt_count+1
                FROM due WHERE c.id=due.id RETURNING c.*
             )
             SELECT c.id::text,c.bulletin_id::text,b.bulletin_date,b.issued_at,
                c.insee_code,c.commune_name,c.department_code,c.daily_rank,c.selection_score,
                c.selection_reason,o.occurred_at trigger_observed_at,
                a24.alert_index alert_24h_index,a24.valid_at alert_24h_valid_at,
                a48.alert_index alert_48h_index,a48.valid_at alert_48h_valid_at,
                c.review_stage review_horizon,c.attempt_count,c.stage_attempt_count
             FROM claimed c JOIN blue.forecast_bulletins b ON b.id=c.bulletin_id
             LEFT JOIN blue.ground_truth_observations o ON o.id=c.trigger_observation_id
             LEFT JOIN blue.forecast_alerts a24 ON a24.id=c.alert_24h_id
             LEFT JOIN blue.forecast_alerts a48 ON a48.id=c.alert_48h_id",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    /// Starts one append-only evidence run after a case has been claimed.
    ///
    /// # Errors
    ///
    /// Returns a database error when the audit run cannot be created.
    pub async fn start_blue_evidence_run(
        &self,
        case_id: &str,
        attempt_no: i16,
        review_horizon: &str,
        stage_attempt_no: i16,
        request_checksum: &str,
        model: &str,
    ) -> Result<String, StoreError> {
        sqlx::query_scalar(
            "INSERT INTO blue.evidence_runs(
                case_id,attempt_no,review_horizon,stage_attempt_no,request_checksum,model)
             VALUES($1::uuid,$2,$3,$4,$5,$6) RETURNING id::text",
        )
        .bind(case_id)
        .bind(attempt_no)
        .bind(review_horizon)
        .bind(stage_attempt_no)
        .bind(request_checksum)
        .bind(model)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    /// Persists a completed review, its raw audit payload and cited sources.
    ///
    /// # Errors
    ///
    /// Returns a database error when the result transaction cannot be committed.
    #[allow(clippy::too_many_lines)]
    pub async fn complete_blue_evidence_run(
        &self,
        case_id: &str,
        run_id: &str,
        review_horizon: &str,
        model: &str,
        result: &BlueEvidenceResult,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE blue.evidence_runs SET response_id=$2,status='completed',raw_response=$3,
                input_tokens=$4,output_tokens=$5,web_search_count=$6,verdict=$7,
                confidence=$8,summary=$9,observed_event_at=$10,observed_location=$11,
                completed_at=NOW()
             WHERE id=$1::uuid AND status='started'",
        )
        .bind(run_id)
        .bind(&result.response_id)
        .bind(&result.raw_response)
        .bind(result.input_tokens)
        .bind(result.output_tokens)
        .bind(result.web_search_count)
        .bind(&result.verdict)
        .bind(result.confidence)
        .bind(&result.summary)
        .bind(result.observed_event_at)
        .bind(&result.observed_location)
        .execute(&mut *tx)
        .await?;
        for source in &result.sources {
            sqlx::query(
                "INSERT INTO blue.evidence_sources(
                    run_id,url,title,published_at,excerpt,domain,relation_strength)
                 VALUES($1::uuid,$2,$3,$4,$5,$6,$7) ON CONFLICT(run_id,url) DO NOTHING",
            )
            .bind(run_id)
            .bind(&source.url)
            .bind(&source.title)
            .bind(source.published_at)
            .bind(&source.excerpt)
            .bind(&source.domain)
            .bind(&source.relation_strength)
            .execute(&mut *tx)
            .await?;
        }
        match review_horizon {
            "hours_24" => {
                sqlx::query(
                    "UPDATE blue.evidence_cases c SET
                        provisional_verdict=$2,provisional_confidence=$3,
                        provisional_summary=$4,provisional_observed_event_at=$5,
                        provisional_observed_location=$6,provisional_completed_at=NOW(),
                        verdict=$2,confidence=$3,summary=$4,observed_event_at=$5,
                        observed_location=$6,response_id=$7,model=$8,
                        status=CASE WHEN c.alert_48h_id IS NULL THEN 'reviewed' ELSE 'pending' END,
                        review_stage=CASE WHEN c.alert_48h_id IS NULL
                            THEN 'completed' ELSE 'hours_48' END,
                        stage_attempt_count=0,next_attempt_at=NULL,
                        research_after=COALESCE((SELECT valid_at+INTERVAL '3 hours'
                            FROM blue.forecast_alerts WHERE id=c.alert_48h_id),c.research_after),
                        completed_at=CASE WHEN c.alert_48h_id IS NULL THEN NOW() ELSE NULL END,
                        updated_at=NOW()
                     WHERE c.id=$1::uuid AND c.status='researching'",
                )
                .bind(case_id)
                .bind(&result.verdict)
                .bind(result.confidence)
                .bind(&result.summary)
                .bind(result.observed_event_at)
                .bind(&result.observed_location)
                .bind(&result.response_id)
                .bind(model)
                .execute(&mut *tx)
                .await?;
            }
            "hours_48" => {
                let retry = result.verdict == "no_evidence_found";
                sqlx::query(
                    "UPDATE blue.evidence_cases SET status=CASE
                            WHEN $9 AND stage_attempt_count < 2 THEN 'retry_due'
                            ELSE 'reviewed' END,
                        review_stage=CASE WHEN $9 AND stage_attempt_count < 2
                            THEN 'hours_48' ELSE 'completed' END,
                        verdict=$2,confidence=$3,summary=$4,observed_event_at=$5,
                        observed_location=$6,response_id=$7,model=$8,
                        next_attempt_at=CASE WHEN $9 AND stage_attempt_count < 2
                            THEN NOW()+INTERVAL '72 hours' ELSE NULL END,
                        completed_at=CASE WHEN $9 AND stage_attempt_count < 2
                            THEN NULL ELSE NOW() END,updated_at=NOW()
                     WHERE id=$1::uuid AND status='researching'",
                )
                .bind(case_id)
                .bind(&result.verdict)
                .bind(result.confidence)
                .bind(&result.summary)
                .bind(result.observed_event_at)
                .bind(&result.observed_location)
                .bind(&result.response_id)
                .bind(model)
                .bind(retry)
                .execute(&mut *tx)
                .await?;
            }
            value => {
                return Err(StoreError::SnapshotContract(format!(
                    "invalid BLUE evidence review horizon {value}"
                )));
            }
        }
        let evaluation_status = match result.verdict.as_str() {
            "confirmed" => "confirmed",
            "probable" => "probable",
            "signal_observed" => "signal_observed",
            _ => "inconclusive",
        };
        sqlx::query(
            "UPDATE blue.forecast_evaluations e SET status=$2,
                observed_event_at=$3,evidence_count=$4,reviewer_note=$5,
                reviewed_at=NOW(),updated_at=NOW()
             FROM blue.evidence_cases c
             WHERE c.id=$1::uuid AND e.alert_id=CASE WHEN $6='hours_24'
                THEN c.alert_24h_id ELSE c.alert_48h_id END",
        )
        .bind(case_id)
        .bind(evaluation_status)
        .bind(result.observed_event_at)
        .bind(i64::try_from(result.sources.len()).unwrap_or(i64::MAX))
        .bind(&result.summary)
        .bind(review_horizon)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Records a sanitized reviewer failure and schedules one bounded retry.
    ///
    /// # Errors
    ///
    /// Returns a database error when the failed run cannot be recorded.
    pub async fn fail_blue_evidence_run(
        &self,
        case_id: &str,
        run_id: &str,
        review_horizon: &str,
        error: &str,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await?;
        let safe_error = error.chars().take(500).collect::<String>();
        sqlx::query(
            "UPDATE blue.evidence_runs SET status='failed',error=$2,completed_at=NOW()
             WHERE id=$1::uuid AND status='started'",
        )
        .bind(run_id)
        .bind(&safe_error)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE blue.evidence_cases c SET
                status=CASE
                    WHEN stage_attempt_count < CASE
                        WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                        THEN 'retry_due'
                    WHEN $2='hours_24' AND alert_48h_id IS NOT NULL THEN 'pending'
                    ELSE 'failed' END,
                review_stage=CASE
                    WHEN stage_attempt_count < CASE
                        WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                        THEN review_stage
                    WHEN $2='hours_24' AND alert_48h_id IS NOT NULL THEN 'hours_48'
                    ELSE 'completed' END,
                stage_attempt_count=CASE
                    WHEN stage_attempt_count < CASE
                        WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                        THEN stage_attempt_count ELSE 0 END,
                verdict=CASE WHEN $2='hours_48' AND stage_attempt_count >= CASE
                    WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                    THEN 'inconclusive' ELSE verdict END,
                next_attempt_at=CASE WHEN stage_attempt_count < CASE
                    WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                    THEN NOW()+INTERVAL '6 hours' ELSE NULL END,
                research_after=CASE WHEN $2='hours_24' AND stage_attempt_count >= CASE
                    WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                    AND alert_48h_id IS NOT NULL THEN COALESCE((SELECT valid_at+INTERVAL '3 hours'
                        FROM blue.forecast_alerts WHERE id=c.alert_48h_id),research_after)
                    ELSE research_after END,
                completed_at=CASE WHEN $2='hours_48' AND stage_attempt_count >= CASE
                    WHEN $3 LIKE 'invalid evidence output:%' THEN 3 ELSE 2 END
                    THEN NOW() ELSE NULL END,updated_at=NOW()
             WHERE c.id=$1::uuid AND c.status='researching'",
        )
        .bind(case_id)
        .bind(review_horizon)
        .bind(&safe_error)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn rate(numerator: i64, denominator: i64) -> BlueRateMetric {
    BlueRateMetric {
        numerator,
        denominator,
        value: (denominator > 0)
            .then(|| bounded_i64_to_f64(numerator) / bounded_i64_to_f64(denominator)),
    }
}

fn select_blue_evidence_candidates(
    candidates: &[BlueEvidenceCandidate],
    requested_limit: usize,
) -> Vec<(BlueEvidenceCandidate, &'static str)> {
    let target = requested_limit
        .min(PROACTIVE_EVIDENCE_LIMIT)
        .min(candidates.len());
    let mut ranked = candidates.to_vec();
    ranked.sort_by(|left, right| {
        right
            .selection_score
            .total_cmp(&left.selection_score)
            .then_with(|| left.commune_name.cmp(&right.commune_name))
            .then_with(|| left.insee_code.cmp(&right.insee_code))
    });
    let mut selected = Vec::with_capacity(target);
    let mut seen = HashSet::with_capacity(target);

    // Keep a small stable baseline for the highest persistent risks. The
    // remaining proactive slots are deliberately rotated so the evidence
    // sample does not become a daily copy of the same national ranking.
    for candidate in ranked.iter().take(PERSISTENT_EVIDENCE_QUOTA) {
        add_blue_candidate(&mut selected, &mut seen, candidate, "national_top", target);
    }

    // A missing previous score means the commune has just crossed the alert
    // threshold. These cases are more informative than another unchanged
    // member of the persistent top ranking.
    let new_threshold = ranked
        .iter()
        .filter(|candidate| {
            candidate.previous_score.is_none()
                && !candidate.recently_selected
                && !seen.contains(&candidate.insee_code)
        })
        .take(NEW_THRESHOLD_EVIDENCE_QUOTA)
        .cloned()
        .collect::<Vec<_>>();
    for candidate in &new_threshold {
        add_blue_candidate(
            &mut selected,
            &mut seen,
            candidate,
            "risk_acceleration",
            target,
        );
    }

    let mut acceleration = ranked
        .iter()
        .filter(|candidate| {
            !seen.contains(&candidate.insee_code)
                && candidate.previous_score.is_some()
                && rotation_eligible(candidate)
        })
        .cloned()
        .collect::<Vec<_>>();
    acceleration.sort_by(|left, right| {
        let left_delta = left.selection_score - left.previous_score.unwrap_or(ALERT_THRESHOLD);
        let right_delta = right.selection_score - right.previous_score.unwrap_or(ALERT_THRESHOLD);
        right_delta
            .total_cmp(&left_delta)
            .then_with(|| right.selection_score.total_cmp(&left.selection_score))
            .then_with(|| left.insee_code.cmp(&right.insee_code))
    });
    for candidate in acceleration.iter().take(ACCELERATION_EVIDENCE_QUOTA) {
        add_blue_candidate(
            &mut selected,
            &mut seen,
            candidate,
            "risk_acceleration",
            target,
        );
    }

    let mut department_leaders = BTreeMap::<String, BlueEvidenceCandidate>::new();
    for candidate in &ranked {
        if seen.contains(&candidate.insee_code) || !rotation_eligible(candidate) {
            continue;
        }
        let Some(department) = candidate.department_code.as_ref() else {
            continue;
        };
        department_leaders
            .entry(department.clone())
            .or_insert_with(|| candidate.clone());
    }
    let mut department_leaders = department_leaders.into_values().collect::<Vec<_>>();
    department_leaders.sort_by(|left, right| {
        right
            .selection_score
            .total_cmp(&left.selection_score)
            .then_with(|| left.insee_code.cmp(&right.insee_code))
    });
    for candidate in department_leaders.iter().take(TERRITORIAL_EVIDENCE_QUOTA) {
        add_blue_candidate(
            &mut selected,
            &mut seen,
            candidate,
            "territorial_top",
            target,
        );
    }

    // Fill a short quota without breaking the three-day cooldown. This is
    // mostly relevant on unusually small bulletins.
    for candidate in ranked
        .iter()
        .filter(|candidate| rotation_eligible(candidate))
    {
        add_blue_candidate(&mut selected, &mut seen, candidate, "national_top", target);
    }

    // The bulletin must still produce a complete deterministic sample if too
    // few fresh communes exist. Persistent repeats are therefore the final,
    // explicit fallback rather than the default behaviour.
    for candidate in &ranked {
        add_blue_candidate(&mut selected, &mut seen, candidate, "national_top", target);
    }
    selected
}

fn rotation_eligible(candidate: &BlueEvidenceCandidate) -> bool {
    !candidate.recently_selected
        || candidate.previous_score.is_some_and(|previous| {
            candidate.selection_score - previous >= STRONG_ACCELERATION_DELTA
        })
}

fn add_blue_candidate(
    selected: &mut Vec<(BlueEvidenceCandidate, &'static str)>,
    seen: &mut HashSet<String>,
    candidate: &BlueEvidenceCandidate,
    reason: &'static str,
    target: usize,
) {
    if selected.len() < target && seen.insert(candidate.insee_code.clone()) {
        selected.push((candidate.clone(), reason));
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.collect();
    (!values.is_empty()).then(|| {
        values.iter().sum::<f64>() / f64::from(u32::try_from(values.len()).unwrap_or(u32::MAX))
    })
}

fn count_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn bounded_i64_to_f64(value: i64) -> f64 {
    f64::from(i32::try_from(value).unwrap_or_else(|_| {
        if value.is_negative() {
            i32::MIN
        } else {
            i32::MAX
        }
    }))
}

fn is_observed(verdict: &str) -> bool {
    matches!(verdict, "signal_observed" | "probable" | "confirmed")
}

fn horizon_verdict(row: &BluePerformanceCase, hours_24: bool) -> &str {
    if hours_24 {
        &row.provisional_verdict
    } else {
        &row.verdict
    }
}

#[allow(clippy::too_many_lines)]
fn horizon_performance(rows: &[BluePerformanceCase], hours_24: bool) -> BlueHorizonPerformance {
    let eligible: Vec<&BluePerformanceCase> = rows
        .iter()
        .filter(|row| {
            if hours_24 {
                row.score_24h
            } else {
                row.score_48h
            }
            .is_some()
        })
        .collect();
    let reviewed: Vec<&BluePerformanceCase> = eligible
        .iter()
        .copied()
        .filter(|row| {
            if hours_24 {
                row.provisional_completed_at.is_some()
            } else {
                row.completed_at.is_some()
            }
        })
        .collect();
    let observed = reviewed
        .iter()
        .filter(|row| is_observed(horizon_verdict(row, hours_24)))
        .count();
    let top_rate = |limit: i16| {
        let top: Vec<&&BluePerformanceCase> = reviewed
            .iter()
            .filter(|row| row.daily_rank <= limit)
            .collect();
        rate(
            count_i64(
                top.iter()
                    .filter(|row| is_observed(horizon_verdict(row, hours_24)))
                    .count(),
            ),
            count_i64(top.len()),
        )
    };
    let lead_times = reviewed.iter().filter_map(|row| {
        let event = if hours_24 {
            row.provisional_observed_event_at
        } else {
            row.observed_event_at
        }?;
        Some(bounded_i64_to_f64((event - row.issued_at).num_minutes()) / 60.0)
    });
    let eligible_count = count_i64(eligible.len());
    let reviewed_count = count_i64(reviewed.len());
    let observed_count = count_i64(observed);
    BlueHorizonPerformance {
        eligible_cases: eligible_count,
        reviewed_cases: reviewed_count,
        pending_cases: count_i64(eligible.len() - reviewed.len()),
        observed_signals: observed_count,
        no_evidence_found: count_i64(
            reviewed
                .iter()
                .filter(|row| horizon_verdict(row, hours_24) == "no_evidence_found")
                .count(),
        ),
        inconclusive: count_i64(
            reviewed
                .iter()
                .filter(|row| horizon_verdict(row, hours_24) == "inconclusive")
                .count(),
        ),
        evidence_sources: reviewed
            .iter()
            .map(|row| {
                if hours_24 {
                    row.sources_24h
                } else {
                    row.sources_48h
                }
            })
            .sum(),
        review_coverage: rate(reviewed_count, eligible_count),
        observed_signal_rate: rate(observed_count, reviewed_count),
        observed_signal_rate_at_5: top_rate(5),
        observed_signal_rate_at_10: top_rate(10),
        observed_signal_rate_at_20: top_rate(20),
        mean_score_reviewed: mean(reviewed.iter().filter_map(|row| {
            if hours_24 {
                row.score_24h
            } else {
                row.score_48h
            }
            .map(f64::from)
        })),
        mean_score_observed: mean(reviewed.iter().filter_map(|row| {
            is_observed(horizon_verdict(row, hours_24)).then_some(
                if hours_24 {
                    row.score_24h
                } else {
                    row.score_48h
                }
                .map(f64::from),
            )?
        })),
        mean_confidence: mean(reviewed.iter().filter_map(|row| {
            if hours_24 {
                row.provisional_confidence
            } else {
                row.confidence
            }
            .map(f64::from)
        })),
        mean_lead_time_hours: mean(lead_times),
    }
}

fn build_blue_performance_summary(
    bulletin_count: i64,
    period_start: Option<NaiveDate>,
    period_end: Option<NaiveDate>,
    rows: &[BluePerformanceCase],
) -> BluePerformanceSummary {
    let mut daily: BTreeMap<(NaiveDate, String), BlueBulletinPerformanceRow> = BTreeMap::new();
    for row in rows {
        let item = daily
            .entry((row.bulletin_date, row.bulletin_id.clone()))
            .or_insert_with(|| BlueBulletinPerformanceRow {
                bulletin_id: row.bulletin_id.clone(),
                bulletin_date: row.bulletin_date,
                selected_cases: 0,
                reviewed_24h: 0,
                reviewed_48h: 0,
                observed_24h: 0,
                observed_48h: 0,
                evidence_sources: 0,
            });
        item.selected_cases += 1;
        item.reviewed_24h += i64::from(row.provisional_completed_at.is_some());
        item.reviewed_48h += i64::from(row.completed_at.is_some());
        item.observed_24h += i64::from(
            row.provisional_completed_at.is_some() && is_observed(&row.provisional_verdict),
        );
        item.observed_48h += i64::from(row.completed_at.is_some() && is_observed(&row.verdict));
        item.evidence_sources += row.sources_24h + row.sources_48h;
    }
    BluePerformanceSummary {
        generated_at: Utc::now(),
        period_start,
        period_end,
        bulletin_count,
        selected_case_count: count_i64(rows.len()),
        hours_24: horizon_performance(rows, true),
        hours_48: horizon_performance(rows, false),
        bulletins: daily.into_values().rev().collect(),
        unavailable_metrics: vec![
            "recall",
            "false_negative_rate",
            "specificity",
            "territorial_accuracy",
            "calibrated_probability_accuracy",
        ],
        methodology: "Mesures limitées à un échantillon quotidien diversifié de communes à risque, complété par des recherches déclenchées par les signaux terrain. Une absence de preuve publiée n'est jamais interprétée comme une absence certaine d'incendie.",
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    fn candidate(
        index: usize,
        department: &str,
        score: f32,
        previous: Option<f32>,
        recently_selected: bool,
    ) -> BlueEvidenceCandidate {
        BlueEvidenceCandidate {
            bulletin_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            insee_code: format!("{index:05}"),
            commune_name: format!("Commune {index}"),
            department_code: Some(department.to_owned()),
            selection_score: score,
            previous_score: previous,
            recently_selected,
            alert_24h_id: Some(format!("00000000-0000-0000-0000-{index:012}")),
            alert_48h_id: None,
            research_24h: None,
            research_48h: None,
        }
    }

    fn case(verdict_24h: &str, verdict_48h: &str, final_ready: bool) -> BluePerformanceCase {
        let issued_at = DateTime::parse_from_rfc3339("2026-08-12T06:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        BluePerformanceCase {
            bulletin_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            bulletin_date: NaiveDate::from_ymd_opt(2026, 8, 12).expect("date"),
            issued_at,
            daily_rank: 1,
            score_24h: Some(0.91),
            score_48h: Some(0.87),
            provisional_verdict: verdict_24h.to_owned(),
            provisional_confidence: Some(0.8),
            provisional_observed_event_at: None,
            provisional_completed_at: Some(issued_at + chrono::Duration::hours(27)),
            verdict: verdict_48h.to_owned(),
            confidence: final_ready.then_some(0.75),
            observed_event_at: None,
            completed_at: final_ready.then_some(issued_at + chrono::Duration::hours(51)),
            sources_24h: 3,
            sources_48h: i64::from(final_ready) * 2,
        }
    }

    #[test]
    fn no_evidence_is_not_counted_as_an_observed_signal() {
        let rows = vec![case("no_evidence_found", "no_evidence_found", true)];
        let summary = build_blue_performance_summary(
            1,
            rows.first().map(|row| row.bulletin_date),
            rows.first().map(|row| row.bulletin_date),
            &rows,
        );
        assert_eq!(summary.hours_24.no_evidence_found, 1);
        assert_eq!(summary.hours_24.observed_signal_rate.denominator, 1);
        assert_eq!(summary.hours_24.observed_signal_rate.numerator, 0);
        assert_eq!(summary.hours_48.no_evidence_found, 1);
        assert!(summary.unavailable_metrics.contains(&"recall"));
    }

    #[test]
    fn pending_final_reviews_do_not_dilute_the_final_rate() {
        let rows = vec![case("signal_observed", "signal_observed", false)];
        let summary = build_blue_performance_summary(1, None, None, &rows);
        assert_eq!(summary.hours_24.observed_signal_rate.denominator, 1);
        assert_eq!(summary.hours_24.observed_signal_rate.numerator, 1);
        assert_eq!(summary.hours_48.pending_cases, 1);
        assert_eq!(summary.hours_48.observed_signal_rate.denominator, 0);
        assert_eq!(summary.hours_48.observed_signal_rate.value, None);
    }

    #[test]
    fn evidence_selection_combines_national_territorial_and_acceleration_cases() {
        let candidates = (0..32)
            .map(|index| {
                let score = 0.99 - f32::from(u16::try_from(index).expect("small index")) * 0.01;
                let previous = if (4..8).contains(&index) {
                    None
                } else if (8..12).contains(&index) {
                    Some(score - 0.10)
                } else {
                    Some(score - 0.01)
                };
                candidate(index, &format!("{:02}", index % 16), score, previous, false)
            })
            .collect::<Vec<_>>();
        let selected = select_blue_evidence_candidates(&candidates, 20);
        assert_eq!(selected.len(), PROACTIVE_EVIDENCE_LIMIT);
        assert_eq!(
            selected
                .iter()
                .filter(|(_, reason)| *reason == "national_top")
                .count(),
            PERSISTENT_EVIDENCE_QUOTA
        );
        assert_eq!(
            selected
                .iter()
                .filter(|(_, reason)| *reason == "territorial_top")
                .count(),
            TERRITORIAL_EVIDENCE_QUOTA
        );
        assert_eq!(
            selected
                .iter()
                .filter(|(_, reason)| *reason == "risk_acceleration")
                .count(),
            NEW_THRESHOLD_EVIDENCE_QUOTA + ACCELERATION_EVIDENCE_QUOTA
        );
        assert_eq!(
            selected
                .iter()
                .map(|(item, _)| &item.insee_code)
                .collect::<HashSet<_>>()
                .len(),
            selected.len()
        );
    }

    #[test]
    fn evidence_selection_applies_cooldown_outside_persistent_baseline() {
        let candidates = (0..28)
            .map(|index| {
                let score = 0.99 - f32::from(u16::try_from(index).expect("small index")) * 0.01;
                candidate(
                    index,
                    &format!("{:02}", index % 14),
                    score,
                    Some(score - 0.01),
                    index < 12,
                )
            })
            .collect::<Vec<_>>();
        let selected = select_blue_evidence_candidates(&candidates, 20);
        assert_eq!(selected.len(), PROACTIVE_EVIDENCE_LIMIT);
        assert!(
            selected
                .iter()
                .skip(PERSISTENT_EVIDENCE_QUOTA)
                .all(|(item, _)| !item.recently_selected)
        );
    }

    #[test]
    fn strong_acceleration_can_break_cooldown() {
        let item = candidate(1, "01", 0.90, Some(0.80), true);
        assert!(rotation_eligible(&item));
        let stable = candidate(2, "02", 0.90, Some(0.87), true);
        assert!(!rotation_eligible(&stable));
    }
}
