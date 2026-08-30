mod backtest;
mod bdiff_pipeline;
mod blue_evidence;
mod blue_feux_de_foret;
mod candidate_artifact;
mod candidate_load_verification;
mod candidate_pipeline;
mod commune_loader;
mod config;
mod dataset_pipeline;
mod export;
mod firms_pipeline;
mod forecast;
mod human_model;
mod model_experiments;
mod quality_pipeline;
mod risk_pipeline;
mod scheduler;
mod snapshot_pipeline;
mod static_layers;
mod territory;
mod v1_candidate_comparison;
mod weather;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::{Arc, OnceLock},
};

use anyhow::Context;
use api::AppState;
use chrono::{DateTime, NaiveDate, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use config::{Config, DataProfile, is_fixture_path};
use grid::H3Grid;
use ingest::{
    FetchCtx, Source,
    fire_history::FireHistorySource,
    firms::FirmsSource,
    meteo_france::MeteoFranceSource,
    osm::{OsmSource, write_aggregate_cache},
};
use risk::{HeuristicV1, LearnedHumanModel};
use store::{StaticFeatureCoverage, Store};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing_subscriber::EnvFilter;

static TRACING_INIT: OnceLock<()> = OnceLock::new();
const DEFAULT_BACKFILL_DAYS: u16 = 7;
const DEFAULT_BACKTEST_WARMUP_DAYS: u16 = 31;
const DEFAULT_NEGATIVES_PER_POSITIVE: usize = 4;
const FIRMS_GEOJSON_PATH: &str = "out/firms.geojson";
const RISK_UPDATE_CHANNEL_CAPACITY: usize = 8;

#[derive(Debug, Parser)]
#[command(
    name = "pyrorisk",
    version,
    about = "Hyperlocal wildfire ignition risk engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Runs database migrations and starts the HTTP service.
    Run,
    /// Backfills observations from one supported source.
    Backfill {
        /// Source connector to execute.
        #[arg(long, value_enum)]
        source: BackfillSource,
        /// Inclusive number of UTC days to retrieve.
        #[arg(long, default_value_t = DEFAULT_BACKFILL_DAYS)]
        days: u16,
    },
    /// Interpolates SYNOP weather and recomputes daily FWI for the AOI.
    Recompute {
        /// UTC date to recompute from the noon station observations.
        #[arg(long)]
        date: NaiveDate,
    },
    /// Fetches live AROME/ARPEGE forecasts and scores all prediction horizons.
    Forecast,
    /// Loads and precalculates all one-shot human static layers.
    LoadStatic,
    /// Loads one normalized fire-history export and refreshes its risk feature.
    LoadFireHistory {
        /// Historical source represented by the configured CSV path.
        #[arg(long, value_enum, default_value_t = FireHistorySelection::Bdiff)]
        source: FireHistorySelection,
    },
    /// Imports a normalized BDIFF CSV into the additive raw/staging/fire architecture.
    ImportBdiff {
        /// Normalized CSV file to import; the bundled fixture can be supplied explicitly.
        #[arg(long)]
        path: PathBuf,
        /// Parses and summarizes the file without connecting to `PostgreSQL`.
        #[arg(long)]
        dry_run: bool,
    },
    /// Computes versioned BDIFF quality assessments without changing source events.
    AuditBdiffQuality {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "v1")]
        rules_version: String,
        #[arg(long)]
        year: Option<i32>,
        #[arg(long)]
        source_record_id: Option<String>,
        #[arg(long)]
        recalculate: bool,
    },
    /// Registers the current `cell_static` bundle as a versioned feature snapshot.
    RegisterFeatureSnapshot {
        #[arg(long)]
        dry_run: bool,
    },
    /// Builds the versioned historical calendar (public holidays are exact;
    /// school holidays stay unavailable without a verified source).
    BuildHistoricalCalendar {
        #[arg(long, default_value_t = 2020)]
        from_year: i32,
        #[arg(long, default_value_t = 2026)]
        to_year: i32,
        #[arg(long)]
        dry_run: bool,
    },
    /// Builds a pilot human-ignition dataset (strict and inclusive variants).
    /// Not the final scientific dataset; see `PHASE3B3_DATASET_FOUNDATION_REPORT.md`.
    BuildHumanDataset {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 2_026_071)]
        seed: i64,
        #[arg(long, default_value_t = 25)]
        negatives_per_split_year: usize,
    },
    /// Prints read-only statistics for an existing dataset version.
    InspectDataset {
        #[arg(long)]
        dataset_version_id: String,
    },
    /// Builds the phase 3B.5 candidate datasets (strict/inclusive x N2/N3).
    /// For scientific review only; never finalized, never trained on.
    BuildCandidateDataset {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 2_026_071)]
        seed: i64,
        #[arg(long, default_value_t = 3)]
        ratio: u32,
    },
    /// Runs the phase 3B.7 experimental training/calibration/comparison.
    /// Never replaces the active v1 model; artifacts go to an isolated,
    /// disposable directory only.
    RunModelExperiments {
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value_t = 2_026_071)]
        seed: i64,
    },
    /// Runs the phase 3B.8 faithful v1-vs-candidate comparison on a
    /// shared 2025 population. Never retrains v1, never deploys, never
    /// modifies serving/API.
    RunV1Comparison {
        #[arg(long, default_value_t = 2_026_071)]
        seed: i64,
    },
    /// Runs the phase 3B.9 candidate artifact packaging, checksums,
    /// and training/inference + offline/online parity checks. Never
    /// writes to `human_model_versions`, never activates anything.
    PackageCandidateArtifact {
        #[arg(long, default_value_t = 2_026_071)]
        seed: i64,
    },
    /// Phase 3B.10 P1: registers exactly one, explicitly-verified
    /// candidate as `candidate`/`inactive` in
    /// `ml.model_candidate_registry`. Never touches `human_model_
    /// versions`, never loads the candidate into a scoring path.
    /// Every value is explicit; no field is inferred from "the latest"
    /// dataset/artifact/commit.
    RegisterModelCandidate {
        #[arg(long)]
        model_family: String,
        #[arg(long)]
        model_name: String,
        #[arg(long)]
        artifact_version: i32,
        /// Path to the exact P0 artifact JSON file (`package-candidate-
        /// artifact`'s output). Never rebuilt from a live dataset query.
        #[arg(long)]
        artifact_path: PathBuf,
        #[arg(long)]
        git_commit: String,
        #[arg(long)]
        dataset_logical_id: String,
        #[arg(long)]
        seed: i64,
        /// Either "candidate" or "inactive". "active" is not a valid value.
        #[arg(long)]
        status: String,
        #[arg(long)]
        expected_artifact_checksum: String,
        #[arg(long)]
        expected_gbm_checksum: String,
        #[arg(long)]
        expected_calibrator_checksum: String,
        #[arg(long)]
        expected_transforms_checksum: String,
        #[arg(long)]
        expected_feature_list_checksum: String,
    },
    /// Phase 3B.11 P2: reads the registered candidate strictly by id
    /// inside a real read-only transaction, validates it, and never
    /// scores anything. Every expected value is explicit; no default
    /// to "latest"/"first"/"active"/"newest".
    VerifyModelCandidateLoad {
        #[arg(long)]
        candidate_id: i64,
        #[arg(long)]
        expected_status: String,
        #[arg(long)]
        expected_artifact_checksum: String,
        #[arg(long)]
        expected_gbm_checksum: String,
        #[arg(long)]
        expected_calibrator_checksum: String,
        #[arg(long)]
        expected_transforms_checksum: String,
        #[arg(long)]
        expected_feature_list_checksum: String,
    },
    /// Pre-aggregates regional OSM PBF extracts into a reusable H3 cache.
    OsmAggregate {
        /// Destination newline-delimited JSON file.
        #[arg(long)]
        output: PathBuf,
    },
    /// Audits configured static files and database feature coverage.
    DataStatus,
    /// Plans H3 work partitions from French department boundaries.
    TerritoryPlan,
    /// Loads or replaces one commune boundary `GeoJSON` fixture, keyed by
    /// INSEE code, backing commune search/lookup (used by the Watch
    /// public map). Generic: reusable for any commune, not specific to
    /// a single one.
    LoadCommuneBoundary {
        /// Five-character INSEE municipality code, e.g. 31490.
        #[arg(long)]
        insee_code: String,
        /// Path to a `GeoJSON` file containing a Polygon/MultiPolygon
        /// geometry, or a single Feature/FeatureCollection wrapping one.
        #[arg(long)]
        geojson: PathBuf,
        /// Commune name.
        #[arg(long)]
        name: String,
        /// Comma-separated postal codes served by the commune.
        #[arg(long, value_delimiter = ',')]
        postal_codes: Vec<String>,
    },
    /// Loads the complete versioned commune catalog and deterministic H3
    /// ownership required by BLUE forecast bulletins.
    LoadCommuneCatalog {
        #[arg(long)]
        geojson: PathBuf,
        #[arg(long)]
        source_version: String,
    },
    /// Replays historical weather and evaluates observed ignitions.
    Backtest {
        /// First local calendar date to replay.
        #[arg(long)]
        from: NaiveDate,
        /// Final local calendar date to replay.
        #[arg(long)]
        to: NaiveDate,
        /// Number of preceding days used to initialize moisture codes.
        #[arg(long, default_value_t = DEFAULT_BACKTEST_WARMUP_DAYS)]
        warmup_days: u16,
    },
    /// Trains, validates, versions, and activates the human ignition model.
    TrainHumanModel {
        /// First date included in model fitting.
        #[arg(long, default_value = "2020-01-01")]
        train_from: NaiveDate,
        /// Final date included in model fitting.
        #[arg(long, default_value = "2024-12-31")]
        train_to: NaiveDate,
        /// First date in the untouched chronological validation set.
        #[arg(long, default_value = "2025-01-01")]
        validation_from: NaiveDate,
        /// Final date in the untouched chronological validation set.
        #[arg(long, default_value = "2025-12-31")]
        validation_to: NaiveDate,
        /// Deterministic non-event cell-days sampled per known human ignition.
        #[arg(long, default_value_t = DEFAULT_NEGATIVES_PER_POSITIVE)]
        negatives_per_positive: usize,
    },
    /// Phase 4A.5: captures one operational observability snapshot
    /// (aggregates and statuses only). Idempotent per (environment,
    /// `capture_date`, cadence). Never trains, scores, or activates
    /// anything.
    SnapshotOperational {
        /// RFC 3339 instant to capture as; defaults to now.
        #[arg(long)]
        at: Option<DateTime<Utc>>,
        #[arg(long, default_value = "daily")]
        cadence: String,
    },
    /// Phase 4A.5: captures (or resumes/no-ops on) the weekly
    /// `nowcast`-only scientific snapshot pilot for one calendar day.
    SnapshotScientific {
        /// Calendar date the snapshot is valid for (UTC midnight).
        #[arg(long)]
        date: NaiveDate,
    },
    /// Phase 4A.6: materializes and activates the immutable static input bundle.
    SnapshotStaticBundle,
    /// Phase 4A.6: publishes the configured territory as the modelable denominator.
    SnapshotCoverageMask,
    /// Phase 4A.6: previews or applies mature BDIFF links. Dry-run by default.
    SnapshotLinkLabels {
        #[arg(long)]
        snapshot_id: String,
        /// Only BDIFF events whose last update is at or before this instant are mature.
        #[arg(long)]
        mature_before: Option<DateTime<Utc>>,
        /// Explicit opt-in to writes; omission is a strict dry run.
        #[arg(long, default_value_t = false)]
        apply: bool,
        /// Maximum number of mature events examined in this bounded pilot.
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Phase 4A.5: verifies a published scientific snapshot is complete
    /// and immutable.
    SnapshotVerify {
        #[arg(long)]
        id: String,
    },
    /// Phase 4A.5: compares the latest daily snapshot to J-N for one or
    /// more values of N (e.g. `--days 1,7`).
    SnapshotCompare {
        #[arg(long, value_delimiter = ',', default_value = "1,7")]
        days: Vec<i64>,
    },
    /// Phase 4A.5: reports what a future retention pass would remove,
    /// without deleting anything. No deletion path exists yet.
    SnapshotRetention {
        #[arg(long, default_value_t = true)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackfillSource {
    /// NASA FIRMS VIIRS S-NPP active fires.
    Firms,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FireHistorySelection {
    /// National BDIFF database.
    Bdiff,
    /// Legacy Mediterranean Prométhée database.
    Promethee,
}

// A flat command dispatch match by design: one arm per CLI subcommand,
// each a one-line delegation. Splitting it further would just move the
// same dispatch into another function without reducing what it does.
#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let cli = Cli::parse();
    let config = Config::from_env().context("failed to load configuration")?;
    config.log_summary();

    match cli.command {
        Command::Run => run(config).await,
        Command::Backfill { source, days } => backfill(config, source, days).await,
        Command::Recompute { date } => recompute(config, date).await,
        Command::Forecast => forecast(config).await,
        Command::LoadStatic => load_static(config).await,
        Command::LoadFireHistory { source } => load_fire_history(config, source).await,
        Command::ImportBdiff { path, dry_run } => import_bdiff(config, &path, dry_run).await,
        Command::AuditBdiffQuality {
            dry_run,
            rules_version,
            year,
            source_record_id,
            recalculate,
        } => {
            quality_pipeline::audit_bdiff_quality(
                config,
                quality_pipeline::QualityOptions {
                    dry_run,
                    rules_version,
                    year,
                    source_record_id,
                    recalculate,
                },
            )
            .await
        }
        Command::RegisterFeatureSnapshot { dry_run } => {
            dataset_pipeline::register_feature_snapshot(
                config,
                dataset_pipeline::SnapshotOptions { dry_run },
            )
            .await
        }
        Command::BuildHistoricalCalendar {
            from_year,
            to_year,
            dry_run,
        } => {
            dataset_pipeline::build_historical_calendar(
                config,
                dataset_pipeline::CalendarOptions {
                    from_year,
                    to_year,
                    dry_run,
                },
            )
            .await
        }
        Command::BuildHumanDataset {
            dry_run,
            seed,
            negatives_per_split_year,
        } => {
            dataset_pipeline::build_human_dataset(
                config,
                dataset_pipeline::DatasetBuildOptions {
                    dry_run,
                    seed,
                    negatives_per_split_year,
                },
            )
            .await
        }
        Command::InspectDataset { dataset_version_id } => {
            inspect_dataset(config, &dataset_version_id).await
        }
        Command::BuildCandidateDataset {
            dry_run,
            seed,
            ratio,
        } => {
            candidate_pipeline::build_candidate_datasets(
                config,
                candidate_pipeline::CandidateBuildOptions {
                    dry_run,
                    seed,
                    ratio,
                },
            )
            .await
        }
        Command::RunModelExperiments { dry_run, seed } => {
            model_experiments::run_experiments(
                config,
                model_experiments::ExperimentOptions { dry_run, seed },
            )
            .await
        }
        Command::RunV1Comparison { seed } => {
            v1_candidate_comparison::run_v1_comparison(
                config,
                v1_candidate_comparison::ComparisonOptions { seed },
            )
            .await
        }
        Command::PackageCandidateArtifact { seed } => {
            candidate_artifact::run_packaging(config, candidate_artifact::PackagingOptions { seed })
                .await
        }
        Command::RegisterModelCandidate {
            model_family,
            model_name,
            artifact_version,
            artifact_path,
            git_commit,
            dataset_logical_id,
            seed,
            status,
            expected_artifact_checksum,
            expected_gbm_checksum,
            expected_calibrator_checksum,
            expected_transforms_checksum,
            expected_feature_list_checksum,
        } => {
            let status = match status.as_str() {
                "candidate" => store::ModelCandidateStatus::Candidate,
                "inactive" => store::ModelCandidateStatus::Inactive,
                other => anyhow::bail!(
                    "invalid --status {other:?}: must be \"candidate\" or \"inactive\""
                ),
            };
            candidate_artifact::run_register_model_candidate(
                config,
                candidate_artifact::RegisterCandidateOptions {
                    model_family,
                    model_name,
                    artifact_version,
                    artifact_path,
                    git_commit,
                    dataset_logical_id,
                    seed,
                    status,
                    expected_artifact_checksum,
                    expected_gbm_checksum,
                    expected_calibrator_checksum,
                    expected_transforms_checksum,
                    expected_feature_list_checksum,
                },
            )
            .await
        }
        Command::VerifyModelCandidateLoad {
            candidate_id,
            expected_status,
            expected_artifact_checksum,
            expected_gbm_checksum,
            expected_calibrator_checksum,
            expected_transforms_checksum,
            expected_feature_list_checksum,
        } => {
            candidate_load_verification::run_verify_load(
                config,
                candidate_load_verification::VerifyLoadOptions {
                    candidate_id,
                    expected_status,
                    expected_artifact_checksum,
                    expected_gbm_checksum,
                    expected_calibrator_checksum,
                    expected_transforms_checksum,
                    expected_feature_list_checksum,
                },
            )
            .await
        }
        Command::OsmAggregate { output } => osm_aggregate(&config, &output).await,
        Command::DataStatus => data_status(config).await,
        Command::TerritoryPlan => territory_plan(&config),
        Command::LoadCommuneBoundary {
            insee_code,
            geojson,
            name,
            postal_codes,
        } => load_commune_boundary(config, &insee_code, &geojson, &name, &postal_codes).await,
        Command::LoadCommuneCatalog {
            geojson,
            source_version,
        } => load_commune_catalog(config, &geojson, &source_version).await,
        Command::Backtest {
            from,
            to,
            warmup_days,
        } => backtest(config, from, to, warmup_days).await,
        Command::TrainHumanModel {
            train_from,
            train_to,
            validation_from,
            validation_to,
            negatives_per_positive,
        } => {
            train_human_model(
                config,
                human_model::TrainingOptions {
                    train_from,
                    train_to,
                    validation_from,
                    validation_to,
                    negatives_per_positive,
                },
            )
            .await
        }
        Command::SnapshotOperational { at, cadence } => {
            snapshot_pipeline::run_operational_snapshot(
                config,
                snapshot_pipeline::OperationalSnapshotOptions { at, cadence },
            )
            .await
        }
        Command::SnapshotScientific { date } => {
            let valid_at = date
                .and_hms_opt(0, 0, 0)
                .context("invalid calendar date")?
                .and_utc();
            snapshot_pipeline::run_scientific_snapshot(
                config,
                snapshot_pipeline::ScientificSnapshotOptions { valid_at },
            )
            .await
        }
        Command::SnapshotStaticBundle => snapshot_pipeline::run_static_bundle(config).await,
        Command::SnapshotCoverageMask => snapshot_pipeline::run_coverage_mask(config).await,
        Command::SnapshotLinkLabels {
            snapshot_id,
            mature_before,
            apply,
            limit,
        } => {
            snapshot_pipeline::run_label_linking(config, snapshot_id, mature_before, apply, limit)
                .await
        }
        Command::SnapshotVerify { id } => {
            snapshot_pipeline::run_verify_snapshot(
                config,
                snapshot_pipeline::VerifySnapshotOptions { id },
            )
            .await
        }
        Command::SnapshotCompare { days } => {
            snapshot_pipeline::run_compare_snapshots(
                config,
                snapshot_pipeline::CompareSnapshotOptions { days },
            )
            .await
        }
        Command::SnapshotRetention { dry_run } => {
            anyhow::ensure!(
                dry_run,
                "only --dry-run is supported in this phase; no deletion path exists"
            );
            snapshot_pipeline::run_retention_dry_run(config).await
        }
    }
}

async fn train_human_model(
    config: Config,
    options: human_model::TrainingOptions,
) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let summary = human_model::train_and_activate(&store, options).await?;
    tracing::info!(
        version = summary.version,
        train_positives = summary.train_positive_count,
        train_negatives = summary.train_negative_count,
        validation_positives = summary.validation_positive_count,
        validation_negatives = summary.validation_negative_count,
        validation_roc_auc = summary.validation_roc_auc,
        validation_average_precision = summary.validation_average_precision,
        validation_brier_score = summary.validation_brier_score,
        validation_log_loss = summary.validation_log_loss,
        "learned human ignition model activated"
    );
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn load_fire_history(config: Config, source: FireHistorySelection) -> anyhow::Result<()> {
    let (path, connector) = match source {
        FireHistorySelection::Bdiff => (
            config.bdiff_path.clone(),
            FireHistorySource::bdiff(&config.bdiff_path),
        ),
        FireHistorySelection::Promethee => (
            config.promethee_path.clone(),
            FireHistorySource::promethee(&config.promethee_path),
        ),
    };
    anyhow::ensure!(
        path.is_file(),
        "fire-history file does not exist: {}",
        path.display()
    );
    anyhow::ensure!(
        config.data_profile != DataProfile::Production || !is_fixture_path(&path),
        "production fire-history import cannot use fixture data: {}",
        path.display()
    );

    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let territory = configured_territory(&config, grid)?;
    let context = FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days: 1,
        end_date: Utc::now().date_naive(),
        firms_map_key: config.firms_map_key.clone(),
        meteofrance_api_key: config.meteofrance_api_key.clone(),
    };
    let observations = match connector.fetch(&context).await {
        Ok(observations) => observations,
        Err(error) => {
            store
                .record_source_error(connector.id(), &error.to_string())
                .await?;
            return Err(error).context("fire-history source failed");
        }
    };
    let rows_upserted = store
        .upsert_ignition_history(&observations)
        .await
        .context("failed to persist ignition history")?;
    store
        .record_source_success(connector.id(), observations.len())
        .await?;

    let history = store
        .historical_ignitions_until(Utc::now().date_naive())
        .await
        .context("failed to reload persisted ignition history")?;
    let history_cells = history
        .iter()
        .map(|ignition| ignition.cell)
        .collect::<Vec<_>>();
    let fallback_cells;
    let partitions = if let Some(territory) = &territory {
        territory
            .partitions
            .iter()
            .map(|partition| partition.cells.as_slice())
            .collect::<Vec<_>>()
    } else {
        fallback_cells = grid.cells_for_bbox(config.aoi_bbox)?;
        vec![fallback_cells.as_slice()]
    };
    let refresh =
        static_layers::refresh_history_features(&store, grid, &partitions, &history_cells).await?;

    tracing::info!(
        source = connector.id(),
        file = %path.display(),
        fetched = observations.len(),
        rows_upserted,
        total_ignitions = refresh.ignitions,
        cells = refresh.cells,
        feature_rows_updated = refresh.rows_updated,
        "fire-history load complete; refreshed features apply with the next complete forecast"
    );
    Ok(())
}

async fn import_bdiff(config: Config, path: &Path, dry_run: bool) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.is_file(),
        "BDIFF file does not exist: {}",
        path.display()
    );
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    if dry_run {
        let document = ingest::bdiff::read_file(path, grid)
            .await
            .context("failed to parse BDIFF file")?;
        let rejected = document
            .rows
            .iter()
            .filter(|row| !row.normalized.is_valid())
            .count();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "dry_run",
                "file": path.file_name().and_then(|name| name.to_str()).unwrap_or("bdiff.csv"),
                "received": document.rows.len(),
                "valid": document.rows.len() - rejected,
                "rejected": rejected,
                "h3_resolution": config.h3_resolution,
                "pipeline_version": "v1",
            }))?
        );
        return Ok(());
    }

    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let result = bdiff_pipeline::run(&store, path, grid).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "batch_id": result.ids.batch_id,
            "run_id": result.ids.pipeline_run_id,
            "received": result.persistence.received,
            "raw": result.persistence.raw_inserted,
            "staging_valid": result.persistence.staging_valid,
            "staging_rejected": result.persistence.staging_rejected,
            "fire_created": result.persistence.fire_created,
            "fire_already_present": result.persistence.fire_already_present,
            "technical_duplicates": result.persistence.technical_duplicates,
            "elapsed_seconds": result.elapsed_seconds,
            "status": result.status.as_str(),
        }))?
    );
    Ok(())
}

async fn osm_aggregate(config: &Config, output: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        output.extension().and_then(|value| value.to_str()) == Some("jsonl"),
        "OSM aggregate output must use a .jsonl extension"
    );
    anyhow::ensure!(
        config.osm_path != output,
        "OSM aggregate output must differ from OSM_PATH"
    );
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let context = FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days: 1,
        end_date: Utc::now().date_naive(),
        firms_map_key: config.firms_map_key.clone(),
        meteofrance_api_key: config.meteofrance_api_key.clone(),
    };
    let observations = OsmSource::new(&config.osm_path)
        .fetch(&context)
        .await
        .with_context(|| format!("failed to aggregate {}", config.osm_path.display()))?;
    write_aggregate_cache(output, &observations)
        .with_context(|| format!("failed to write {}", output.display()))?;
    let bytes = fs::metadata(output)
        .with_context(|| format!("failed to inspect {}", output.display()))?
        .len();
    tracing::info!(
        input = %config.osm_path.display(),
        output = %output.display(),
        cells = observations.len(),
        bytes,
        "OSM H3 aggregate cache complete"
    );
    Ok(())
}

fn territory_plan(config: &Config) -> anyhow::Result<()> {
    let path = config
        .territory_geojson_path
        .as_deref()
        .context("TERRITORY_GEOJSON_PATH is required for territory-plan")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let territory = territory::Territory::load(path, &config.territory_codes, grid)?;
    println!("Territory: {}", path.display());
    println!("H3 resolution: {}", config.h3_resolution);
    println!("\nCODE  DEPARTMENT                       CELLS");
    for partition in &territory.partitions {
        println!(
            "{:<5} {:<30} {:>10}",
            partition.code,
            partition.name,
            partition.cells.len()
        );
    }
    println!("\nDepartments: {}", territory.partitions.len());
    println!("Unique H3 cells: {}", territory.cell_count());
    println!(
        "Duplicate border cells removed: {}",
        territory.duplicate_cells_removed
    );
    Ok(())
}

async fn load_commune_boundary(
    config: Config,
    insee_code: &str,
    geojson: &Path,
    name: &str,
    postal_codes: &[String],
) -> anyhow::Result<()> {
    let fixture =
        commune_loader::CommuneBoundaryFixture::load(geojson, insee_code, name, postal_codes)?;
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    store
        .upsert_commune_boundary(
            &fixture.insee_code,
            &fixture.name,
            &fixture.postal_codes,
            &fixture.geometry,
        )
        .await
        .context("failed to persist commune boundary")?;
    println!(
        "Loaded commune boundary: {} ({}), postal codes: {}",
        fixture.name,
        fixture.insee_code,
        fixture.postal_codes.join(", ")
    );
    Ok(())
}

async fn load_commune_catalog(
    config: Config,
    geojson: &Path,
    source_version: &str,
) -> anyhow::Result<()> {
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let (entries, checksum) = commune_loader::load_catalog(geojson, grid)?;
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let resolution = i16::from(config.h3_resolution);
    let (boundaries, mappings) = store
        .replace_commune_catalog(&entries, resolution, source_version, &checksum)
        .await
        .context("failed to persist commune catalog")?;
    println!(
        "Loaded {boundaries} communes and {mappings} H3 mappings at resolution {resolution} \
         (checksum {checksum})"
    );
    Ok(())
}

async fn forecast(config: Config) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let model = operational_risk_model(&store, config.risk).await?;
    let territory = configured_territory(&config, grid)?;
    let result = if let Some(territory) = &territory {
        let regions = territory
            .partitions
            .iter()
            .map(|partition| forecast::ForecastRegion {
                code: &partition.code,
                bbox: partition.bbox,
                cells: &partition.cells,
            })
            .collect::<Vec<_>>();
        forecast::recompute_forecast_regions(
            &store,
            &model,
            grid,
            &regions,
            config.weather_idw_power,
            &config.weather_cache_dir,
            None,
        )
        .await
    } else {
        forecast::recompute_forecast(
            &store,
            &model,
            grid,
            config.aoi_bbox,
            config.weather_idw_power,
            &config.weather_cache_dir,
            None,
        )
        .await
    };
    match result {
        Ok(summary) => {
            for source_error in &summary.source_errors {
                store
                    .record_source_error(source_error.source_id, &source_error.message)
                    .await?;
            }
            store
                .record_source_success(summary.source_id, summary.anchors)
                .await?;
            tracing::info!(
                source = summary.source_id,
                computed_at = %summary.computed_at,
                base_valid_at = %summary.base_valid_at,
                anchors = summary.anchors,
                cells = summary.cells,
                scores_upserted = summary.scores_upserted,
                elapsed_seconds = summary.elapsed_seconds,
                "operational forecast complete"
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn backtest(
    config: Config,
    from: NaiveDate,
    to: NaiveDate,
    warmup_days: u16,
) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let cells = grid
        .cells_for_bbox(config.aoi_bbox)
        .context("failed to cover backtest AOI")?;
    let model = heuristic_risk_model(config.risk)?;
    let summary = backtest::run(
        &store,
        &model,
        backtest::BacktestOptions {
            grid,
            cells: &cells,
            from,
            to,
            weather_path: &config.backtest_weather_path,
            weather_idw_power: config.weather_idw_power,
            warmup_days,
        },
    )
    .await?;
    tracing::info!(
        from = %from,
        to = %to,
        days = summary.days,
        warmup_days = summary.warmup_days,
        cells_per_day = summary.cells_per_day,
        ignitions = summary.ignitions,
        approximate_auc = summary.approximate_auc,
        top_five_hits = summary.top_five_hits,
        top_ten_hits = summary.top_ten_hits,
        elapsed_seconds = summary.elapsed_seconds,
        output = "out/backtest_report.md",
        "historical backtest complete"
    );
    Ok(())
}

async fn load_static(config: Config) -> anyhow::Result<()> {
    config
        .validate_static_data()
        .context("static data readiness check failed")?;
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let territory = configured_territory(&config, grid)?;
    let context = FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days: 1,
        end_date: Utc::now().date_naive(),
        firms_map_key: config.firms_map_key,
        meteofrance_api_key: config.meteofrance_api_key,
    };
    let paths = static_layers::StaticPaths {
        osm: config.osm_path,
        bdiff: config.bdiff_path,
        promethee: config.promethee_path,
        corine: config.corine_path,
        insee: config.insee_path,
        calendar: config.calendar_path,
    };
    let summary = if let Some(territory) = &territory {
        let partitions = territory
            .partitions
            .iter()
            .map(|partition| partition.cells.as_slice())
            .collect::<Vec<_>>();
        static_layers::load_static_partitions(
            &store,
            &context,
            paths,
            config.data_profile == DataProfile::Production,
            &partitions,
        )
        .await?
    } else {
        static_layers::load_static(
            &store,
            &context,
            paths,
            config.data_profile == DataProfile::Production,
        )
        .await?
    };
    tracing::info!(
        cells = summary.cells,
        osm_records = summary.osm_records,
        corine_samples = summary.corine_samples,
        population_samples = summary.population_samples,
        historical_ignitions = summary.historical_ignitions,
        calendar_days = summary.calendar_days,
        rows_upserted = summary.cell_rows_upserted,
        "static layer load complete"
    );
    let model = operational_risk_model(&store, config.risk).await?;
    if let Some(territory) = &territory {
        for partition in &territory.partitions {
            let _summary =
                risk_pipeline::recompute_latest_risk(&store, &model, &partition.cells, None)
                    .await?;
        }
    } else {
        let cells = context.grid.cells_for_bbox(context.aoi)?;
        let _summary = risk_pipeline::recompute_latest_risk(&store, &model, &cells, None).await?;
    }
    Ok(())
}

async fn inspect_dataset(config: Config, dataset_version_id: &str) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let summary = store
        .dataset_version_summary(dataset_version_id)
        .await
        .context("failed to load dataset version summary")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "logical_id": summary.logical_id,
            "variant": summary.variant,
            "status": summary.status,
            "row_count": summary.row_count,
            "positive_count": summary.positive_count,
            "negative_count": summary.negative_count,
            "exclusion_count": summary.exclusion_count,
            "checksum": summary.checksum,
            "created_at": summary.created_at,
            "finalized_at": summary.finalized_at,
        }))?
    );
    Ok(())
}

async fn data_status(config: Config) -> anyhow::Result<()> {
    println!("Data profile: {}", config.data_profile);
    println!("\nStatic input files:");
    println!("{:<11} {:<10} {:>10}  PATH", "SOURCE", "STATUS", "SIZE");
    for source in config.static_data_paths() {
        let metadata = fs::metadata(source.path).ok().filter(fs::Metadata::is_file);
        let status = if metadata.is_none() {
            "missing"
        } else if is_fixture_path(source.path) {
            "fixture"
        } else {
            "ready"
        };
        let size = metadata.map_or_else(|| "-".to_owned(), |value| human_bytes(value.len()));
        println!(
            "{:<11} {:<10} {:>10}  {}",
            source.source,
            status,
            size,
            source.path.display()
        );
    }
    if matches!(
        config
            .corine_path
            .extension()
            .and_then(|value| value.to_str()),
        Some("tif" | "tiff")
    ) {
        let gdal_ready = ProcessCommand::new("gdal_translate")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        println!(
            "\nCORINE raster tool: {}",
            if gdal_ready {
                "gdal_translate ready"
            } else {
                "gdal_translate missing"
            }
        );
    }

    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let territory = configured_territory(&config, grid)?;
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let (coverage, cell_count, area_label) = if let Some(territory) = &territory {
        let mut coverage = StaticFeatureCoverage::default();
        for partition in &territory.partitions {
            let partition_coverage = store
                .static_feature_coverage(&partition.cells)
                .await
                .with_context(|| format!("failed to inspect department {}", partition.code))?;
            add_coverage(&mut coverage, partition_coverage);
        }
        (coverage, territory.cell_count(), "territory")
    } else {
        let cells = grid
            .cells_for_bbox(config.aoi_bbox)
            .context("failed to cover data-status AOI")?;
        let coverage = store
            .static_feature_coverage(&cells)
            .await
            .context("failed to inspect static feature coverage")?;
        (coverage, cells.len(), "AOI")
    };
    println!("\nPostgreSQL feature coverage ({cell_count} {area_label} cells):");
    print_coverage("static rows", coverage.static_rows, cell_count);
    print_coverage("road", coverage.road_cells, cell_count);
    print_coverage("combustible", coverage.combustible_cells, cell_count);
    print_coverage("population", coverage.population_cells, cell_count);
    print_coverage("history", coverage.history_cells, cell_count);
    print_coverage("WUI", coverage.wui_cells, cell_count);
    print_coverage("agriculture", coverage.agriculture_cells, cell_count);

    config
        .validate_static_data()
        .context("static data readiness check failed")?;
    Ok(())
}

fn add_coverage(total: &mut StaticFeatureCoverage, value: StaticFeatureCoverage) {
    total.static_rows += value.static_rows;
    total.road_cells += value.road_cells;
    total.combustible_cells += value.combustible_cells;
    total.population_cells += value.population_cells;
    total.history_cells += value.history_cells;
    total.wui_cells += value.wui_cells;
    total.agriculture_cells += value.agriculture_cells;
}

fn print_coverage(label: &str, covered: i64, total: usize) {
    #[allow(clippy::cast_precision_loss)]
    let percentage = if total == 0 {
        0.0
    } else {
        100.0 * covered as f64 / total as f64
    };
    println!("  {label:<12} {covered:>7}/{total:<7} {percentage:>6.2}%");
}

#[allow(clippy::cast_precision_loss)]
fn human_bytes(bytes: u64) -> String {
    const KIB: u64 = 1_024;
    const MIB: u64 = KIB * 1_024;
    const GIB: u64 = MIB * 1_024;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

async fn backfill(config: Config, source: BackfillSource, days: u16) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let territory = configured_territory(&config, grid)?;
    let context = FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days,
        end_date: Utc::now().date_naive(),
        firms_map_key: config.firms_map_key.clone(),
        meteofrance_api_key: config.meteofrance_api_key.clone(),
    };

    match source {
        BackfillSource::Firms => {
            let connector = FirmsSource::new(config.firms_fixture_path);
            let result = firms_pipeline::run(
                &store,
                &connector,
                &context,
                firms_pipeline::FirmsTrigger::Backfill,
            )
            .await
            .context("FIRMS pipeline failed")?;
            export::write_firms_geojson(&result.observations, Path::new(FIRMS_GEOJSON_PATH))
                .await?;
            tracing::info!(
                import_batch_id = %result.ids.batch_id,
                pipeline_run_id = %result.ids.pipeline_run_id,
                fetched = result.persistence.received,
                inserted = result.persistence.public_inserted,
                status = result.status.as_str(),
                elapsed_seconds = result.elapsed_seconds,
                output = FIRMS_GEOJSON_PATH,
                "FIRMS backfill complete"
            );
        }
    }
    let model = operational_risk_model(&store, config.risk).await?;
    if let Some(territory) = &territory {
        for partition in &territory.partitions {
            let _summary =
                risk_pipeline::recompute_latest_risk(&store, &model, &partition.cells, None)
                    .await?;
        }
    } else {
        let cells = context.grid.cells_for_bbox(context.aoi)?;
        let _summary = risk_pipeline::recompute_latest_risk(&store, &model, &cells, None).await?;
    }
    Ok(())
}

async fn recompute(config: Config, date: NaiveDate) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let context = FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days: 1,
        end_date: date,
        firms_map_key: config.firms_map_key,
        meteofrance_api_key: config.meteofrance_api_key,
    };
    let connector = MeteoFranceSource::new(config.meteofrance_fixture_path);
    let summary =
        match weather::recompute_weather(&store, &connector, &context, config.weather_idw_power)
            .await
        {
            Ok(summary) => summary,
            Err(error) => {
                store
                    .record_source_error(connector.id(), &error.to_string())
                    .await?;
                return Err(error);
            }
        };
    store
        .record_source_success(connector.id(), summary.station_count)
        .await?;
    tracing::info!(
        date = %summary.date,
        stations = summary.station_count,
        cells = summary.cell_count,
        observations_inserted = summary.observations_inserted,
        states_upserted = summary.states_upserted,
        "weather FWI recompute complete"
    );
    let model = operational_risk_model(&store, config.risk).await?;
    let cells = context.grid.cells_for_bbox(context.aoi)?;
    let risk_summary = risk_pipeline::recompute_risk(&store, &model, date, &cells, None).await?;
    tracing::info!(
        input_date = %risk_summary.input_date,
        computed_at = %risk_summary.computed_at,
        cells = risk_summary.cells,
        rows_upserted = risk_summary.rows_upserted,
        elapsed_ms = risk_summary.elapsed_ms,
        "weather-triggered risk recalculation complete"
    );
    Ok(())
}

async fn run(config: Config) -> anyhow::Result<()> {
    let store = Store::connect(&config.database_url)
        .await
        .context("failed to initialize database")?;
    let listener = TcpListener::bind(config.api_bind)
        .await
        .with_context(|| format!("failed to bind API to {}", config.api_bind))?;
    tracing::info!(bind = %config.api_bind, "API listening");
    let grid = H3Grid::new(config.h3_resolution).context("failed to configure H3 grid")?;
    let model = operational_risk_model(&store, config.risk).await?;
    let territory = configured_territory(&config, grid)?.map(Arc::new);
    let (updates, _) = broadcast::channel::<Arc<api::RiskUpdate>>(RISK_UPDATE_CHANNEL_CAPACITY);
    scheduler::spawn(
        config.clone(),
        store.clone(),
        grid,
        model,
        territory.clone(),
        updates.clone(),
    );

    let territory_label = config.territory_label.clone().unwrap_or_else(|| {
        if config.territory_geojson_path.is_some() {
            "France métropolitaine".to_owned()
        } else {
            "Aude · Occitanie".to_owned()
        }
    });
    let mut app_state =
        AppState::new(store, grid, updates).with_operational_area(config.aoi_bbox, territory_label);
    if let Some(territory) = &territory {
        let cells = territory
            .partitions
            .iter()
            .flat_map(|partition| partition.cells.iter().copied())
            .collect();
        app_state = app_state.with_operational_cells(cells);
    }
    axum::serve(listener, api::router(app_state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("API server failed")
}

fn configured_territory(
    config: &Config,
    grid: H3Grid,
) -> anyhow::Result<Option<territory::Territory>> {
    config
        .territory_geojson_path
        .as_deref()
        .map(|path| territory::Territory::load(path, &config.territory_codes, grid))
        .transpose()
}

fn heuristic_risk_model(config: config::RiskConfig) -> anyhow::Result<HeuristicV1> {
    HeuristicV1::new(config.heuristic()).context("failed to configure risk model")
}

async fn operational_risk_model(
    store: &Store,
    config: config::RiskConfig,
) -> anyhow::Result<HeuristicV1> {
    let model = heuristic_risk_model(config)?;
    let Some(version) = store
        .active_human_model()
        .await
        .context("failed to load active human model")?
    else {
        tracing::warn!("no learned human model is active; using heuristic fallback");
        return Ok(model);
    };
    let artifact = serde_json::from_value::<LearnedHumanModel>(version.artifact)
        .context("active human model artifact is invalid")?;
    tracing::info!(
        model_version = version.id,
        trained_at = %version.trained_at,
        metrics = %version.metrics,
        "learned human ignition model loaded"
    );
    model
        .with_learned_human(artifact)
        .context("active human model parameters are invalid")
}

async fn shutdown_signal() {
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("shutdown signal received"),
        Err(error) => tracing::error!(%error, "failed to listen for shutdown signal"),
    }
}

fn init_tracing() {
    TRACING_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        if cfg!(debug_assertions) {
            tracing_subscriber::fmt()
                .with_env_filter(filter.clone())
                .pretty()
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
    });
}
