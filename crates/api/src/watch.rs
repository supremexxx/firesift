//! Public wildfire-risk map ("`FireSift` Watch") API. Read-only, and
//! deliberately thin: risk data itself comes from the existing
//! top-level `/risk`, `/risk/cell/{h3}`, `/sources` and `/config`
//! routes, unchanged. The only thing missing for a commune-name search
//! UI is exposed here, reusing the existing commune catalog/boundary
//! store methods that already back the client console.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{ApiError, AppState, database_error, validate_insee_code};

/// Builds the `/api/watch/*` sub-router. The caller decides whether to
/// nest this at all (only when `AppState::watch_console_enabled` is
/// true).
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/communes", get(commune_search))
        .route("/communes/{insee_code}", get(commune_lookup))
}

const MAX_COMMUNE_SEARCH_RESULTS: i64 = 20;

#[derive(Debug, Deserialize)]
struct CommuneSearchQuery {
    q: String,
}

#[derive(Debug, Serialize)]
struct CommuneSearchResponse {
    insee_code: String,
    name: String,
    department_code: Option<String>,
}

async fn commune_search(
    State(state): State<AppState>,
    Query(query): Query<CommuneSearchQuery>,
) -> Result<Json<Vec<CommuneSearchResponse>>, ApiError> {
    let prefix = query.q.trim();
    if prefix.chars().count() < 2 {
        return Err(ApiError::bad_request(
            "query_too_short",
            "q must be at least two characters",
        ));
    }
    let results = state
        .store()
        .search_communes(prefix, MAX_COMMUNE_SEARCH_RESULTS)
        .await
        .map_err(database_error)?;
    Ok(Json(
        results
            .into_iter()
            .map(|entry| CommuneSearchResponse {
                insee_code: entry.insee_code,
                name: entry.name,
                department_code: entry.department_code,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
struct CommuneLookupResponse {
    insee_code: String,
    name: String,
    bbox: [f64; 4],
}

async fn commune_lookup(
    State(state): State<AppState>,
    Path(insee_code): Path<String>,
) -> Result<Json<CommuneLookupResponse>, ApiError> {
    let insee_code = validate_insee_code(&insee_code)?;
    let boundary = state
        .store()
        .commune_boundary(&insee_code)
        .await
        .map_err(database_error)?
        .ok_or_else(|| {
            ApiError::not_found("commune_not_found", "no commune boundary is registered")
        })?;
    Ok(Json(CommuneLookupResponse {
        insee_code: boundary.insee_code,
        name: boundary.name,
        bbox: [
            boundary.bbox.west,
            boundary.bbox.south,
            boundary.bbox.east,
            boundary.bbox.north,
        ],
    }))
}
