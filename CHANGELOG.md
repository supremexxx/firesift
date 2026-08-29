# Changelog

All notable changes to FireSift are documented in this file.

## [Unreleased]

### Added

- **BLUE forecast-evidence center** (`/blue`, `/api/blue/*`, gated
  behind `BLUE_CENTER_ENABLED`, default `false`): immutable `+24h`/`+48h`
  forecast-evidence archiving, a statistical bulletin view, insurer
  evidence reports, and a ground-truth feedback loop comparing forecasts
  against observed outcomes. An optional AI-assisted evidence reviewer
  (`BLUE_AI_EVIDENCE_ENABLED`, requires `OPENAI_API_KEY`;
  `BLUE_FEUX_DE_FORET_ENABLED` narrows its scope) shows interim review
  responses and is now configured to use `gpt-4o-mini` by default.
  Evidence selection was hardened to ground deterministically, preserve
  existing selections across reruns, and resume invalidated evidence
  with a fresh attempt instead of silently stalling.
- **Community/terrain-reported BLUE evidence**: migration
  `0032_blue_community_evidence.sql` adds `community_reported`,
  `press_confirmed`, and `authority_confirmed` evidence levels, plus a
  dedicated `blue.ground_truth_rejections` table so a community report
  that turns out to be a false alarm is archived separately and can
  never itself create a positive ground-truth match.
- **FireSift Watch** (`/watch`, `/api/watch/*`, gated behind
  `WATCH_CONSOLE_ENABLED`, default `false`): an experimental public map
  console reusing the existing `/risk`, `/risk/cell/{h3}`, `/sources`,
  and `/config` routes, plus commune name search and bbox lookup. Not
  yet part of a tagged release.
- Deployment provenance hardening for `deploy/oracle/`:
  `deploy-code.sh` now refuses to deploy tracked, uncommitted changes or
  untracked build inputs (`Cargo.toml`, `Cargo.lock`, `Dockerfile`,
  `crates`, `migrations`, `testdata`), and captures the deployed Git
  revision and built image digest into the remote `.env` for
  after-the-fact verification.
- `docs/project-identity.md`, explaining the FireSift/Erytheon/PyroRisk
  naming (which identifiers persist where, and why) and how the Cargo
  workspace version, Git tags, model versions, dataset versions, and
  migration numbers relate to each other.

### Changed

- Public branding fully renamed from Erytheon to FireSift across the
  dashboard, scientific console, and client console UI copy, crate doc
  comments, and OCI build-time labels — completing the rename begun in
  `v0.5.0`'s repository/documentation-level changes.
- `.github/workflows/container.yml` no longer dual-publishes to
  `ghcr.io/supremexxx/erytheon`; it now publishes only
  `ghcr.io/supremexxx/firesift`, closing out the rename transition.
- `docs/scientific-limitations.md` updated to reflect BLUE as a partial,
  not absent, prospective-validation foundation.
- `docs/architecture.md`, `docs/api.md`, `ROADMAP.md`, and `README.md`
  updated to describe all 5 HTTP surfaces (operational core, scientific,
  territorial/client, BLUE, Watch) instead of 3, correct a stale
  `v0.4.x` reference, and close Phase 4A.3 for its historical scope
  (scientific console only) while opening a new transverse-stabilization
  phase for Client/BLUE/Watch ahead of Phase 4B and P3.
- `PR1_INTEGRATION_REVIEW_REPORT.md` archived to
  `docs/research/reports/` with a header note clarifying its 2026-07-28
  `BLOCKED` verdict describes a since-merged PR at the time, not the
  current state of `main`.

### Fixed

- BLUE no longer crashes when no bulletin has been published yet
  (`crates/api/static/blue/blue.js`); it now shows a friendly empty
  state instead of a raw `Cannot read properties of null` error.
- The Client console now shows the scientific-limitations disclaimer
  mirroring Watch's — it was the only console still missing one.
- BLUE's "not an official alert" disclaimer is now explicit in all four
  of its views (Analyse, Tableau, Performance, Terrain); previously each
  view only had domain-specific score/evidence caveats, none of them
  stating this directly.
- BLUE no longer overflows horizontally on mobile viewports.
- `deploy/oracle/deploy-code.sh` now preserves runtime file permissions
  during deployment instead of resetting them.
- The CLI `engine forecast` command no longer writes an orphaned
  `weather_forecast` source-status row on failure — nothing else in the
  codebase ever clears it, so a single failed manual run left the
  operational dashboard permanently reporting a false `ERREUR` for that
  source. The command already surfaces the failure via its own exit
  code, so the write was redundant. (A pre-existing stuck row from
  before this fix was cleared directly in production; the fix here
  prevents it from happening again.)
- GitHub license detection, by renaming `LICENSE` to `COPYRIGHT` (GitHub
  was misidentifying the project's dual-license setup from the old
  filename).
- Interpolated forecast humidity is now clamped to a physically valid
  range (`crates/engine/src/forecast.rs`).
- Cargo cache mounts in the container build now scope per target
  architecture, fixing a cross-arch cache race.
- Rollback-guard test coverage restored across the full `0009`–`0032`
  migration chain.
- Embedded migrations are now correctly invalidated and re-embedded on
  change (`crates/store/build.rs`), instead of potentially serving a
  stale cached build.
- Watch's freshness indicator now uses `computed_at` (the age of the
  risk data actually on screen) instead of `valid_at`, and its "live
  sources" list is restricted to `firms` and `ecmwf_ifs025_direct` — the
  only sources actually polled on a recurring cycle — instead of
  including one-time static loads and unused fallback sources, which
  made the map read as stale when it wasn't.

### Security

- `deploy-code.sh` build-arg names (`ERYTHEON_GIT_COMMIT`,
  `ERYTHEON_SCIENCE_CONSOLE`) updated to match the Dockerfile's current
  `FIRESIFT_GIT_COMMIT`/`FIRESIFT_SCIENCE_CONSOLE` ARG names — the
  Erytheon-to-FireSift rename had silently made the original build-args
  no-ops, so deployed images stopped actually receiving their intended
  git-revision and science-console build-time labels.

## [0.5.0] - 2026-08-17 - First Open Research Release

### Added

- Copyright holder set in `LICENSE-MIT`/`LICENSE-APACHE`:
  `Copyright (c) 2026 William Ducamp`. Dual license (MIT OR Apache-2.0)
  unchanged.
- `NOTICE.md`, a quick-reference attribution list for all data sources
  (full detail stays in `docs/data-sources.md`).
- `docs/release-notes-v0.5.0.md` — release notes published as this
  `v0.5.0` GitHub Release.
- `LICENSE`, `LICENSE-MIT`, `LICENSE-APACHE` matching the `MIT OR Apache-2.0`
  license already declared in `Cargo.toml`.
- `docs/data-sources.md` documenting per-source data licensing, attribution,
  and redistribution status (NASA FIRMS, Météo-France, ECMWF, Open-Meteo,
  BDIFF, Prométhée, OpenStreetMap, CORINE Land Cover, INSEE).
- `docs/architecture.md`, `docs/api.md`, `docs/deployment.md`,
  `docs/models.md`, `docs/reproducibility.md`,
  `docs/scientific-methodology.md`, `docs/scientific-limitations.md`,
  `docs/public-platform.md` (vision document, not implemented).
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `GOVERNANCE.md`.
- GitHub issue templates (bug report, feature request, data source
  proposal, scientific/model proposal) and a pull request template under
  `.github/`.
- `.github/dependabot.yml` for Cargo, GitHub Actions, and Docker, on a
  weekly cadence with grouped minor/patch updates.
- `OPEN_SOURCE_READINESS_REPORT.md` — full audit ahead of public release.
- An "Open-source track" section in `ROADMAP.md` (Phases A–F), separate
  from the existing scientific/product roadmap.
- English-language root `README.md` rewritten for an open-source research
  audience, replacing the previous French-language version (content
  preserved and expanded, not just translated — see the readiness report
  for what changed).

### Changed

- Reorganized ~70 root-level phase/report Markdown documents into
  `docs/research/phases/` and `docs/research/reports/` (via `git mv`, to
  preserve file history), with a `docs/research/README.md` index. Internal
  cross-document links were updated to match.
- `docs/data-sources.md` license statuses re-verified against each
  provider's current live terms (not from memory): most sources moved
  from an unverified/uncertain status to `CLEAR`, with BDIFF and
  Open-Meteo given precise, conditional wording instead of a blanket
  status. See `OPEN_SOURCE_READINESS_REPORT.md`.
- README Quick Start and `CONTRIBUTING.md` now document a `gdal`/`eccodes`
  host prerequisite for the ECMWF direct-weather path, and a note on
  resolving a port-5432 conflict — both found by actually running the
  Quick Start end-to-end rather than only reading it.
- `.github/workflows/ci.yml` now declares an explicit minimal
  `permissions: contents: read` (defense-in-depth; no functional change).
- Replaced `testdata/promethee_aude.csv`'s single row with clearly
  synthetic data (fictional municipality, `SYNTH-`-prefixed ID), closing
  an earlier "unconfirmed provenance" question rather than leaving it
  open. No test asserted the row's specific values, so this is
  behavior-neutral; `crates/engine`'s static-layer tests were re-run to
  confirm.
- Reclassified administrative boundaries and territorial calendars in
  `docs/data-sources.md` from a blanket `REQUIRES LEGAL / LICENSE REVIEW`
  to precise statuses (`NOT BUNDLED / USER PROVIDED` for boundaries;
  `CLEAR` for the bundled calendar fixture, `NOT BUNDLED` for a real
  production calendar) after confirming what actually ships.

### Security

- Redacted a real production VPS public IP address and system hostname
  that were committed in three deployment/runbook documents, replacing
  them with placeholders. See `OPEN_SOURCE_READINESS_REPORT.md` for detail
  and for the full security audit (no secrets or credentials were found in
  the current tree or Git history via `gitleaks`). The same IP/hostname
  remain in Git history across most tags and branches — this is now a
  recorded, explicit accept-risk decision by the repository owner
  (William Ducamp), not an open question; see the readiness
  report's "Git history security" section for the exact scope and the
  commands to remove them if the maintainer chooses to.

- Direct, credential-free ECMWF IFS open-data weather acquisition with local decoded-grid caching
  and controlled Open-Meteo fallback.
- Phase 0 Cargo workspace with the seven required crates.
- PostgreSQL 16/PostGIS Docker Compose service and initial SQLx migration.
- Typed, validated environment configuration with documented defaults.
- Axum `GET /health` endpoint backed by a live database check.
- Structured tracing, graceful shutdown, CI checks, and setup documentation.
- Phase 1 pure Rust implementation of FFMC, DMC, DC, ISI, BUI, and FWI.
- Typed daily weather, persistent moisture state, outputs, and input errors.
- Standard 48-day `cffdrs` reference fixture with precision-aware validation.
- Phase 2 H3 point projection and lossless PostgreSQL `BIGINT` conversion.
- NASA FIRMS VIIRS S-NPP connector with real API windowing and fixture fallback.
- Idempotent observation persistence through source-specific deduplication keys.
- `engine backfill --source firms --days N` and static H3 GeoJSON export.
- Phase 3 Météo-France SYNOP connector with OAuth2 real access and official fixture fallback.
- Complete H3 AOI coverage and four-nearest-station inverse-distance interpolation.
- Daily moisture-code carry-over plus latest-wind intraday ISI/FWI recomputation.
- Set-based, idempotent PostgreSQL persistence for one FWI row per AOI cell and date.
- Phase 4 one-shot OSM, BDIFF, Prométhée, CORINE, INSEE, and calendar loaders.
- Direct Geofabrik PBF support, EPSG:3035 reprojection, and AOI-clipped GDAL GeoTIFF support.
- Ring-based road/POI/power densities, historical ignition kernel, and 50-metre WUI calculation.
- Idempotent `engine load-static`, `cell_static`, `ignition_history`, and `calendar_days` persistence.
- Phase 5 configurable `HeuristicV1` physical/human fusion with combustible masking and ranked factors.
- Atomic, AOI-scoped `risk_scores` batches retaining both input date and calculation time.
- Complete `/risk`, `/risk/cell/{h3}`, `/alerts`, `/sources`, and enriched `/health` REST API.
- Bbox-filterable `/stream` WebSocket risk updates and resilient FIRMS/Météo-France scheduler loops.
- Source execution status persistence, new-data triggers, and a configurable 15-minute risk safety tick.
- End-to-end fixture coverage from static/weather ingestion through valid GeoJSON and explainable scores.
- Phase 6 monthly Météo-France SYNOP archive loader with Gzip and plain-CSV support.
- Leakage-safe daily FWI and historical-density replay through `engine backtest --from --to`.
- Markdown evaluation report with approximate AUC, top-5%/top-10% ignition capture, and worst false negatives.
- Official 2025 Aude summer BDIFF evaluation fixture covering 89 forest-fire alerts.
- Embedded read-only operator dashboard served at `/` with an interactive H3 risk map.
- Threshold filtering, prioritized alerts, source health, cell explanations, FWI detail, and live WebSocket refreshes.
- Phase 7 fixture/production data profiles with strict real-file and connector validation.
- `data-status` CLI audit for configured static files, GDAL readiness, and PostgreSQL feature coverage.
- Configurable historical FWI warm-up, defaulting to 31 days before scored backtest dates.
- Phase 8 live Open-Meteo transport for Météo-France AROME/ARPEGE forecasts.
- Four atomic risk horizons at nowcast, +6 hours, +24 hours, and +48 hours with forecast-valid timestamps.
- Forecast-noon moisture-code progression, target-hour wind recomputation, and bounded forecast-batch retention.
- Horizon-aware risk, alert, cell-detail, GeoJSON, and WebSocket API contracts.
- Dashboard horizon controls with forecast-valid times and horizon-specific FWI explanations.
- Phase 9A non-root multi-architecture production image for AMD64 and Oracle ARM64 hosts.
- Isolated Oracle Compose stack with Caddy ingress, private PostGIS networking, health checks, and persistent volumes.
- Rolling Cloudflare R2 backup and guarded restore scripts with a daily systemd timer.
- GHCR multi-platform publishing workflow and zero-budget deployment runbook.
- Phase 9B official metropolitan-France department boundary download and `territory-plan` workload audit.
- Unique centroid-owned H3 resolution-8 department partitions with optional `TERRITORY_CODES` rollout filters.
- Sequential national cell-feature, FIRMS-triggered, and AROME/ARPEGE forecast processing with bounded per-partition calculation batches.
- Forecast batch lifecycle that hides partial national runs and atomically publishes only completed surfaces.
- Runtime dashboard configuration for the active territory, bbox, and H3 resolution.
- Phase 9C sequential regional Geofabrik PBF ingestion with relevant-node filtering and bounded regional working sets.
- Reusable per-H3 OSM JSONL cache plus checksummed metropolitan-France regional download workflow.
