use std::{sync::Arc, time::Instant};

use anyhow::Context as _;
use chrono::{DateTime, NaiveDate, Utc};
use grid::CellIndex;
use risk::{CellFeatures, IgnitionModel, RiskScore};
use serde::Deserialize;
use store::Store;
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug)]
pub struct RiskRecomputeSummary {
    pub input_date: NaiveDate,
    pub computed_at: DateTime<Utc>,
    pub cells: usize,
    pub rows_upserted: u64,
    pub elapsed_ms: u128,
}

pub async fn recompute_risk(
    store: &Store,
    model: &impl IgnitionModel,
    input_date: NaiveDate,
    cells: &[CellIndex],
    updates: Option<&broadcast::Sender<Arc<api::RiskUpdate>>>,
) -> anyhow::Result<RiskRecomputeSummary> {
    let started = Instant::now();
    let inputs = store
        .risk_inputs(input_date, cells)
        .await
        .context("failed to load risk inputs")?;
    let computed_at = Utc::now();
    let scores = inputs
        .into_iter()
        .map(|input| {
            let static_features = serde_json::from_value::<StaticFeatures>(input.features.clone())
                .context("invalid cell_static feature document")?;
            Ok(model.score(
                input.cell,
                &CellFeatures {
                    fwi: input.fwi,
                    hist: static_features.hist,
                    wui: static_features.wui,
                    road: static_features.road,
                    agri: static_features.agri,
                    population: static_features.population,
                    poi: static_features.poi,
                    power_line: static_features.power_line,
                    combustible: static_features.combustible,
                    date: input_date,
                    school_holiday: input.school_holiday,
                    public_holiday: input.public_holiday,
                },
                computed_at,
            ))
        })
        .collect::<anyhow::Result<Vec<RiskScore>>>()?;
    let rows_upserted = store
        .upsert_risk_scores(input_date, &scores)
        .await
        .context("failed to persist risk scores")?;
    if let Some(updates) = updates {
        let _receivers = updates.send(Arc::new(api::RiskUpdate::from_scores(&scores)));
    }
    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        %input_date,
        %computed_at,
        cells = scores.len(),
        rows_upserted,
        elapsed_ms,
        "risk recalculation complete"
    );
    Ok(RiskRecomputeSummary {
        input_date,
        computed_at,
        cells: scores.len(),
        rows_upserted,
        elapsed_ms,
    })
}

pub async fn recompute_latest_risk(
    store: &Store,
    model: &impl IgnitionModel,
    cells: &[CellIndex],
    updates: Option<&broadcast::Sender<Arc<api::RiskUpdate>>>,
) -> anyhow::Result<Option<RiskRecomputeSummary>> {
    let Some(date) = store
        .latest_fwi_date()
        .await
        .context("failed to find latest FWI date")?
    else {
        tracing::warn!("risk recalculation skipped because no FWI state exists");
        return Ok(None);
    };
    recompute_risk(store, model, date, cells, updates)
        .await
        .map(Some)
}

#[derive(Deserialize)]
struct StaticFeatures {
    hist: f32,
    wui: f32,
    road: f32,
    agri: f32,
    #[serde(default)]
    population: f32,
    #[serde(default)]
    poi: f32,
    #[serde(default)]
    power_line: f32,
    combustible: bool,
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use api::AppState;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use chrono::NaiveDate;
    use grid::{BoundingBox, CellIndex, H3Grid};
    use ingest::{FetchCtx, meteo_france::MeteoFranceSource};
    use risk::{HeuristicConfig, HeuristicV1, Horizon};
    use store::Store;
    use tokio::sync::broadcast;
    use tower::ServiceExt as _;

    use crate::{
        static_layers::{StaticPaths, load_static},
        weather::recompute_weather,
    };

    use super::recompute_risk;

    #[tokio::test]
    async fn fixture_pipeline_serves_explainable_geojson_under_ten_seconds() {
        dotenvy::dotenv().ok();
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping database integration test: DATABASE_URL is not configured");
            return;
        };
        let store = Store::connect(&database_url)
            .await
            .expect("database should accept migrations");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let grid = H3Grid::new(9).expect("valid grid");
        let aoi = BoundingBox::new(2.34, 43.18, 2.40, 43.22).expect("valid AOI");
        let date = NaiveDate::from_ymd_opt(2025, 7, 16).expect("valid date");
        let context = FetchCtx {
            client: reqwest::Client::new(),
            aoi,
            grid,
            days: 1,
            end_date: date,
            firms_map_key: None,
            meteofrance_api_key: None,
        };
        load_static(
            &store,
            &context,
            StaticPaths {
                osm: root.join("testdata/osm_features.csv"),
                bdiff: root.join("testdata/bdiff_aude.csv"),
                promethee: root.join("testdata/promethee_aude.csv"),
                corine: root.join("testdata/corine_aude.csv"),
                insee: root.join("testdata/insee_filosofi_200m.csv"),
                calendar: root.join("testdata/calendar_zone_c.csv"),
            },
            false,
        )
        .await
        .expect("static fixture load");
        recompute_weather(
            &store,
            &MeteoFranceSource::new(root.join("testdata/meteo_france_synop.csv")),
            &context,
            2.0,
        )
        .await
        .expect("weather fixture recompute");
        let model = HeuristicV1::new(HeuristicConfig {
            fwi_max: 30.0,
            alpha: 0.6,
            beta: 0.4,
            w_hist: 0.4,
            w_wui: 0.25,
            w_road: 0.2,
            w_agri: 0.15,
        })
        .expect("valid model");
        let (updates, _) = broadcast::channel::<Arc<api::RiskUpdate>>(2);
        let cells = grid.cells_for_bbox(aoi).expect("valid cells");
        let summary = recompute_risk(&store, &model, date, &cells, Some(&updates))
            .await
            .expect("risk recompute");
        let scores = store
            .latest_risk_scores(&cells, 0.0, Horizon::Nowcast)
            .await
            .expect("latest scores");

        assert_eq!(summary.cells, cells.len());
        assert_eq!(scores.len(), cells.len());
        assert!(summary.elapsed_ms < Duration::from_secs(10).as_millis());
        assert!(scores.iter().any(|score| !score.top_factors.is_empty()));
        assert!(scores.iter().all(|score| {
            score
                .top_factors
                .windows(2)
                .all(|pair| pair[0].contribution >= pair[1].contribution)
        }));

        let detail_cell = scores
            .iter()
            .find(|score| !score.top_factors.is_empty())
            .expect("one explainable score")
            .cell;
        let app = api::router(
            AppState::new(store, grid, updates),
            "testdata/no-such-web-assets-dir",
        );
        assert_complete_api(&app, cells.len(), detail_cell).await;
    }

    async fn assert_complete_api(app: &Router, cell_count: usize, detail_cell: CellIndex) {
        let payload = get_json(
            app,
            "/risk?bbox=2.34,43.18,2.40,43.22&min_score=0&at=latest",
        )
        .await;
        assert_eq!(payload["type"], "FeatureCollection");
        assert_eq!(
            payload["features"].as_array().map(Vec::len),
            Some(cell_count)
        );
        let detail_payload = get_json(app, &format!("/risk/cell/{detail_cell}")).await;
        assert!(
            detail_payload["current"]["top_factors"]
                .as_array()
                .is_some_and(|factors| !factors.is_empty())
        );
        assert!(detail_payload["fwi"]["fwi"].is_number());
        let alerts_payload = get_json(app, "/alerts?threshold=0").await;
        assert_eq!(alerts_payload.as_array().map(Vec::len), Some(cell_count));
        let sources_payload = get_json(app, "/sources").await;
        assert!(
            sources_payload
                .as_array()
                .is_some_and(|sources| sources.len() >= 6)
        );
    }

    async fn get_json(app: &Router, uri: &str) -> serde_json::Value {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("API request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("readable response");
        serde_json::from_slice(&body).expect("valid JSON response")
    }
}
