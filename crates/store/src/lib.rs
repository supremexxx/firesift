//! `PostgreSQL` persistence for `FireSift`.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use fwi::FwiState;
use grid::CellIndex;
use ingest::{Observation, calendar::CalendarDay};
use risk::{Factor, Horizon, RiskScore};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};

mod bdiff;
mod blue;
mod blue_ground_truth;
mod commune;
mod dataset;
mod firms;
mod model_candidate;
mod observability;
mod quality;
mod science;
mod shadow_scoring;

pub use bdiff::{BdiffImportIds, BdiffImportStart, BdiffPersistenceResult, BdiffTerminalState};
pub use blue::{
    BlueEvidenceCaseRow, BlueEvidenceClaim, BlueEvidenceResult, BlueEvidenceSourceInput,
    BlueForecastAlertRow, BlueForecastBulletinRow, BlueForecastContext, BlueHorizonPerformance,
    BluePerformanceSummary, BlueRateMetric,
};
pub use blue_ground_truth::{
    BlueGroundTruthConfirmationRow, BlueGroundTruthMatchRow, BlueGroundTruthRefresh,
    BlueGroundTruthSummary,
};
pub use commune::{CommuneBoundary, CommuneCatalogEntry};
pub use dataset::{
    AnyCauseEventForNegativeDesign, CalendarDayLookup, CalendarRuleVersion, DatasetBuildCounts,
    DatasetEventLinkRecord, DatasetExclusionRecord, DatasetRowRecord, DatasetVersionSpec,
    DatasetVersionSummary, FeatureSnapshotSpec, HistoricalCalendarDayRecord,
    HumanDatasetCandidateEvent, TrainingRow, dataset_row_count,
};
pub use firms::{FirmsImportIds, FirmsImportStart, FirmsPersistenceResult, FirmsTerminalState};
pub use model_candidate::{
    ModelCandidateRegistration, ModelCandidateRegistrationOutcome, ModelCandidateRow,
    ModelCandidateStatus,
};
pub use observability::{
    ComparisonEntry, DeferredLabelLinkReport, DeferredLabelSummary, FreshnessThresholds,
    HourlySnapshotSummary, ScientificSnapshotContext, ScientificSnapshotRow,
    ScientificSnapshotVerification, SnapshotAlertRow, SnapshotCaptureAttemptRow,
    SystemSnapshotContext, SystemSnapshotRow,
};
pub use quality::{
    CombustibilityAssessmentRecord, CombustibleCandidateRecord, CoordinateGroupRecord,
    DuplicateGroupRecord, DuplicateMemberRecord, DuplicatePairRecord, GeographicAssessmentRecord,
    LabelAssessmentRecord, QualityPersistenceBundle, QualityRuleVersion, QualitySourceEvent,
};
pub use science::{
    CalendarSummary, CategoryCount, DataQualitySummary, DatasetDetail, DatasetExclusionCount,
    DatasetSplitCount, DatasetVersionSummaryRow, FeatureSnapshotRow, IgnitionEventExplorationRow,
    ImportBatchRow, PipelineRunRow, ScienceOverview, SourceOverviewRow, SystemSummary,
};
pub use shadow_scoring::{ShadowInput, ShadowScoreWrite};

/// One complete daily FWI result ready for persistence.
#[derive(Clone, Copy, Debug)]
pub struct FwiStateRow {
    /// H3 target cell.
    pub cell: CellIndex,
    /// Calculation date.
    pub date: NaiveDate,
    /// Fine Fuel Moisture Code.
    pub ffmc: f64,
    /// Duff Moisture Code.
    pub dmc: f64,
    /// Drought Code.
    pub dc: f64,
    /// Initial Spread Index.
    pub isi: f64,
    /// Buildup Index.
    pub bui: f64,
    /// Fire Weather Index.
    pub fwi: f64,
}

/// Forecast FWI values associated with one prediction horizon.
#[derive(Clone, Copy, Debug)]
pub struct ForecastFwiRow {
    pub cell: CellIndex,
    pub computed_at: DateTime<Utc>,
    pub valid_at: DateTime<Utc>,
    pub horizon: Horizon,
    pub ffmc: f64,
    pub dmc: f64,
    pub dc: f64,
    pub isi: f64,
    pub bui: f64,
    pub fwi: f64,
}

/// Precomputed static feature document for one H3 cell.
#[derive(Clone, Debug)]
pub struct CellStaticRow {
    /// H3 target cell.
    pub cell: CellIndex,
    /// Typed feature document serialized as JSON.
    pub features: serde_json::Value,
}

/// Non-zero static feature coverage over a requested H3 AOI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StaticFeatureCoverage {
    /// Static documents found in `PostgreSQL`.
    pub static_rows: i64,
    /// Cells with non-zero road proximity.
    pub road_cells: i64,
    /// Cells classified as combustible land cover.
    pub combustible_cells: i64,
    /// Cells with non-zero population density.
    pub population_cells: i64,
    /// Cells with non-zero historical ignition density.
    pub history_cells: i64,
    /// Cells with non-zero wildland-urban interface exposure.
    pub wui_cells: i64,
    /// Cells with non-zero agricultural exposure.
    pub agriculture_cells: i64,
}

/// One historical ignition loaded for retrospective evaluation.
#[derive(Clone, Debug)]
pub struct HistoricalIgnition {
    /// H3 cell containing the public source coordinate.
    pub cell: CellIndex,
    /// Source alert timestamp normalized to UTC.
    pub occurred_at: DateTime<Utc>,
    /// Stable source identifier.
    pub source: String,
    /// Normalized source payload.
    pub payload: serde_json::Value,
}

/// One known human-caused ignition joined to its static cell features.
#[derive(Clone, Debug)]
pub struct HumanIgnitionSample {
    /// H3 cell containing the public source coordinate.
    pub cell: CellIndex,
    /// Calendar date used by the human model.
    pub date: NaiveDate,
    /// Static feature document available to operational scoring.
    pub features: serde_json::Value,
}

/// One persisted learned human-model version.
#[derive(Clone, Debug)]
pub struct HumanModelVersion {
    /// Monotonic database version identifier.
    pub id: i64,
    /// UTC training completion time.
    pub trained_at: DateTime<Utc>,
    /// Serialized [`risk::LearnedHumanModel`] artifact.
    pub artifact: serde_json::Value,
    /// Serialized validation metrics.
    pub metrics: serde_json::Value,
}

/// Physical and static inputs loaded for one risk calculation.
#[derive(Clone, Debug)]
pub struct RiskInputRow {
    /// H3 target cell.
    pub cell: CellIndex,
    /// Current Fire Weather Index.
    pub fwi: f32,
    /// Static feature document.
    pub features: serde_json::Value,
    /// School holiday flag for the input date.
    pub school_holiday: bool,
    /// Public holiday flag for the input date.
    pub public_holiday: bool,
}

/// One persisted risk score returned by read queries.
#[derive(Clone, Debug)]
pub struct StoredRiskScore {
    /// H3 target cell.
    pub cell: CellIndex,
    /// Calculation timestamp.
    pub computed_at: DateTime<Utc>,
    /// UTC time represented by the prediction.
    pub valid_at: DateTime<Utc>,
    /// Date of the FWI state used by the score.
    pub input_date: NaiveDate,
    /// Prediction horizon.
    pub horizon: Horizon,
    /// Fused score.
    pub score: f32,
    /// Physical component.
    pub physical: f32,
    /// Human component.
    pub human: f32,
    /// Explainable top factors.
    pub top_factors: Vec<Factor>,
}

/// FWI values associated with a detailed score response.
#[derive(Clone, Copy, Debug)]
pub struct FwiSnapshot {
    /// State date.
    pub date: NaiveDate,
    /// Exact forecast validity time.
    pub valid_at: DateTime<Utc>,
    /// Fine Fuel Moisture Code.
    pub ffmc: f64,
    /// Duff Moisture Code.
    pub dmc: f64,
    /// Drought Code.
    pub dc: f64,
    /// Initial Spread Index.
    pub isi: f64,
    /// Buildup Index.
    pub bui: f64,
    /// Fire Weather Index.
    pub fwi: f64,
}

/// Complete database payload needed by `GET /risk/cell/{h3}`.
#[derive(Clone, Debug)]
pub struct RiskCellData {
    /// Current score.
    pub current: StoredRiskScore,
    /// FWI state used by the current score.
    pub fwi: FwiSnapshot,
    /// Human static features.
    pub features: serde_json::Value,
    /// Score history over the preceding 24 hours.
    pub history: Vec<StoredRiskScore>,
}

/// Operational state for one external connector.
#[derive(Clone, Debug)]
pub struct SourceStatusRow {
    /// Stable connector identifier.
    pub id: String,
    /// Most recent execution attempt.
    pub last_run: DateTime<Utc>,
    /// Most recent successful execution.
    pub last_success: Option<DateTime<Utc>>,
    /// Number of normalized observations in the latest successful run.
    pub observation_count: u64,
    /// Error from the latest failed execution, cleared on success.
    pub recent_error: Option<String>,
}

/// Database access shared by application services.
#[derive(Clone, Debug)]
pub struct Store {
    pool: PgPool,
}

impl Store {
    /// Connects to `PostgreSQL` and applies pending migrations.
    ///
    /// # Errors
    ///
    /// Returns an error when the connection or a migration fails.
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPool::connect(database_url).await?;
        sqlx::migrate!("../../migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Verifies that `PostgreSQL` accepts a simple query.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` is unavailable.
    pub async fn health_check(&self) -> Result<(), StoreError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Inserts normalized observations while ignoring source duplicates.
    ///
    /// Returns the number of newly persisted rows.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction cannot be completed.
    pub async fn insert_observations(
        &self,
        observations: &[Observation],
    ) -> Result<u64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let mut inserted = 0;
        for observation in observations {
            inserted += insert_observation(&mut transaction, observation).await?;
        }
        transaction.commit().await?;
        Ok(inserted)
    }

    /// Loads moisture-code state for the requested cells on one exact date.
    ///
    /// Missing cells are intentionally omitted so callers can apply the
    /// standard FWI initial state.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn fwi_states(
        &self,
        date: NaiveDate,
        cells: &[CellIndex],
    ) -> Result<HashMap<CellIndex, FwiState>, StoreError> {
        if cells.is_empty() {
            return Ok(HashMap::new());
        }
        let database_cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT h3, ffmc, dmc, dc
             FROM fwi_state
             WHERE date = $1 AND h3 = ANY($2)",
        )
        .bind(date)
        .bind(&database_cells)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let cell = grid::cell_from_db(row.try_get("h3")?)?;
                let state = FwiState {
                    ffmc: row.try_get("ffmc")?,
                    dmc: row.try_get("dmc")?,
                    dc: row.try_get("dc")?,
                };
                Ok((cell, state))
            })
            .collect()
    }

    /// Inserts or replaces complete daily FWI values in one set-based query.
    ///
    /// Returns the number of target rows affected.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the upsert.
    pub async fn upsert_fwi_states(&self, states: &[FwiStateRow]) -> Result<u64, StoreError> {
        if states.is_empty() {
            return Ok(0);
        }
        let cells = states
            .iter()
            .map(|state| grid::cell_to_db(state.cell))
            .collect::<Vec<_>>();
        let dates = states.iter().map(|state| state.date).collect::<Vec<_>>();
        let ffmc = states.iter().map(|state| state.ffmc).collect::<Vec<_>>();
        let dmc = states.iter().map(|state| state.dmc).collect::<Vec<_>>();
        let dc = states.iter().map(|state| state.dc).collect::<Vec<_>>();
        let isi = states.iter().map(|state| state.isi).collect::<Vec<_>>();
        let bui = states.iter().map(|state| state.bui).collect::<Vec<_>>();
        let fwi = states.iter().map(|state| state.fwi).collect::<Vec<_>>();

        let result = sqlx::query(
            "INSERT INTO fwi_state (h3, date, ffmc, dmc, dc, isi, bui, fwi)
             SELECT * FROM UNNEST(
                 $1::bigint[], $2::date[], $3::double precision[],
                 $4::double precision[], $5::double precision[],
                 $6::double precision[], $7::double precision[],
                 $8::double precision[]
             )
             ON CONFLICT (h3, date) DO UPDATE SET
                 ffmc = EXCLUDED.ffmc,
                 dmc = EXCLUDED.dmc,
                 dc = EXCLUDED.dc,
                 isi = EXCLUDED.isi,
                 bui = EXCLUDED.bui,
                 fwi = EXCLUDED.fwi",
        )
        .bind(cells)
        .bind(dates)
        .bind(ffmc)
        .bind(dmc)
        .bind(dc)
        .bind(isi)
        .bind(bui)
        .bind(fwi)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Inserts or replaces FWI values for forecast horizons.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the upsert.
    pub async fn upsert_forecast_fwi(&self, states: &[ForecastFwiRow]) -> Result<u64, StoreError> {
        if states.is_empty() {
            return Ok(0);
        }
        let cells = states
            .iter()
            .map(|state| grid::cell_to_db(state.cell))
            .collect::<Vec<_>>();
        let computed_at = states
            .iter()
            .map(|state| state.computed_at)
            .collect::<Vec<_>>();
        let valid_at = states
            .iter()
            .map(|state| state.valid_at)
            .collect::<Vec<_>>();
        let horizons = states
            .iter()
            .map(|state| state.horizon.as_str())
            .collect::<Vec<_>>();
        let ffmc = states.iter().map(|state| state.ffmc).collect::<Vec<_>>();
        let dmc = states.iter().map(|state| state.dmc).collect::<Vec<_>>();
        let dc = states.iter().map(|state| state.dc).collect::<Vec<_>>();
        let isi = states.iter().map(|state| state.isi).collect::<Vec<_>>();
        let bui = states.iter().map(|state| state.bui).collect::<Vec<_>>();
        let fwi = states.iter().map(|state| state.fwi).collect::<Vec<_>>();
        let result = sqlx::query(
            "INSERT INTO forecast_fwi
                (h3, computed_at, valid_at, horizon, ffmc, dmc, dc, isi, bui, fwi)
             SELECT * FROM UNNEST(
                 $1::bigint[], $2::timestamptz[], $3::timestamptz[], $4::text[],
                 $5::double precision[], $6::double precision[], $7::double precision[],
                 $8::double precision[], $9::double precision[], $10::double precision[]
             )
             ON CONFLICT (h3, computed_at, horizon) DO UPDATE SET
                 valid_at = EXCLUDED.valid_at,
                 ffmc = EXCLUDED.ffmc,
                 dmc = EXCLUDED.dmc,
                 dc = EXCLUDED.dc,
                 isi = EXCLUDED.isi,
                 bui = EXCLUDED.bui,
                 fwi = EXCLUDED.fwi",
        )
        .bind(cells)
        .bind(computed_at)
        .bind(valid_at)
        .bind(horizons)
        .bind(ffmc)
        .bind(dmc)
        .bind(dc)
        .bind(isi)
        .bind(bui)
        .bind(fwi)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Inserts or updates historical ignition records by source identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the operation.
    pub async fn upsert_ignition_history(
        &self,
        observations: &[Observation],
    ) -> Result<u64, StoreError> {
        if observations.is_empty() {
            return Ok(0);
        }
        let occurred_at = observations
            .iter()
            .map(|observation| observation.observed_at)
            .collect::<Vec<_>>();
        let cells = observations
            .iter()
            .map(|observation| grid::cell_to_db(observation.cell))
            .collect::<Vec<_>>();
        let sources = observations
            .iter()
            .map(|observation| observation.source.clone())
            .collect::<Vec<_>>();
        let payloads = observations
            .iter()
            .map(|observation| observation.payload.to_string())
            .collect::<Vec<_>>();
        let dedupe_keys = observations
            .iter()
            .map(|observation| observation.dedupe_key.clone())
            .collect::<Vec<_>>();
        let result = sqlx::query(
            "INSERT INTO ignition_history
                (occurred_at, h3, source, payload, dedupe_key)
             SELECT input.occurred_at, input.h3, input.source,
                    input.payload::jsonb, input.dedupe_key
             FROM UNNEST(
                 $1::timestamptz[], $2::bigint[], $3::text[], $4::text[], $5::text[]
             ) AS input(occurred_at, h3, source, payload, dedupe_key)
             ON CONFLICT (source, dedupe_key) WHERE dedupe_key IS NOT NULL
             DO UPDATE SET
                 occurred_at = EXCLUDED.occurred_at,
                 h3 = EXCLUDED.h3,
                 payload = EXCLUDED.payload",
        )
        .bind(occurred_at)
        .bind(cells)
        .bind(sources)
        .bind(payloads)
        .bind(dedupe_keys)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Inserts or replaces all static feature documents in one set-based query.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the operation.
    pub async fn upsert_cell_static(&self, rows: &[CellStaticRow]) -> Result<u64, StoreError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let cells = rows
            .iter()
            .map(|row| grid::cell_to_db(row.cell))
            .collect::<Vec<_>>();
        let features = rows
            .iter()
            .map(|row| row.features.to_string())
            .collect::<Vec<_>>();
        let result = sqlx::query(
            "INSERT INTO cell_static (h3, features)
             SELECT input.h3, input.features::jsonb
             FROM UNNEST($1::bigint[], $2::text[]) AS input(h3, features)
             ON CONFLICT (h3) DO UPDATE SET
                 features = EXCLUDED.features,
                 updated_at = NOW()",
        )
        .bind(cells)
        .bind(features)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Updates only the normalized historical-ignition feature for existing cells.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the operation.
    pub async fn update_cell_history(&self, rows: &[(CellIndex, f64)]) -> Result<u64, StoreError> {
        if rows.is_empty() {
            return Ok(0);
        }
        let cells = rows
            .iter()
            .map(|(cell, _)| grid::cell_to_db(*cell))
            .collect::<Vec<_>>();
        let values = rows.iter().map(|(_, value)| *value).collect::<Vec<_>>();
        let result = sqlx::query(
            "UPDATE cell_static AS target
             SET features = jsonb_set(target.features, '{hist}', to_jsonb(input.hist), true),
                 updated_at = NOW()
             FROM UNNEST($1::bigint[], $2::double precision[]) AS input(h3, hist)
             WHERE target.h3 = input.h3",
        )
        .bind(cells)
        .bind(values)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Inserts or replaces normalized calendar days.
    ///
    /// # Errors
    ///
    /// Returns an error when `PostgreSQL` rejects the operation.
    pub async fn upsert_calendar_days(&self, days: &[CalendarDay]) -> Result<u64, StoreError> {
        if days.is_empty() {
            return Ok(0);
        }
        let dates = days.iter().map(|day| day.date).collect::<Vec<_>>();
        let school_holidays = days
            .iter()
            .map(|day| day.school_holiday)
            .collect::<Vec<_>>();
        let public_holidays = days
            .iter()
            .map(|day| day.public_holiday)
            .collect::<Vec<_>>();
        let labels = days.iter().map(|day| day.label.clone()).collect::<Vec<_>>();
        let result = sqlx::query(
            "INSERT INTO calendar_days
                (date, school_holiday, public_holiday, label)
             SELECT * FROM UNNEST($1::date[], $2::boolean[], $3::boolean[], $4::text[])
             ON CONFLICT (date) DO UPDATE SET
                 school_holiday = EXCLUDED.school_holiday,
                 public_holiday = EXCLUDED.public_holiday,
                 label = EXCLUDED.label,
                 updated_at = NOW()",
        )
        .bind(dates)
        .bind(school_holidays)
        .bind(public_holidays)
        .bind(labels)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Counts static rows among a requested set of cells.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn count_cell_static(&self, cells: &[CellIndex]) -> Result<i64, StoreError> {
        if cells.is_empty() {
            return Ok(0);
        }
        let cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let row = sqlx::query("SELECT COUNT(*) AS count FROM cell_static WHERE h3 = ANY($1)")
            .bind(cells)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("count")?)
    }

    /// Summarizes non-zero static feature coverage for a requested AOI.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn static_feature_coverage(
        &self,
        cells: &[CellIndex],
    ) -> Result<StaticFeatureCoverage, StoreError> {
        if cells.is_empty() {
            return Ok(StaticFeatureCoverage::default());
        }
        let database_cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let row = sqlx::query(
            "SELECT
                 COUNT(*) AS static_rows,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'road')::double precision, 0) > 0
                 ) AS road_cells,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'combustible')::boolean, false)
                 ) AS combustible_cells,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'population')::double precision, 0) > 0
                 ) AS population_cells,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'hist')::double precision, 0) > 0
                 ) AS history_cells,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'wui')::double precision, 0) > 0
                 ) AS wui_cells,
                 COUNT(*) FILTER (
                     WHERE COALESCE((features->>'agri')::double precision, 0) > 0
                 ) AS agriculture_cells
             FROM cell_static
             WHERE h3 = ANY($1)",
        )
        .bind(database_cells)
        .fetch_one(&self.pool)
        .await?;
        Ok(StaticFeatureCoverage {
            static_rows: row.try_get("static_rows")?,
            road_cells: row.try_get("road_cells")?,
            combustible_cells: row.try_get("combustible_cells")?,
            population_cells: row.try_get("population_cells")?,
            history_cells: row.try_get("history_cells")?,
            wui_cells: row.try_get("wui_cells")?,
            agriculture_cells: row.try_get("agriculture_cells")?,
        })
    }

    /// Loads static feature documents for the requested cells.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn cell_static_rows(
        &self,
        cells: &[CellIndex],
    ) -> Result<Vec<CellStaticRow>, StoreError> {
        if cells.is_empty() {
            return Ok(Vec::new());
        }
        let database_cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let rows =
            sqlx::query("SELECT h3, features FROM cell_static WHERE h3 = ANY($1) ORDER BY h3")
                .bind(database_cells)
                .fetch_all(&self.pool)
                .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CellStaticRow {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    features: row.try_get("features")?,
                })
            })
            .collect()
    }

    /// Loads all historical ignitions before the day following `to`.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn historical_ignitions_until(
        &self,
        to: NaiveDate,
    ) -> Result<Vec<HistoricalIgnition>, StoreError> {
        let rows = sqlx::query(
            "SELECT h3, occurred_at, source, payload
             FROM ignition_history
             WHERE occurred_at < ($1::date + INTERVAL '1 day')
             ORDER BY occurred_at, h3",
        )
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(HistoricalIgnition {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    occurred_at: row.try_get("occurred_at")?,
                    source: row.try_get("source")?,
                    payload: row.try_get("payload")?,
                })
            })
            .collect()
    }

    /// Loads BDIFF ignitions with a known human cause and complete burnable features.
    ///
    /// Unknown and natural causes are intentionally excluded from supervised labels.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn human_ignition_samples_between(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<HumanIgnitionSample>, StoreError> {
        let rows = sqlx::query(
            "SELECT history.h3, history.occurred_at::date AS date, cell.features
             FROM ignition_history AS history
             INNER JOIN cell_static AS cell ON cell.h3 = history.h3
             WHERE history.source = 'bdiff'
               AND history.occurred_at >= $1::date
               AND history.occurred_at < ($2::date + INTERVAL '1 day')
               AND history.payload->>'cause' IN (
                   'Malveillance',
                   'Involontaire (particulier)',
                   'Involontaire (travaux)',
                   'Accidentelle'
               )
               AND COALESCE((cell.features->>'combustible')::boolean, FALSE)
             ORDER BY history.occurred_at, history.h3",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(HumanIgnitionSample {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    date: row.try_get("date")?,
                    features: row.try_get("features")?,
                })
            })
            .collect()
    }

    /// Selects a deterministic national control-cell sample.
    ///
    /// # Errors
    ///
    /// Returns an error when the query, count conversion, or H3 decoding fails.
    pub async fn sample_combustible_cells(
        &self,
        count: usize,
        seed: i64,
    ) -> Result<Vec<CellStaticRow>, StoreError> {
        let count = i64::try_from(count).map_err(|_| StoreError::CountOverflow(count))?;
        let rows = sqlx::query(
            "SELECT h3, features
             FROM cell_static
             WHERE COALESCE((features->>'combustible')::boolean, FALSE)
             ORDER BY hashtextextended(h3::text, $2), h3
             LIMIT $1",
        )
        .bind(count)
        .bind(seed)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CellStaticRow {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    features: row.try_get("features")?,
                })
            })
            .collect()
    }

    /// Every `cell_static` row (all 920,016 at H3 resolution 9), for the
    /// phase 3B.5 resolution-9-to-8 feature aggregation
    /// (`dataset::features_h3`). One-time read per candidate-dataset build
    /// process, not per row; never mutates `public.cell_static`.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn all_cell_static_rows(&self) -> Result<Vec<CellStaticRow>, StoreError> {
        let rows = sqlx::query("SELECT h3, features FROM cell_static")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CellStaticRow {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    features: row.try_get("features")?,
                })
            })
            .collect()
    }

    /// Returns the active learned human model, if one has been trained.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn active_human_model(&self) -> Result<Option<HumanModelVersion>, StoreError> {
        let row = sqlx::query(
            "SELECT id, trained_at, artifact, metrics
             FROM human_model_versions
             WHERE active
             ORDER BY id DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(HumanModelVersion {
                id: row.try_get("id")?,
                trained_at: row.try_get("trained_at")?,
                artifact: row.try_get("artifact")?,
                metrics: row.try_get("metrics")?,
            })
        })
        .transpose()
    }

    /// Persists and atomically activates a learned human model.
    ///
    /// # Errors
    ///
    /// Returns an error when the database rejects the model version.
    #[allow(clippy::too_many_arguments)]
    pub async fn activate_human_model(
        &self,
        train_from: NaiveDate,
        train_to: NaiveDate,
        validation_from: NaiveDate,
        validation_to: NaiveDate,
        train_positive_count: usize,
        train_negative_count: usize,
        validation_positive_count: usize,
        validation_negative_count: usize,
        artifact: &serde_json::Value,
        metrics: &serde_json::Value,
    ) -> Result<i64, StoreError> {
        let train_positive_count = model_count(train_positive_count)?;
        let train_negative_count = model_count(train_negative_count)?;
        let validation_positive_count = model_count(validation_positive_count)?;
        let validation_negative_count = model_count(validation_negative_count)?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("UPDATE human_model_versions SET active = FALSE WHERE active")
            .execute(&mut *transaction)
            .await?;
        let row = sqlx::query(
            "INSERT INTO human_model_versions (
                 train_from, train_to, validation_from, validation_to,
                 train_positive_count, train_negative_count,
                 validation_positive_count, validation_negative_count,
                 artifact, metrics, active
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, TRUE)
             RETURNING id",
        )
        .bind(train_from)
        .bind(train_to)
        .bind(validation_from)
        .bind(validation_to)
        .bind(train_positive_count)
        .bind(train_negative_count)
        .bind(validation_positive_count)
        .bind(validation_negative_count)
        .bind(artifact)
        .bind(metrics)
        .fetch_one(&mut *transaction)
        .await?;
        let id = row.try_get("id")?;
        transaction.commit().await?;
        Ok(id)
    }

    /// Loads available calendar flags in an inclusive date interval.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn calendar_days_between(
        &self,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<CalendarDay>, StoreError> {
        let rows = sqlx::query(
            "SELECT date, school_holiday, public_holiday, label
             FROM calendar_days
             WHERE date BETWEEN $1 AND $2
             ORDER BY date",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(CalendarDay {
                    date: row.try_get("date")?,
                    school_holiday: row.try_get("school_holiday")?,
                    public_holiday: row.try_get("public_holiday")?,
                    label: row.try_get("label")?,
                })
            })
            .collect()
    }

    /// Returns the most recent FWI state date, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the query fails.
    pub async fn latest_fwi_date(&self) -> Result<Option<NaiveDate>, StoreError> {
        let row = sqlx::query("SELECT MAX(date) AS date FROM fwi_state")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get("date")?)
    }

    /// Loads all complete risk inputs for one FWI date.
    ///
    /// Cells missing either static features or FWI state are omitted.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or H3 decoding fails.
    pub async fn risk_inputs(
        &self,
        date: NaiveDate,
        cells: &[CellIndex],
    ) -> Result<Vec<RiskInputRow>, StoreError> {
        if cells.is_empty() {
            return Ok(Vec::new());
        }
        let cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT cell.h3, state.fwi::real AS fwi, cell.features,
                    COALESCE(day.school_holiday, FALSE) AS school_holiday,
                    COALESCE(day.public_holiday, FALSE) AS public_holiday
             FROM cell_static AS cell
             INNER JOIN fwi_state AS state
                ON state.h3 = cell.h3 AND state.date = $1
             LEFT JOIN calendar_days AS day ON day.date = $1
             WHERE cell.h3 = ANY($2)
             ORDER BY cell.h3",
        )
        .bind(date)
        .bind(cells)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(RiskInputRow {
                    cell: grid::cell_from_db(row.try_get("h3")?)?,
                    fwi: row.try_get("fwi")?,
                    features: row.try_get("features")?,
                    school_holiday: row.try_get("school_holiday")?,
                    public_holiday: row.try_get("public_holiday")?,
                })
            })
            .collect()
    }

    /// Persists one complete risk-calculation batch in a set-based query.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or the database operation fails.
    pub async fn upsert_risk_scores(
        &self,
        input_date: NaiveDate,
        scores: &[RiskScore],
    ) -> Result<u64, StoreError> {
        if scores.is_empty() {
            return Ok(0);
        }
        let cells = scores
            .iter()
            .map(|score| grid::cell_to_db(score.cell))
            .collect::<Vec<_>>();
        let computed_at = scores
            .iter()
            .map(|score| score.computed_at)
            .collect::<Vec<_>>();
        let valid_at = scores
            .iter()
            .map(|score| score.valid_at)
            .collect::<Vec<_>>();
        let input_dates = vec![input_date; scores.len()];
        let horizons = scores
            .iter()
            .map(|score| score.horizon.as_str())
            .collect::<Vec<_>>();
        let score_values = scores.iter().map(|score| score.score).collect::<Vec<_>>();
        let physical = scores
            .iter()
            .map(|score| score.physical)
            .collect::<Vec<_>>();
        let human = scores.iter().map(|score| score.human).collect::<Vec<_>>();
        let factors = scores
            .iter()
            .map(|score| serde_json::to_string(&score.top_factors))
            .collect::<Result<Vec<_>, _>>()?;
        let result = sqlx::query(
            "INSERT INTO risk_scores
                (h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors)
             SELECT input.h3, input.computed_at, input.valid_at, input.input_date, input.horizon,
                    input.score, input.physical, input.human, input.factors::jsonb
             FROM UNNEST(
                 $1::bigint[], $2::timestamptz[], $3::timestamptz[], $4::date[], $5::text[],
                 $6::real[], $7::real[], $8::real[], $9::text[]
             ) AS input(
                 h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             )
             ON CONFLICT (h3, computed_at, horizon) DO UPDATE SET
                 valid_at = EXCLUDED.valid_at,
                 input_date = EXCLUDED.input_date,
                 score = EXCLUDED.score,
                 physical = EXCLUDED.physical,
                 human = EXCLUDED.human,
                 factors = EXCLUDED.factors",
        )
        .bind(cells)
        .bind(computed_at)
        .bind(valid_at)
        .bind(input_dates)
        .bind(horizons)
        .bind(score_values)
        .bind(physical)
        .bind(human)
        .bind(factors)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Returns the latest score batch for requested cells above a threshold.
    ///
    /// # Errors
    ///
    /// Returns an error when the query, H3 decoding, or factor decoding fails.
    pub async fn latest_risk_scores(
        &self,
        cells: &[CellIndex],
        min_score: f32,
        horizon: Horizon,
    ) -> Result<Vec<StoredRiskScore>, StoreError> {
        if cells.is_empty() {
            return Ok(Vec::new());
        }
        let cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE horizon = $1
               AND computed_at = (
                   SELECT MAX(candidate.computed_at)
                   FROM risk_scores AS candidate
                   WHERE candidate.horizon = $1
                     AND NOT EXISTS (
                         SELECT 1 FROM forecast_batches AS batch
                         WHERE batch.computed_at = candidate.computed_at
                           AND batch.completed_at IS NULL
                     )
               )
               AND h3 = ANY($2)
               AND score >= $3
             ORDER BY h3",
        )
        .bind(horizon.as_str())
        .bind(cells)
        .bind(min_score)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(stored_risk_from_row).collect()
    }

    /// Returns a score-limited latest batch for map rendering.
    ///
    /// # Errors
    ///
    /// Returns an error when the query, H3 decoding, or factor decoding fails.
    pub async fn latest_risk_scores_limited(
        &self,
        cells: &[CellIndex],
        min_score: f32,
        horizon: Horizon,
        limit: u32,
    ) -> Result<Vec<StoredRiskScore>, StoreError> {
        if cells.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let cells = cells
            .iter()
            .copied()
            .map(grid::cell_to_db)
            .collect::<Vec<_>>();
        let rows = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE horizon = $1
               AND computed_at = (
                   SELECT MAX(candidate.computed_at)
                   FROM risk_scores AS candidate
                   WHERE candidate.horizon = $1
                     AND NOT EXISTS (
                         SELECT 1 FROM forecast_batches AS batch
                         WHERE batch.computed_at = candidate.computed_at
                           AND batch.completed_at IS NULL
                     )
               )
               AND h3 = ANY($2)
               AND score >= $3
             ORDER BY score DESC, h3
             LIMIT $4",
        )
        .bind(horizon.as_str())
        .bind(cells)
        .bind(min_score)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(stored_risk_from_row).collect()
    }

    /// Returns latest alerts above a threshold, sorted by decreasing score.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or decoding fails.
    pub async fn latest_alerts(
        &self,
        threshold: f32,
        horizon: Horizon,
    ) -> Result<Vec<StoredRiskScore>, StoreError> {
        let rows = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE horizon = $1
               AND computed_at = (
                   SELECT MAX(candidate.computed_at)
                   FROM risk_scores AS candidate
                   WHERE candidate.horizon = $1
                     AND NOT EXISTS (
                         SELECT 1 FROM forecast_batches AS batch
                         WHERE batch.computed_at = candidate.computed_at
                           AND batch.completed_at IS NULL
                     )
               )
               AND score >= $2
             ORDER BY score DESC, h3",
        )
        .bind(horizon.as_str())
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(stored_risk_from_row).collect()
    }

    /// Returns limited latest alerts ordered by decreasing score.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or decoding fails.
    pub async fn latest_alerts_limited(
        &self,
        threshold: f32,
        horizon: Horizon,
        limit: u32,
    ) -> Result<Vec<StoredRiskScore>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE horizon = $1
               AND computed_at = (
                   SELECT MAX(candidate.computed_at)
                   FROM risk_scores AS candidate
                   WHERE candidate.horizon = $1
                     AND NOT EXISTS (
                         SELECT 1 FROM forecast_batches AS batch
                         WHERE batch.computed_at = candidate.computed_at
                           AND batch.completed_at IS NULL
                     )
               )
               AND score >= $2
             ORDER BY score DESC, h3
             LIMIT $3",
        )
        .bind(horizon.as_str())
        .bind(threshold)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(stored_risk_from_row).collect()
    }

    /// Returns one cell's current details and 24-hour score history.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or decoding fails.
    pub async fn risk_cell(
        &self,
        cell: CellIndex,
        horizon: Horizon,
    ) -> Result<Option<RiskCellData>, StoreError> {
        let database_cell = grid::cell_to_db(cell);
        let current_row = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE h3 = $1 AND horizon = $2
               AND NOT EXISTS (
                   SELECT 1 FROM forecast_batches AS batch
                   WHERE batch.computed_at = risk_scores.computed_at
                     AND batch.completed_at IS NULL
               )
             ORDER BY computed_at DESC LIMIT 1",
        )
        .bind(database_cell)
        .bind(horizon.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let Some(current_row) = current_row else {
            return Ok(None);
        };
        let current = stored_risk_from_row(&current_row)?;
        let forecast_fwi = sqlx::query(
            "SELECT valid_at, ffmc, dmc, dc, isi, bui, fwi
             FROM forecast_fwi
             WHERE h3 = $1 AND computed_at = $2 AND horizon = $3",
        )
        .bind(database_cell)
        .bind(current.computed_at)
        .bind(horizon.as_str())
        .fetch_optional(&self.pool)
        .await?;
        let fwi = if let Some(row) = forecast_fwi {
            let valid_at: DateTime<Utc> = row.try_get("valid_at")?;
            FwiSnapshot {
                date: valid_at.date_naive(),
                valid_at,
                ffmc: row.try_get("ffmc")?,
                dmc: row.try_get("dmc")?,
                dc: row.try_get("dc")?,
                isi: row.try_get("isi")?,
                bui: row.try_get("bui")?,
                fwi: row.try_get("fwi")?,
            }
        } else {
            let row = sqlx::query(
                "SELECT date, ffmc, dmc, dc, isi, bui, fwi
                 FROM fwi_state WHERE h3 = $1 AND date = $2",
            )
            .bind(database_cell)
            .bind(current.input_date)
            .fetch_one(&self.pool)
            .await?;
            FwiSnapshot {
                date: row.try_get("date")?,
                valid_at: current.valid_at,
                ffmc: row.try_get("ffmc")?,
                dmc: row.try_get("dmc")?,
                dc: row.try_get("dc")?,
                isi: row.try_get("isi")?,
                bui: row.try_get("bui")?,
                fwi: row.try_get("fwi")?,
            }
        };
        let features_row = sqlx::query("SELECT features FROM cell_static WHERE h3 = $1")
            .bind(database_cell)
            .fetch_one(&self.pool)
            .await?;
        let history_rows = sqlx::query(
            "SELECT h3, computed_at, valid_at, input_date, horizon, score, physical, human, factors
             FROM risk_scores
             WHERE h3 = $1 AND horizon = $2
               AND computed_at >= $3 - INTERVAL '24 hours'
               AND NOT EXISTS (
                   SELECT 1 FROM forecast_batches AS batch
                   WHERE batch.computed_at = risk_scores.computed_at
                     AND batch.completed_at IS NULL
               )
             ORDER BY computed_at DESC",
        )
        .bind(database_cell)
        .bind(horizon.as_str())
        .bind(current.computed_at)
        .fetch_all(&self.pool)
        .await?;

        Ok(Some(RiskCellData {
            current,
            fwi,
            features: features_row.try_get("features")?,
            history: history_rows
                .iter()
                .map(stored_risk_from_row)
                .collect::<Result<Vec<_>, _>>()?,
        }))
    }

    /// Registers a forecast batch as running and therefore hidden from reads.
    ///
    /// # Errors
    ///
    /// Returns an error when the insert fails.
    pub async fn begin_forecast_batch(&self, computed_at: DateTime<Utc>) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO forecast_batches (computed_at, completed_at)
             VALUES ($1, NULL)
             ON CONFLICT (computed_at) DO UPDATE SET completed_at = NULL",
        )
        .bind(computed_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Deletes every row written by one failed forecast batch.
    ///
    /// # Errors
    ///
    /// Returns an error when the cleanup transaction fails.
    pub async fn abort_forecast_batch(&self, computed_at: DateTime<Utc>) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM forecast_fwi WHERE computed_at = $1")
            .bind(computed_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM risk_scores WHERE computed_at = $1")
            .bind(computed_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM forecast_batches WHERE computed_at = $1")
            .bind(computed_at)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Publishes one forecast batch and removes superseded operational batches.
    ///
    /// # Errors
    ///
    /// Returns an error when the transaction fails.
    pub async fn retain_forecast_batch(
        &self,
        computed_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("UPDATE forecast_batches SET completed_at = NOW() WHERE computed_at = $1")
            .bind(computed_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM forecast_fwi WHERE computed_at <> $1")
            .bind(computed_at)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "DELETE FROM risk_scores
             WHERE horizon IN ('nowcast', 'hours_6', 'hours_24', 'hours_48')
               AND computed_at <> $1",
        )
        .bind(computed_at)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "DELETE FROM forecast_batches AS batch
             WHERE batch.computed_at <> $1
               AND NOT EXISTS (
                   SELECT 1 FROM observability.scientific_snapshots AS snapshot
                   WHERE snapshot.forecast_batch_computed_at = batch.computed_at
               )
               AND NOT EXISTS (
                   SELECT 1 FROM blue.forecast_bulletins AS bulletin
                   WHERE bulletin.forecast_batch_computed_at = batch.computed_at
               )",
        )
        .bind(computed_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Records a successful connector execution.
    ///
    /// # Errors
    ///
    /// Returns an error when the upsert fails.
    pub async fn record_source_success(
        &self,
        id: &str,
        observation_count: usize,
    ) -> Result<(), StoreError> {
        let count = i64::try_from(observation_count)
            .map_err(|_| StoreError::CountOverflow(observation_count))?;
        sqlx::query(
            "INSERT INTO source_status
                (id, last_run, last_success, observation_count, recent_error)
             VALUES ($1, NOW(), NOW(), $2, NULL)
             ON CONFLICT (id) DO UPDATE SET
                last_run = NOW(),
                last_success = NOW(),
                observation_count = EXCLUDED.observation_count,
                recent_error = NULL",
        )
        .bind(id)
        .bind(count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records a failed connector execution without losing last-success data.
    ///
    /// # Errors
    ///
    /// Returns an error when the upsert fails.
    pub async fn record_source_error(&self, id: &str, error: &str) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO source_status
                (id, last_run, last_success, observation_count, recent_error)
             VALUES ($1, NOW(), NULL, 0, $2)
             ON CONFLICT (id) DO UPDATE SET
                last_run = NOW(),
                recent_error = EXCLUDED.recent_error",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Clears a stale `recent_error` left by a lower-priority fallback
    /// source that a poll cycle did not need to try because a
    /// higher-priority source in the same chain already succeeded.
    /// Never touches `last_run`/`last_success`/`observation_count`: a
    /// source that has not actually run keeps reporting that honestly.
    /// This only removes a message that would otherwise sit unchanged
    /// forever and misrepresent a long-past failure as a live one.
    ///
    /// # Errors
    ///
    /// Returns an error when the update fails.
    pub async fn clear_stale_source_error(&self, id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE source_status SET recent_error = NULL
             WHERE id = $1 AND recent_error IS NOT NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists operational source states in stable identifier order.
    ///
    /// # Errors
    ///
    /// Returns an error when the query or count conversion fails.
    pub async fn source_statuses(&self) -> Result<Vec<SourceStatusRow>, StoreError> {
        let rows = sqlx::query(
            "SELECT id, last_run, last_success, observation_count, recent_error
             FROM source_status ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let count: i64 = row.try_get("observation_count")?;
                Ok(SourceStatusRow {
                    id: row.try_get("id")?,
                    last_run: row.try_get("last_run")?,
                    last_success: row.try_get("last_success")?,
                    observation_count: u64::try_from(count)
                        .map_err(|_| StoreError::InvalidPersistedCount(count))?,
                    recent_error: row.try_get("recent_error")?,
                })
            })
            .collect()
    }
}

fn stored_risk_from_row(row: &PgRow) -> Result<StoredRiskScore, StoreError> {
    let horizon: String = row.try_get("horizon")?;
    let horizon = horizon
        .parse()
        .map_err(|_| StoreError::InvalidHorizon(horizon))?;
    let factors: serde_json::Value = row.try_get("factors")?;
    Ok(StoredRiskScore {
        cell: grid::cell_from_db(row.try_get("h3")?)?,
        computed_at: row.try_get("computed_at")?,
        valid_at: row.try_get("valid_at")?,
        input_date: row.try_get("input_date")?,
        horizon,
        score: row.try_get("score")?,
        physical: row.try_get("physical")?,
        human: row.try_get("human")?,
        top_factors: serde_json::from_value(factors)?,
    })
}

async fn insert_observation(
    transaction: &mut Transaction<'_, Postgres>,
    observation: &Observation,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO observations
            (source, kind, h3, observed_at, payload, dedupe_key)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (source, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING",
    )
    .bind(&observation.source)
    .bind(observation.kind.as_str())
    .bind(grid::cell_to_db(observation.cell))
    .bind(observation.observed_at)
    .bind(&observation.payload)
    .bind(&observation.dedupe_key)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

fn model_count(count: usize) -> Result<i32, StoreError> {
    i32::try_from(count).map_err(|_| StoreError::CountOverflow(count))
}

/// Persistence failures.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A `PostgreSQL` query or connection failed.
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    /// A database migration failed.
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// A persisted H3 index was invalid.
    #[error("invalid persisted H3 cell: {0}")]
    Grid(#[from] grid::GridError),
    /// A JSON document in the database was malformed.
    #[error("invalid persisted JSON document: {0}")]
    Json(#[from] serde_json::Error),
    /// A persisted risk horizon is unsupported.
    #[error("invalid persisted risk horizon: {0}")]
    InvalidHorizon(String),
    /// A source observation count could not fit the database type.
    #[error("source observation count does not fit in i64: {0}")]
    CountOverflow(usize),
    /// A valid BDIFF staging row lacked a required normalized value.
    #[error("valid BDIFF staging row is incomplete")]
    InvalidBdiffNormalizedRow,
    /// A persisted count was unexpectedly negative.
    #[error("invalid persisted non-negative count: {0}")]
    InvalidPersistedCount(i64),
    /// A hardened snapshot omitted a mandatory identity or provenance input.
    #[error("snapshot contract violation: {0}")]
    SnapshotContract(String),
    /// A stored H3 resolution did not fit the supported range.
    #[error("invalid persisted H3 resolution: {0}")]
    InvalidH3Resolution(i16),
    /// An existing immutable quality rule had different content.
    #[error("quality rule changed without a new logical version: {0}")]
    QualityRuleChanged(String),
    /// A required quality rule was not registered.
    #[error("missing quality rule id: {0}")]
    MissingQualityRule(String),
    /// A geographic assessment referenced an unavailable coordinate group.
    #[error("missing persisted coordinate group")]
    MissingCoordinateGroup,
    /// An existing immutable calendar rule had different content.
    #[error("calendar rule changed without a new logical version: {0}")]
    CalendarRuleChanged(String),
    /// A rebuild was attempted against a dataset version already finalized.
    #[error("dataset version {0} is finalized and immutable; use a new logical_id")]
    DatasetVersionFinalized(String),
    /// A rebuild under the same `logical_id` used different defining parameters.
    #[error(
        "dataset version {0} already exists with different defining parameters; \
         use a new logical_id"
    )]
    DatasetVersionParametersChanged(String),
    /// A model candidate's logical identity already exists with a
    /// different artifact checksum; registration must refuse rather
    /// than overwrite.
    #[error("model candidate checksum conflict: {0}")]
    ModelCandidateChecksumConflict(String),
    /// A persisted commune boundary was not a valid polygonal geometry.
    #[error("invalid persisted commune boundary: {0}")]
    InvalidCommuneBoundary(String),
}
