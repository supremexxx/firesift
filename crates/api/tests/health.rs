use api::AppState;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::json;
use store::Store;
use tokio::sync::broadcast;
use tower::ServiceExt;

#[tokio::test]
async fn root_is_not_found_and_health_is_served() {
    dotenvy::dotenv().ok();
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping database integration test: DATABASE_URL is not configured");
        return;
    };

    let store = Store::connect(&database_url)
        .await
        .expect("database should accept connections and migrations");
    let grid = grid::H3Grid::new(9).expect("valid grid");
    let (updates, _) = broadcast::channel(1);
    let aoi = grid::BoundingBox::new(-5.15, 41.31, 9.57, 51.09).expect("valid France bbox");
    let app = api::router(
        AppState::new(store, grid, updates).with_operational_area(aoi, "France métropolitaine"),
        "testdata/no-such-web-assets-dir",
    );
    // All bundled web interfaces were removed on 2026-08-30 -- this is now
    // a bare read-only data API. With no built frontend at the given
    // path, "/" falls through to a plain 404 instead of an HTML shell.
    let root = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("root request should complete");
    assert_eq!(root.status(), StatusCode::NOT_FOUND);

    let config_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/config")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("config request should complete");
    assert_eq!(config_response.status(), StatusCode::OK);
    let config_body = to_bytes(config_response.into_body(), 1024)
        .await
        .expect("config body should be readable");
    let config: serde_json::Value =
        serde_json::from_slice(&config_body).expect("config body should be JSON");
    assert_eq!(config["territory"], json!("France métropolitaine"));
    assert_eq!(config["h3_resolution"], json!(9));
    assert_eq!(config["bbox"], json!([-5.15, 41.31, 9.57, 51.09]));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("health request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024)
        .await
        .expect("response body should be readable");
    let payload: serde_json::Value =
        serde_json::from_slice(&body).expect("response body should be JSON");
    assert_eq!(payload["status"], json!("ok"));
    assert_eq!(payload["db"], json!("ok"));
    assert!(payload["sources"].is_array());
}

#[tokio::test]
async fn built_web_assets_are_served_with_an_spa_fallback() {
    dotenvy::dotenv().ok();
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping database integration test: DATABASE_URL is not configured");
        return;
    };

    let assets_dir = std::env::temp_dir().join(format!(
        "firesift-api-test-web-assets-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&assets_dir).expect("temp assets dir should be creatable");
    std::fs::write(
        assets_dir.join("index.html"),
        "<!doctype html><title>spa shell</title>",
    )
    .expect("index.html should be writable");
    std::fs::write(assets_dir.join("app.js"), "console.log('real asset')")
        .expect("app.js should be writable");

    let store = Store::connect(&database_url)
        .await
        .expect("database should accept connections and migrations");
    let grid = grid::H3Grid::new(9).expect("valid grid");
    let (updates, _) = broadcast::channel(1);
    let aoi = grid::BoundingBox::new(-5.15, 41.31, 9.57, 51.09).expect("valid France bbox");
    let app = api::router(
        AppState::new(store, grid, updates).with_operational_area(aoi, "France métropolitaine"),
        &assets_dir,
    );

    // A real file on disk is served as-is.
    let asset = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app.js")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("asset request should complete");
    assert_eq!(asset.status(), StatusCode::OK);
    let asset_body = to_bytes(asset.into_body(), 1024)
        .await
        .expect("asset body should be readable");
    assert_eq!(asset_body, "console.log('real asset')".as_bytes());

    // Any path that isn't a real file or an API route falls back to
    // index.html, so client-side routing works.
    let spa_route = app
        .oneshot(
            Request::builder()
                .uri("/some/client-side/route")
                .body(Body::empty())
                .expect("request should be valid"),
        )
        .await
        .expect("SPA fallback request should complete");
    assert_eq!(spa_route.status(), StatusCode::OK);
    let spa_body = to_bytes(spa_route.into_body(), 1024)
        .await
        .expect("SPA fallback body should be readable");
    assert!(String::from_utf8_lossy(&spa_body).contains("spa shell"));

    std::fs::remove_dir_all(&assets_dir).ok();
}
