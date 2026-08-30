//! Read-only HTTP and WebSocket API for `FireSift`.

use std::{str::FromStr as _, sync::Arc};

use axum::{
    Json, Router,
    extract::{
        Path, Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use geojson::{Feature, FeatureCollection, Geometry, Value};
use grid::{BoundingBox, CellIndex, H3Grid};
use risk::{Factor, Horizon, RiskScore};
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use store::{FwiSnapshot, SourceStatusRow, Store, StoredRiskScore};
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;

mod blue;
mod science;
mod watch;

const DASHBOARD_HTML: &str = include_str!("../static/index.html");
const DASHBOARD_CSS: &str = include_str!("../static/dashboard.css");
const DASHBOARD_JS: &str = include_str!("../static/dashboard.js");
const MAX_RISK_FEATURES: u32 = 10_000;
const MAX_ALERTS: u32 = 500;

/// Shared API dependencies.
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct AppState {
    store: Store,
    grid: H3Grid,
    updates: broadcast::Sender<Arc<RiskUpdate>>,
    aoi: BoundingBox,
    territory_label: String,
    operational_cells: Option<Arc<Vec<CellIndex>>>,
    science_console_enabled: bool,
    blue_center_enabled: bool,
    watch_console_enabled: bool,
}

impl AppState {
    /// Creates API state from initialized application services.
    #[must_use]
    pub fn new(store: Store, grid: H3Grid, updates: broadcast::Sender<Arc<RiskUpdate>>) -> Self {
        Self {
            store,
            grid,
            updates,
            aoi: BoundingBox {
                west: 1.68,
                south: 42.57,
                east: 3.26,
                north: 43.46,
            },
            territory_label: "Aude · Occitanie".to_owned(),
            operational_cells: None,
            science_console_enabled: false,
            blue_center_enabled: false,
            watch_console_enabled: false,
        }
    }

    #[must_use]
    pub fn with_operational_area(mut self, aoi: BoundingBox, label: impl Into<String>) -> Self {
        self.aoi = aoi;
        self.territory_label = label.into();
        self
    }

    #[must_use]
    pub fn with_operational_cells(mut self, cells: Vec<CellIndex>) -> Self {
        self.operational_cells = Some(Arc::new(cells));
        self
    }

    /// Phase 4A: enables mounting the read-only `/science` and
    /// `/api/science/*` routes. Defaults to `false` -- no
    /// authentication exists anywhere in this API yet, so this is a
    /// deployment gate, not real access control.
    #[must_use]
    pub const fn with_science_console_enabled(mut self, enabled: bool) -> Self {
        self.science_console_enabled = enabled;
        self
    }

    #[must_use]
    pub const fn with_blue_center_enabled(mut self, enabled: bool) -> Self {
        self.blue_center_enabled = enabled;
        self
    }

    /// Enables mounting the read-only `/watch` and `/api/watch/*`
    /// routes -- the public wildfire-risk map. Defaults to `false` for
    /// the same reason as `with_science_console_enabled`: a deployment
    /// gate, not real access control.
    #[must_use]
    pub const fn with_watch_console_enabled(mut self, enabled: bool) -> Self {
        self.watch_console_enabled = enabled;
        self
    }

    pub(crate) fn store(&self) -> &Store {
        &self.store
    }
}

/// One real-time score update batch.
#[derive(Clone, Debug, Serialize)]
pub struct RiskUpdate {
    #[serde(rename = "type")]
    kind: &'static str,
    computed_at: DateTime<Utc>,
    valid_at: DateTime<Utc>,
    horizon: Horizon,
    cells: Vec<RiskUpdateCell>,
}

impl RiskUpdate {
    /// Builds a WebSocket update from one persisted score batch.
    #[must_use]
    pub fn from_scores(scores: &[RiskScore]) -> Self {
        Self {
            kind: "risk_update",
            computed_at: scores
                .first()
                .map_or_else(Utc::now, |score| score.computed_at),
            valid_at: scores.first().map_or_else(Utc::now, |score| score.valid_at),
            horizon: scores
                .first()
                .map_or(Horizon::Nowcast, |score| score.horizon),
            cells: scores
                .iter()
                .map(|score| RiskUpdateCell {
                    h3: score.cell.to_string(),
                    score: score.score,
                    physical: score.physical,
                    human: score.human,
                })
                .collect(),
        }
    }

    fn filtered(&self, grid: H3Grid, bbox: Option<BoundingBox>) -> Self {
        let Some(bbox) = bbox else {
            return self.clone();
        };
        Self {
            kind: self.kind,
            computed_at: self.computed_at,
            valid_at: self.valid_at,
            horizon: self.horizon,
            cells: self
                .cells
                .iter()
                .filter(|cell| {
                    CellIndex::from_str(&cell.h3).is_ok_and(|index| {
                        let center = grid.cell_center(index);
                        bbox.contains(center.lat(), center.lng())
                    })
                })
                .cloned()
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct RiskUpdateCell {
    h3: String,
    score: f32,
    physical: f32,
    human: f32,
}

/// Builds the complete application router.
pub fn router(state: AppState) -> Router {
    let mut router = Router::new()
        .route("/", get(dashboard))
        .route("/dashboard.css", get(dashboard_css))
        .route("/dashboard.js", get(dashboard_js))
        .route("/health", get(health))
        .route("/config", get(api_config))
        .route("/risk", get(risk_geojson))
        .route("/risk/cell/{h3}", get(risk_cell))
        .route("/alerts", get(alerts))
        .route("/sources", get(sources))
        .route("/stream", get(stream));

    // Phase 4A: the read-only scientific console is only mounted when
    // explicitly enabled. No authentication exists anywhere in this
    // API, so this flag is a deployment gate, not real access control
    // -- see SCIENTIFIC_CONSOLE_ARCHITECTURE.md.
    if state.science_console_enabled {
        router = router
            .route("/science", get(science_shell))
            .route("/science/{*path}", get(science_shell))
            .route("/science.css", get(science_css))
            .route("/science.js", get(science_js))
            .nest("/api/science", science::router());
    }

    if state.blue_center_enabled {
        router = router
            .route("/blue", get(blue_shell))
            .route("/blue/{*path}", get(blue_shell))
            .route("/blue.css", get(blue_css))
            .route("/blue.js", get(blue_js))
            .nest("/api/blue", blue::router());
    }

    // Public wildfire-risk map: same deployment-gate rationale as the
    // consoles above. Read-only, consumes the existing operational
    // /risk, /risk/cell/{h3}, /sources and /config routes directly.
    if state.watch_console_enabled {
        router = router
            .route("/watch", get(watch_shell))
            .route("/watch/{*path}", get(watch_shell))
            .route("/watch.css", get(watch_css))
            .route("/watch.js", get(watch_js))
            .nest("/api/watch", watch::router());
    }

    router
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

const SCIENCE_HTML: &str = include_str!("../static/science/index.html");
const SCIENCE_CSS: &str = include_str!("../static/science/science.css");
const SCIENCE_JS: &str = include_str!("../static/science/science.js");
const BLUE_HTML: &str = include_str!("../static/blue/index.html");
const BLUE_CSS: &str = include_str!("../static/blue/blue.css");
const BLUE_JS: &str = include_str!("../static/blue/blue.js");

async fn blue_shell() -> Html<&'static str> {
    Html(BLUE_HTML)
}

async fn blue_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        BLUE_CSS,
    )
}

async fn blue_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        BLUE_JS,
    )
}

async fn science_shell() -> Html<&'static str> {
    Html(SCIENCE_HTML)
}

async fn science_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        SCIENCE_CSS,
    )
}

async fn science_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        SCIENCE_JS,
    )
}

const WATCH_HTML: &str = include_str!("../static/watch/index.html");
const WATCH_CSS: &str = include_str!("../static/watch/watch.css");
const WATCH_JS: &str = include_str!("../static/watch/watch.js");

async fn watch_shell() -> Html<&'static str> {
    Html(WATCH_HTML)
}

async fn watch_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        WATCH_CSS,
    )
}

async fn watch_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        WATCH_JS,
    )
}

async fn api_config(State(state): State<AppState>) -> Json<ApiConfigResponse> {
    Json(ApiConfigResponse {
        territory: state.territory_label,
        bbox: [
            state.aoi.west,
            state.aoi.south,
            state.aoi.east,
            state.aoi.north,
        ],
        h3_resolution: u8::from(state.grid.resolution()),
    })
}

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn dashboard_css() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        DASHBOARD_CSS,
    )
}

async fn dashboard_js() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        DASHBOARD_JS,
    )
}

async fn not_found() -> ApiError {
    ApiError::not_found("not_found", "route not found")
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed("method_not_allowed", "HTTP method not allowed")
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    state.store.health_check().await.map_err(|error| {
        tracing::error!(%error, "database health check failed");
        ApiError::service_unavailable("database_unavailable", "database health check failed")
    })?;
    let sources = state
        .store
        .source_statuses()
        .await
        .map_err(database_error)?;
    Ok(Json(HealthResponse {
        status: "ok",
        db: "ok",
        sources: sources.into_iter().map(source_health).collect(),
    }))
}

async fn risk_geojson(
    State(state): State<AppState>,
    Query(query): Query<RiskQuery>,
) -> Result<Json<FeatureCollection>, ApiError> {
    if query.at.as_deref().is_some_and(|at| at != "latest") {
        return Err(ApiError::bad_request(
            "invalid_at",
            "only at=latest is supported",
        ));
    }
    let bbox = query
        .bbox
        .ok_or_else(|| ApiError::bad_request("missing_bbox", "bbox is required"))?
        .parse::<BoundingBox>()
        .map_err(|error| ApiError::bad_request("invalid_bbox", error.to_string()))?;
    let min_score = parse_unit_interval(query.min_score.as_deref(), 0.0, "min_score")?;
    let horizon = parse_horizon(query.horizon.as_deref())?;
    let limit = parse_optional_limit(query.limit.as_deref(), MAX_RISK_FEATURES, "limit")?;
    let covers_operational_area = bbox.west <= state.aoi.west
        && bbox.south <= state.aoi.south
        && bbox.east >= state.aoi.east
        && bbox.north >= state.aoi.north;
    let cells = if limit.is_some() && covers_operational_area {
        Vec::new()
    } else if let Some(operational_cells) = &state.operational_cells {
        operational_cells
            .iter()
            .copied()
            .filter(|cell| {
                let center = state.grid.cell_center(*cell);
                bbox.contains(center.lat(), center.lng())
            })
            .collect()
    } else {
        state
            .grid
            .cells_for_bbox(bbox)
            .map_err(|error| ApiError::bad_request("invalid_bbox", error.to_string()))?
    };
    let point_geometry = match query.geometry.as_deref() {
        None | Some("polygon") => false,
        Some("point") => true,
        Some(_) => {
            return Err(ApiError::bad_request(
                "invalid_geometry",
                "geometry must be polygon or point",
            ));
        }
    };
    let scores = match (limit, covers_operational_area) {
        (Some(limit), true) => {
            state
                .store
                .latest_alerts_limited(min_score, horizon, limit)
                .await
        }
        (Some(limit), false) => {
            state
                .store
                .latest_risk_scores_limited(&cells, min_score, horizon, limit)
                .await
        }
        (None, _) => {
            state
                .store
                .latest_risk_scores(&cells, min_score, horizon)
                .await
        }
    }
    .map_err(database_error)?;
    let truncated = limit.is_some_and(|limit| scores.len() == limit as usize);
    let mut metadata = Map::new();
    metadata.insert("truncated".to_owned(), json!(truncated));
    if let Some(limit) = limit {
        metadata.insert("limit".to_owned(), json!(limit));
    }
    Ok(Json(FeatureCollection {
        bbox: None,
        features: scores
            .iter()
            .map(|score| risk_feature(score, point_geometry))
            .collect(),
        foreign_members: Some(metadata),
    }))
}

async fn risk_cell(
    State(state): State<AppState>,
    Path(h3): Path<String>,
    Query(query): Query<HorizonQuery>,
) -> Result<Json<RiskCellResponse>, ApiError> {
    let cell = CellIndex::from_str(&h3)
        .map_err(|error| ApiError::bad_request("invalid_h3", error.to_string()))?;
    let horizon = parse_horizon(query.horizon.as_deref())?;
    let detail = state
        .store
        .risk_cell(cell, horizon)
        .await
        .map_err(database_error)?
        .ok_or_else(|| ApiError::not_found("risk_not_found", "no score exists for this cell"))?;
    Ok(Json(RiskCellResponse {
        h3,
        current: ScoreResponse::from(&detail.current),
        fwi: FwiResponse::from(detail.fwi),
        features: detail.features,
        history: detail.history.iter().map(ScoreResponse::from).collect(),
    }))
}

async fn alerts(
    State(state): State<AppState>,
    Query(query): Query<AlertsQuery>,
) -> Result<Json<Vec<AlertResponse>>, ApiError> {
    let threshold = parse_unit_interval(query.threshold.as_deref(), 0.8, "threshold")?;
    let horizon = parse_horizon(query.horizon.as_deref())?;
    let limit = parse_optional_limit(query.limit.as_deref(), MAX_ALERTS, "limit")?;
    let scores = match limit {
        Some(limit) => {
            state
                .store
                .latest_alerts_limited(threshold, horizon, limit)
                .await
        }
        None => state.store.latest_alerts(threshold, horizon).await,
    }
    .map_err(database_error)?;
    Ok(Json(
        scores
            .iter()
            .map(|score| {
                let center = state.grid.cell_center(score.cell);
                AlertResponse {
                    h3: score.cell.to_string(),
                    score: score.score,
                    latitude: center.lat(),
                    longitude: center.lng(),
                    computed_at: score.computed_at,
                    valid_at: score.valid_at,
                    horizon: score.horizon,
                }
            })
            .collect(),
    ))
}

async fn sources(State(state): State<AppState>) -> Result<Json<Vec<SourceResponse>>, ApiError> {
    let statuses = state
        .store
        .source_statuses()
        .await
        .map_err(database_error)?;
    Ok(Json(
        statuses.into_iter().map(SourceResponse::from).collect(),
    ))
}

async fn stream(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| stream_socket(socket, state))
}

async fn stream_socket(mut socket: WebSocket, state: AppState) {
    let mut updates = state.updates.subscribe();
    let mut bbox = None;
    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(message)) = message else {
                    break;
                };
                match message {
                    Message::Text(text) => match parse_subscription(&text) {
                        Ok(subscription) => bbox = Some(subscription),
                        Err(error) => {
                            let payload = error_payload("invalid_subscription", &error);
                            if socket.send(Message::Text(payload.into())).await.is_err() {
                                break;
                            }
                        }
                    },
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        let filtered = update.filtered(state.grid, bbox);
                        match serde_json::to_string(&filtered) {
                            Ok(payload) => {
                                if socket.send(Message::Text(payload.into())).await.is_err() {
                                    break;
                                }
                            }
                            Err(error) => {
                                tracing::error!(%error, "failed to serialize WebSocket update");
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "WebSocket subscriber lagged behind");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

fn parse_subscription(text: &str) -> Result<BoundingBox, String> {
    let subscription =
        serde_json::from_str::<Subscription>(text).map_err(|error| error.to_string())?;
    if subscription.kind != "subscribe" {
        return Err("type must be subscribe".to_owned());
    }
    let [west, south, east, north] = subscription.bbox;
    BoundingBox::new(west, south, east, north).map_err(|error| error.to_string())
}

fn risk_feature(score: &StoredRiskScore, point_geometry: bool) -> Feature {
    let geometry = if point_geometry {
        let center = grid::LatLng::from(score.cell);
        Geometry::new(Value::Point(vec![center.lng(), center.lat()]))
    } else {
        let boundary = score.cell.boundary();
        let mut ring = boundary
            .iter()
            .map(|coordinate| vec![coordinate.lng(), coordinate.lat()])
            .collect::<Vec<_>>();
        if let Some(first) = ring.first().cloned() {
            ring.push(first);
        }
        Geometry::new(Value::Polygon(vec![ring]))
    };
    let mut properties = Map::new();
    properties.insert("h3".to_owned(), json!(score.cell.to_string()));
    properties.insert("score".to_owned(), json!(score.score));
    properties.insert("physical".to_owned(), json!(score.physical));
    properties.insert("human".to_owned(), json!(score.human));
    properties.insert("computed_at".to_owned(), json!(score.computed_at));
    properties.insert("valid_at".to_owned(), json!(score.valid_at));
    properties.insert("horizon".to_owned(), json!(score.horizon));
    Feature {
        bbox: None,
        geometry: Some(geometry),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

fn parse_unit_interval(value: Option<&str>, default: f32, name: &str) -> Result<f32, ApiError> {
    let Some(value) = value else {
        return Ok(default);
    };
    let parsed = value
        .parse::<f32>()
        .map_err(|error| ApiError::bad_request(format!("invalid_{name}"), error.to_string()))?;
    if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
        return Err(ApiError::bad_request(
            format!("invalid_{name}"),
            format!("{name} must be between zero and one"),
        ));
    }
    Ok(parsed)
}

fn parse_optional_limit(
    value: Option<&str>,
    maximum: u32,
    name: &str,
) -> Result<Option<u32>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = value
        .parse::<u32>()
        .map_err(|error| ApiError::bad_request(format!("invalid_{name}"), error.to_string()))?;
    if parsed == 0 || parsed > maximum {
        return Err(ApiError::bad_request(
            format!("invalid_{name}"),
            format!("{name} must be between 1 and {maximum}"),
        ));
    }
    Ok(Some(parsed))
}

fn parse_horizon(value: Option<&str>) -> Result<Horizon, ApiError> {
    value.unwrap_or("nowcast").parse().map_err(|error| {
        ApiError::bad_request(
            "invalid_horizon",
            format!("{error}; expected nowcast, hours_6, hours_24, or hours_48"),
        )
    })
}

fn source_health(status: SourceStatusRow) -> SourceHealth {
    let staleness_s = status.last_success.map(|last_success| {
        Utc::now()
            .signed_duration_since(last_success)
            .num_seconds()
            .max(0)
    });
    SourceHealth {
        id: status.id,
        last_success: status.last_success,
        staleness_s,
    }
}

fn database_error(error: store::StoreError) -> ApiError {
    tracing::error!(%error, "API database operation failed");
    drop(error);
    ApiError::service_unavailable("database_unavailable", "database operation failed")
}

pub(crate) fn validate_insee_code(raw: &str) -> Result<String, ApiError> {
    let code = raw.to_uppercase();
    let valid = code.len() == 5
        && if let Some(rest) = code.strip_prefix("2A").or_else(|| code.strip_prefix("2B")) {
            rest.bytes().all(|byte| byte.is_ascii_digit())
        } else {
            code.bytes().all(|byte| byte.is_ascii_digit())
        };
    if !valid {
        return Err(ApiError::bad_request(
            "invalid_insee_code",
            "insee_code must be five digits, or 2A/2B followed by three digits",
        ));
    }
    Ok(code)
}

#[cfg(test)]
mod insee_code_tests {
    use super::validate_insee_code;

    #[test]
    fn accepts_a_plain_five_digit_code() {
        assert_eq!(validate_insee_code("31490").unwrap(), "31490");
    }

    #[test]
    fn accepts_and_normalizes_corsica_prefixes() {
        assert_eq!(validate_insee_code("2a004").unwrap(), "2A004");
    }

    #[test]
    fn rejects_malformed_codes() {
        assert!(validate_insee_code("3144").is_err());
        assert!(validate_insee_code("abcde").is_err());
        assert!(validate_insee_code("314906").is_err());
    }
}

fn error_payload(code: &str, message: &str) -> String {
    json!({"error": {"code": code, "message": message}}).to_string()
}

#[derive(Debug, Deserialize)]
struct RiskQuery {
    bbox: Option<String>,
    min_score: Option<String>,
    at: Option<String>,
    horizon: Option<String>,
    limit: Option<String>,
    geometry: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlertsQuery {
    threshold: Option<String>,
    horizon: Option<String>,
    limit: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HorizonQuery {
    horizon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Subscription {
    #[serde(rename = "type")]
    kind: String,
    bbox: [f64; 4],
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    db: &'static str,
    sources: Vec<SourceHealth>,
}

#[derive(Debug, Serialize)]
struct ApiConfigResponse {
    territory: String,
    bbox: [f64; 4],
    h3_resolution: u8,
}

#[derive(Debug, Serialize)]
struct SourceHealth {
    id: String,
    last_success: Option<DateTime<Utc>>,
    staleness_s: Option<i64>,
}

#[derive(Debug, Serialize)]
struct RiskCellResponse {
    h3: String,
    current: ScoreResponse,
    fwi: FwiResponse,
    features: serde_json::Value,
    history: Vec<ScoreResponse>,
}

#[derive(Debug, Serialize)]
struct ScoreResponse {
    score: f32,
    physical: f32,
    human: f32,
    computed_at: DateTime<Utc>,
    valid_at: DateTime<Utc>,
    horizon: Horizon,
    top_factors: Vec<Factor>,
}

impl From<&StoredRiskScore> for ScoreResponse {
    fn from(score: &StoredRiskScore) -> Self {
        Self {
            score: score.score,
            physical: score.physical,
            human: score.human,
            computed_at: score.computed_at,
            valid_at: score.valid_at,
            horizon: score.horizon,
            top_factors: score.top_factors.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
struct FwiResponse {
    date: chrono::NaiveDate,
    valid_at: DateTime<Utc>,
    ffmc: f64,
    dmc: f64,
    dc: f64,
    isi: f64,
    bui: f64,
    fwi: f64,
}

impl From<FwiSnapshot> for FwiResponse {
    fn from(snapshot: FwiSnapshot) -> Self {
        Self {
            date: snapshot.date,
            valid_at: snapshot.valid_at,
            ffmc: snapshot.ffmc,
            dmc: snapshot.dmc,
            dc: snapshot.dc,
            isi: snapshot.isi,
            bui: snapshot.bui,
            fwi: snapshot.fwi,
        }
    }
}

#[derive(Debug, Serialize)]
struct AlertResponse {
    h3: String,
    score: f32,
    latitude: f64,
    longitude: f64,
    computed_at: DateTime<Utc>,
    valid_at: DateTime<Utc>,
    horizon: Horizon,
}

#[derive(Debug, Serialize)]
struct SourceResponse {
    id: String,
    last_run: DateTime<Utc>,
    last_success: Option<DateTime<Utc>>,
    observation_count: u64,
    recent_error: Option<String>,
}

impl From<SourceStatusRow> for SourceResponse {
    fn from(status: SourceStatusRow) -> Self {
        Self {
            id: status.id,
            last_run: status.last_run,
            last_success: status.last_success,
            observation_count: status.observation_count,
            recent_error: status.recent_error,
        }
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: code.into(),
            message: message.into(),
        }
    }

    fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: code.into(),
            message: message.into(),
        }
    }

    fn method_not_allowed(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code: code.into(),
            message: message.into(),
        }
    }

    fn service_unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: code.into(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};
    use grid::H3Grid;
    use risk::{Horizon, RiskScore};

    use super::{RiskUpdate, parse_subscription};

    #[test]
    fn subscription_requires_a_valid_bbox() {
        let bbox = parse_subscription(r#"{"type":"subscribe","bbox":[2.34,43.20,2.36,43.22]}"#)
            .expect("valid subscription");
        assert!(bbox.contains(43.21, 2.35));
        assert!(parse_subscription(r#"{"type":"other","bbox":[2.34,43.20,2.36,43.22]}"#).is_err());
    }

    #[test]
    fn websocket_update_filters_cells_by_bbox() {
        let grid = H3Grid::new(9).expect("valid grid");
        let inside = grid.cell_for_point(43.2122, 2.3537).expect("valid cell");
        let outside = grid.cell_for_point(43.30, 2.55).expect("valid cell");
        let computed_at = Utc.with_ymd_and_hms(2025, 7, 16, 14, 30, 0).unwrap();
        let scores = [inside, outside].map(|cell| RiskScore {
            cell,
            computed_at,
            valid_at: computed_at,
            horizon: Horizon::Nowcast,
            score: 0.8,
            physical: 0.9,
            human: 0.7,
            top_factors: Vec::new(),
        });
        let bbox = "2.34,43.20,2.36,43.22".parse().expect("valid bbox");
        let update = RiskUpdate::from_scores(&scores).filtered(grid, Some(bbox));

        assert_eq!(update.cells.len(), 1);
        assert_eq!(update.cells[0].h3, inside.to_string());
    }
}
