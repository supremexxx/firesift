use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use grid::H3Grid;
use ingest::{
    Cadence, FetchCtx, Source,
    ecmwf_open::EcmwfOpenDataForecastSource,
    firms::FirmsSource,
    open_meteo::{ForecastModel, OpenMeteoForecastSource},
};
use risk::HeuristicV1;
use store::{BlueForecastContext, FreshnessThresholds, Store, SystemSnapshotContext};
use tokio::{
    sync::broadcast,
    time::{MissedTickBehavior, interval},
};

use crate::{
    blue_evidence::BlueEvidenceReviewer, config::Config, firms_pipeline, forecast,
    territory::Territory,
};

const FORECAST_POLL_INTERVAL: Duration = Duration::from_hours(1);
const DAY: Duration = Duration::from_hours(24);
/// 02:15 UTC: chosen to fall well outside `poll_forecast`'s on-the-hour
/// runs, the FIRMS poll window, and the documented VPS backup timers
/// (`deploy/oracle/systemd/pyrorisk-*.timer`), per
/// `PHASE4A5_SNAPSHOT_ARCHITECTURE_DECISION.md` §12 of the phase spec.
const DAILY_SNAPSHOT_HOUR_UTC: u32 = 2;
const DAILY_SNAPSHOT_MINUTE_UTC: u32 = 15;
/// Matches `snapshot_pipeline::ENVIRONMENT`: this pilot runs a single
/// environment bucket. A multi-environment deployment would need this
/// sourced from configuration instead of duplicated as a constant.
const SNAPSHOT_ENVIRONMENT: &str = "default";

pub fn spawn(
    config: Config,
    store: Store,
    grid: H3Grid,
    model: HeuristicV1,
    territory: Option<Arc<Territory>>,
    updates: broadcast::Sender<Arc<api::RiskUpdate>>,
) {
    tokio::spawn(poll_firms(config.clone(), store.clone(), grid));
    tokio::spawn(poll_blue_evidence(config.clone(), store.clone()));
    tokio::spawn(poll_blue_ground_truth(store.clone()));
    tokio::spawn(poll_forecast(
        config,
        store.clone(),
        grid,
        model,
        territory,
        updates,
    ));
    tokio::spawn(snapshot_operational_hourly(store.clone()));
    tokio::spawn(snapshot_operational_daily(store));
    // Scientific FWI history is archived directly after the first complete
    // nowcast of each UTC day. The legacy weekly full-row pilot remains
    // available through controlled tooling but is no longer scheduled.
}

async fn poll_blue_ground_truth(store: Store) {
    let mut ticker = interval(FORECAST_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        match store.refresh_blue_ground_truth().await {
            Ok(refresh) => {
                let reactive_cases = match store.latest_blue_bulletin().await {
                    Ok(Some(bulletin)) => store
                        .ensure_blue_reactive_evidence_cases(&bulletin.id)
                        .await
                        .unwrap_or_else(|error| {
                            tracing::error!(%error, bulletin_id = %bulletin.id, "BLUE reactive evidence selection failed safely");
                            0
                        }),
                    Ok(None) => 0,
                    Err(error) => {
                        tracing::error!(%error, "latest BLUE bulletin unavailable for reactive evidence");
                        0
                    }
                };
                tracing::info!(
                    satellite_windows = refresh.satellite_windows_upserted,
                    confirmed_ignitions = refresh.confirmed_ignitions_upserted,
                    comparisons = refresh.comparisons_inserted,
                    reactive_cases,
                    "BLUE Ground Truth refresh complete"
                );
            }
            Err(error) => tracing::error!(%error, "BLUE Ground Truth refresh failed safely"),
        }
    }
}

async fn poll_firms(config: Config, store: Store, grid: H3Grid) {
    let source = FirmsSource::new(config.firms_fixture_path.clone());
    let mut ticker = source_ticker(&source);
    loop {
        ticker.tick().await;
        let context = fetch_context(&config, grid, Utc::now().date_naive());
        match firms_pipeline::run(
            &store,
            &source,
            &context,
            firms_pipeline::FirmsTrigger::Scheduler,
        )
        .await
        {
            Ok(result) => {
                tracing::info!(
                    import_batch_id = %result.ids.batch_id,
                    pipeline_run_id = %result.ids.pipeline_run_id,
                    fetched = result.persistence.received,
                    inserted = result.persistence.public_inserted,
                    "scheduled FIRMS poll complete"
                );
            }
            Err(error) => {
                tracing::error!(source = source.id(), %error, "scheduled source poll failed; continuing");
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn poll_forecast(
    config: Config,
    store: Store,
    grid: H3Grid,
    model: HeuristicV1,
    territory: Option<Arc<Territory>>,
    updates: broadcast::Sender<Arc<api::RiskUpdate>>,
) {
    let mut ticker = interval(FORECAST_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let forecast_sources = [
        EcmwfOpenDataForecastSource::ID,
        OpenMeteoForecastSource::new(ForecastModel::MeteoFrance).id(),
        OpenMeteoForecastSource::new(ForecastModel::Ecmwf).id(),
    ];
    let forecast_is_fresh = store.source_statuses().await.is_ok_and(|statuses| {
        statuses.into_iter().any(|status| {
            forecast_sources.contains(&status.id.as_str())
                && status.last_success.is_some_and(|last_success| {
                    let age = Utc::now().signed_duration_since(last_success);
                    age >= ChronoDuration::zero() && age < ChronoDuration::minutes(55)
                })
        })
    });
    if forecast_is_fresh {
        ticker.tick().await;
        tracing::info!("recent operational weather forecast retained after restart");
    }
    loop {
        ticker.tick().await;
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
                Some(&updates),
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
                Some(&updates),
            )
            .await
        };
        match result {
            Ok(summary) => {
                for source_error in &summary.source_errors {
                    record_error(&store, source_error.source_id, &source_error.message).await;
                }
                record_success(&store, summary.source_id, summary.anchors).await;
                // A source further down the fallback chain that this cycle
                // never had to try (because something earlier in the chain
                // already worked) must not keep displaying whatever error
                // it last recorded, possibly days ago -- that reads as a
                // live failure when it is actually just unused right now.
                let attempted: Vec<&str> = std::iter::once(summary.source_id)
                    .chain(summary.source_errors.iter().map(|error| error.source_id))
                    .collect();
                for source_id in forecast_sources {
                    if !attempted.contains(&source_id) {
                        clear_stale_error(&store, source_id).await;
                    }
                }
                match store
                    .capture_daily_dense_scientific_snapshot(
                        summary.computed_at,
                        summary.base_valid_at,
                        summary.source_id,
                    )
                    .await
                {
                    Ok(snapshot) => tracing::info!(
                        snapshot_id = %snapshot.id,
                        valid_at = %snapshot.valid_at,
                        cells = snapshot.cell_count_present,
                        "daily dense scientific archive available"
                    ),
                    Err(error) => tracing::error!(
                        %error,
                        computed_at = %summary.computed_at,
                        "daily dense scientific archive failed; operational forecast remains published"
                    ),
                }
                match crate::shadow_scoring::record_forecast_batch(
                    &store,
                    &config,
                    summary.computed_at,
                )
                .await
                {
                    Ok(0) => {}
                    Ok(rows) => {
                        tracing::info!(rows, "P3 shadow scores recorded; v1 serving unchanged");
                    }
                    Err(error) => {
                        tracing::error!(%error, "P3 shadow scoring failed safely after v1 forecast");
                    }
                }
                if config.blue_center_enabled {
                    let context = BlueForecastContext {
                        environment: std::env::var("ERYTHEON_ENVIRONMENT")
                            .unwrap_or_else(|_| "production".to_owned()),
                        application_revision: std::env::var("ERYTHEON_GIT_REVISION")
                            .unwrap_or_default(),
                        application_image: std::env::var("ERYTHEON_IMAGE_REFERENCE")
                            .unwrap_or_default(),
                        application_image_digest: std::env::var("ERYTHEON_IMAGE_DIGEST")
                            .unwrap_or_default(),
                    };
                    match store
                        .capture_blue_daily_bulletin(
                            summary.computed_at,
                            summary.source_id,
                            &context,
                        )
                        .await
                    {
                        Ok(Some(bulletin)) => {
                            match store.ensure_blue_evidence_cases(&bulletin.id, 20).await {
                                Ok(selected) => tracing::info!(
                                    bulletin_id = %bulletin.id,
                                    bulletin_date = %bulletin.bulletin_date,
                                    alerts_24h = bulletin.alerts_24h,
                                    alerts_48h = bulletin.alerts_48h,
                                    newly_selected_cases = selected,
                                    "BLUE daily forecast bulletin and diversified evidence selection available"
                                ),
                                Err(error) => tracing::error!(
                                    %error,
                                    bulletin_id = %bulletin.id,
                                    "BLUE bulletin published but evidence selection failed"
                                ),
                            }
                        }
                        Ok(None) => tracing::debug!("BLUE daily issue slot not reached"),
                        Err(error) => tracing::error!(
                            %error,
                            "BLUE bulletin failed; operational forecast remains published"
                        ),
                    }
                }
                tracing::info!(
                    source = summary.source_id,
                    computed_at = %summary.computed_at,
                    base_valid_at = %summary.base_valid_at,
                    anchors = summary.anchors,
                    cells = summary.cells,
                    elapsed_seconds = summary.elapsed_seconds,
                    "scheduled weather forecast complete"
                );
            }
            Err(error) => {
                tracing::error!(%error, "scheduled weather forecast failed; continuing");
                for source_id in forecast_sources {
                    record_error(&store, source_id, &error.to_string()).await;
                }
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn poll_blue_evidence(config: Config, store: Store) {
    if !config.blue_center_enabled {
        return;
    }
    let reviewer = if config.blue_ai_evidence_enabled {
        if let Some(api_key) = config.openai_api_key {
            match BlueEvidenceReviewer::new(
                api_key,
                config.blue_openai_model,
                config.blue_feux_de_foret_enabled,
            ) {
                Ok(reviewer) => Some(reviewer),
                Err(error) => {
                    tracing::error!(%error, "BLUE automatic evidence reviewer could not start");
                    None
                }
            }
        } else {
            tracing::warn!(
                "BLUE automatic evidence review is enabled but OPENAI_API_KEY is absent"
            );
            None
        }
    } else {
        tracing::info!("BLUE diversified selection enabled; automatic evidence review is disabled");
        None
    };
    let mut ticker = interval(FORECAST_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let bulletin = match store.latest_blue_bulletin().await {
            Ok(Some(bulletin)) => bulletin,
            Ok(None) => continue,
            Err(error) => {
                tracing::error!(%error, "failed to read latest BLUE bulletin for evidence selection");
                continue;
            }
        };
        if let Err(error) = store.ensure_blue_evidence_cases(&bulletin.id, 20).await {
            tracing::error!(%error, bulletin_id = %bulletin.id, "BLUE diversified selection failed");
            continue;
        }
        let Some(reviewer) = &reviewer else {
            continue;
        };
        for _ in 0..20 {
            let claim = match store.claim_blue_evidence_case().await {
                Ok(Some(claim)) => claim,
                Ok(None) => break,
                Err(error) => {
                    tracing::error!(%error, "failed to claim BLUE evidence case");
                    break;
                }
            };
            let checksum = reviewer.request_checksum(&claim);
            let run_id = match store
                .start_blue_evidence_run(
                    &claim.id,
                    claim.attempt_count,
                    &claim.review_horizon,
                    claim.stage_attempt_count,
                    &checksum,
                    reviewer.model(),
                )
                .await
            {
                Ok(id) => id,
                Err(error) => {
                    tracing::error!(%error, case_id = %claim.id, "failed to start BLUE evidence run");
                    break;
                }
            };
            match reviewer.review(&claim).await {
                Ok(result) => {
                    if let Err(error) = store
                        .complete_blue_evidence_run(
                            &claim.id,
                            &run_id,
                            &claim.review_horizon,
                            reviewer.model(),
                            &result,
                        )
                        .await
                    {
                        tracing::error!(%error, case_id = %claim.id, "failed to persist BLUE evidence result");
                    } else {
                        tracing::info!(
                            case_id = %claim.id,
                            commune = %claim.commune_name,
                            verdict = %result.verdict,
                            horizon = %claim.review_horizon,
                            sources = result.sources.len(),
                            "BLUE automatic evidence review complete"
                        );
                    }
                }
                Err(error) => {
                    let safe_error = error.to_string();
                    if error.is_quota_exhausted() {
                        if let Err(store_error) = store
                            .defer_blue_evidence_run_for_quota(&claim.id, &run_id, &safe_error)
                            .await
                        {
                            tracing::error!(%store_error, case_id = %claim.id, "failed to defer BLUE evidence review after provider-credit exhaustion");
                        }
                        tracing::warn!(
                            case_id = %claim.id,
                            commune = %claim.commune_name,
                            "BLUE evidence review deferred: provider credit is exhausted"
                        );
                        break;
                    }
                    if let Err(store_error) = store
                        .fail_blue_evidence_run(
                            &claim.id,
                            &run_id,
                            &claim.review_horizon,
                            &safe_error,
                        )
                        .await
                    {
                        tracing::error!(%store_error, case_id = %claim.id, "failed to persist BLUE evidence failure");
                    }
                    tracing::warn!(%error, case_id = %claim.id, "BLUE evidence review failed safely");
                }
            }
        }
    }
}

fn source_ticker(source: &impl Source) -> tokio::time::Interval {
    let Cadence::Poll(cadence) = source.cadence() else {
        unreachable!("scheduler only accepts pollable sources");
    };
    let mut ticker = interval(cadence);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ticker
}

fn fetch_context(config: &Config, grid: H3Grid, end_date: chrono::NaiveDate) -> FetchCtx {
    FetchCtx {
        client: reqwest::Client::new(),
        aoi: config.aoi_bbox,
        grid,
        days: 1,
        end_date,
        firms_map_key: config.firms_map_key.clone(),
        meteofrance_api_key: config.meteofrance_api_key.clone(),
    }
}

async fn record_success(store: &Store, source: &str, count: usize) {
    if let Err(error) = store.record_source_success(source, count).await {
        tracing::error!(source, %error, "failed to record source success");
    }
}

async fn record_error(store: &Store, source: &str, message: &str) {
    if let Err(error) = store.record_source_error(source, message).await {
        tracing::error!(source, %error, "failed to record source error");
    }
}

async fn clear_stale_error(store: &Store, source: &str) {
    if let Err(error) = store.clear_stale_source_error(source).await {
        tracing::error!(source, %error, "failed to clear stale source error");
    }
}

/// Duration until the next occurrence of `hour:minute` UTC (today if
/// still ahead, otherwise tomorrow). A snapshot outage never blocks the
/// main service: any failure here is logged and the loop simply waits
/// for its next tick.
fn duration_until_next_utc_time(hour: u32, minute: u32) -> Duration {
    let now = Utc::now();
    let today_target = now
        .date_naive()
        .and_hms_opt(hour, minute, 0)
        .expect("valid hour/minute constants");
    let target = if now.naive_utc() < today_target {
        today_target
    } else {
        today_target + ChronoDuration::days(1)
    };
    (target - now.naive_utc())
        .to_std()
        .unwrap_or(Duration::from_secs(0))
}

async fn capture_operational_snapshot(store: &Store, cadence: &str) {
    let ctx = SystemSnapshotContext {
        application_revision: std::env::var("ERYTHEON_GIT_REVISION")
            .or_else(|_| std::env::var("ERYTHEON_APPLICATION_REVISION"))
            .ok(),
        application_image: std::env::var("ERYTHEON_IMAGE_REFERENCE")
            .or_else(|_| std::env::var("ERYTHEON_APPLICATION_IMAGE"))
            .ok(),
        application_image_digest: std::env::var("ERYTHEON_IMAGE_DIGEST").ok(),
        application_restart_count: None,
        caddy_state: std::env::var("ERYTHEON_CADDY_STATE").ok(),
        trigger_kind: Some("scheduler".to_owned()),
    };
    match store
        .capture_system_snapshot(SNAPSHOT_ENVIRONMENT, cadence, Utc::now(), &ctx)
        .await
    {
        Ok(snapshot) => {
            tracing::info!(
                cadence,
                snapshot_id = snapshot.id,
                checksum = %snapshot.checksum,
                "operational snapshot captured"
            );
            match store
                .evaluate_and_record_alerts(&snapshot, FreshnessThresholds::default())
                .await
            {
                Ok(alerts) if !alerts.is_empty() => {
                    tracing::warn!(count = alerts.len(), "new observability alerts recorded");
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error, "failed to evaluate observability alert rules");
                }
            }
        }
        Err(error) => {
            tracing::error!(cadence, %error, "operational snapshot capture failed; continuing");
        }
    }
}

async fn snapshot_operational_hourly(store: Store) {
    let mut ticker = interval(Duration::from_hours(1));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        capture_operational_snapshot(&store, "hourly").await;
    }
}

async fn snapshot_operational_daily(store: Store) {
    tokio::time::sleep(duration_until_next_utc_time(
        DAILY_SNAPSHOT_HOUR_UTC,
        DAILY_SNAPSHOT_MINUTE_UTC,
    ))
    .await;
    let mut ticker = interval(DAY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        capture_operational_snapshot(&store, "daily").await;
        ticker.tick().await;
    }
}
