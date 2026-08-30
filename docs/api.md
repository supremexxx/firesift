# API

All endpoints are unauthenticated `GET`/`WS` reads unless noted. There is
no write, import, training, migration, or model-activation endpoint
anywhere in the HTTP API — every route in `crates/api/src` is registered
with `get(...)`. Stability tiers below reflect actual project maturity;
do not treat `experimental` routes as stable contracts.

## Operational (stable-ish, pre-1.0)

| Endpoint | Description |
|---|---|
| `GET /health` | Service and data-source health, backed by a live database check |
| `GET /` | Operational dashboard (HTML) |
| `GET /config` | Public runtime configuration |
| `GET /risk` | Risk surfaces as GeoJSON, over the configured AOI, for a given horizon |
| `GET /risk/cell/{h3}` | Explained risk for a single H3 cell (factor breakdown) |
| `GET /alerts` | Cells exceeding a configured risk threshold |
| `GET /sources` | Per-source ingestion/freshness status |
| `WS /stream` | Live risk-update push |

`nowcast`, `+6h`, `+24h`, and `+48h` horizons are supported where a
`horizon` parameter is accepted; see `crates/risk` for the `Horizon` enum.

- Flag: none — always mounted.
- Target audience: the public dashboard and any client consuming risk
  data directly.
- Authentication: none, by design; this surface is meant to be public.
- Read-only: yes, all `GET`/`WS`.

## Scientific console — `experimental`, disabled by default

Mounted only when `SCIENCE_CONSOLE_ENABLED=true`; all routes under
`/api/science/*` are read-only (`overview`, `progress`, `sources`,
`imports`, `pipelines`, `data-quality`, `data-quality/events`, `features`,
`calendar`, `datasets`, `datasets/{logical_id}`, `models`, `system`,
`observability/latest`, `observability/history`, `observability/compare`,
`observability/attempts`, `snapshots`, `snapshots/{id}`,
`snapshots/{id}/verification`, `snapshot-labels/summary`,
`snapshot-alerts`). See
[`docs/research/reports/SCIENTIFIC_CONSOLE_DATA_CONTRACTS.md`](research/reports/SCIENTIFIC_CONSOLE_DATA_CONTRACTS.md)
for response shapes. `SCIENCE_CONSOLE_ENABLED` is a deployment lock, not an
authentication mechanism — a public deployment must put its own
authentication (e.g. a reverse-proxy basic-auth layer) in front of it if it
is enabled and not meant to be fully public.

- Flag: `SCIENCE_CONSOLE_ENABLED`, default `false`.
- Target audience: internal/scientific — dataset, pipeline, and model
  registry inspection.
- Authentication: none in the API itself; deployments are expected to add
  reverse-proxy Basic Auth if enabling this publicly.
- When the flag is off: `/science`, `/science/{*path}`, and every
  `/api/science/*` route return `404`.
- Read-only: yes, every route is `GET`.

## BLUE forecast-evidence center — `experimental`, disabled by default

Mounted only when `BLUE_CENTER_ENABLED=true`; every route under
`/api/blue/*` is `GET`:

| Endpoint | Description |
|---|---|
| `GET /api/blue/overview` | Summary counters for the forecast-evidence center |
| `GET /api/blue/bulletins` | Daily forecast bulletins (immutable `+24h`/`+48h` snapshots) |
| `GET /api/blue/performance` | Aggregate hit/miss performance by horizon |
| `GET /api/blue/ground-truth` | Ground-truth matching summary, confirmation and rejection counts |
| `GET /api/blue/cases` | Individual evidence cases under review |
| `GET /api/blue/alerts` | BLUE-specific alert list |
| `GET /api/blue/alerts/{id}` | Detail for one BLUE alert |

- Flag: `BLUE_CENTER_ENABLED`, default `false`. Two related flags change
  *how* evidence is gathered, not whether the API is mounted:
  `BLUE_AI_EVIDENCE_ENABLED` (default `false`, requires `OPENAI_API_KEY`
  to do anything) enables an automatic OpenAI-backed evidence reviewer;
  `BLUE_FEUX_DE_FORET_ENABLED` narrows its scope.
- Stability: `experimental`. BLUE is an active foundation, not a
  finished prospective-validation system — see
  [`docs/architecture.md#blue-forecast-evidence-center`](architecture.md#blue-forecast-evidence-center)
  and [`docs/scientific-limitations.md`](scientific-limitations.md).
- Target audience: internal/scientific review of forecast accuracy over
  time, not a public dashboard.
- Authentication: none. Reverse-proxy protection is recommended if this
  is enabled on a public deployment.
- When the flag is off: `/blue`, `/blue/{*path}`, and every
  `/api/blue/*` route return `404`.
- Read-only via HTTP: yes. All BLUE writes happen from `engine`'s
  scheduler (`poll_blue_evidence`, `poll_blue_ground_truth`), never from
  an HTTP request.

## Watch public map — `experimental`, disabled by default

Mounted only when `WATCH_CONSOLE_ENABLED=true`:

| Endpoint | Description |
|---|---|
| `GET /watch`, `/watch/{*path}` | The Watch single-page map (HTML shell) |
| `GET /watch.css`, `/watch.js` | Watch's bundled stylesheet and script |
| `GET /api/watch/communes?q=<prefix>` | Commune name search, `q` must be at least 2 characters (else `400 query_too_short`); returns up to 20 matches |
| `GET /api/watch/communes/{insee_code}` | Bounding box lookup for one commune by INSEE code; `404 commune_not_found` if unregistered, `400` if `insee_code` fails the standard 5-character/Corsica-prefix format check |

Watch's risk data itself is **not** duplicated here: the map consumes the
existing unconditional `/risk`, `/risk/cell/{h3}`, `/sources`, and
`/config` routes directly, unchanged. `/api/watch/*` only adds the
commune search needed for "type a commune name, pan the map."

- Flag: `WATCH_CONSOLE_ENABLED`, default `false`.
- Stability: `experimental`, present in `Unreleased` (not yet part of a
  tagged release as of this writing).
- Target audience: the general public — a plain-language, map-first view
  of the same risk data the operational dashboard exposes.
- Authentication: none. Like every other console here, the flag is a
  deployment gate, not a login.
- When the flag is off: `/watch`, `/watch/{*path}`, `/watch.css`,
  `/watch.js`, and every `/api/watch/*` route return `404`; `/health`
  and the operational core are unaffected.
- Read-only: yes, every route is `GET`.
- **Not an official wildfire warning** — see the in-app disclaimer and
  [`docs/scientific-limitations.md`](scientific-limitations.md).

## Internal / not part of the public contract

Anything under `crates/api/static` (dashboard/science/blue/watch HTML,
CSS, JS assets) is served as-is and is an implementation detail of the
bundled UI, not a documented API.

## Stability

This project is pre-1.0. The Cargo workspace version is still `0.1.0`;
the most recent tagged repository release is `v0.5.0` — see
[`docs/project-identity.md`](project-identity.md) for how those two
numbers relate. No endpoint listed above is guaranteed stable across
releases yet, and `experimental` routes (scientific console, BLUE, Watch)
should not be treated as stable contracts at all;
breaking changes will be called out in [`CHANGELOG.md`](../CHANGELOG.md),
not silently shipped.
