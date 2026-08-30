# API

All endpoints are unauthenticated `GET`/`WS` reads. There is no write,
import, training, migration, or model-activation endpoint anywhere in
the HTTP API — every route in `crates/api/src/lib.rs` is registered with
`get(...)`.

**As of 2026-08-30, this is the entire API.** The scientific console,
BLUE, Watch, and the operational dashboard's HTML shell were all removed
in one pass — the project owner wants the web interface layer rebuilt
from scratch rather than incrementally reworked. See
[`ROADMAP.md`](../ROADMAP.md) for the removal record.

## Operational (stable-ish, pre-1.0)

| Endpoint | Description |
|---|---|
| `GET /health` | Service and data-source health, backed by a live database check |
| `GET /config` | Public runtime configuration |
| `GET /risk` | Risk surfaces as GeoJSON, over the configured AOI, for a given horizon |
| `GET /risk/cell/{h3}` | Explained risk for a single H3 cell (factor breakdown) |
| `GET /alerts` | Cells exceeding a configured risk threshold |
| `GET /sources` | Per-source ingestion/freshness status |
| `WS /stream` | Live risk-update push |

`nowcast`, `+6h`, `+24h`, and `+48h` horizons are supported where a
`horizon` parameter is accepted; see `crates/risk` for the `Horizon` enum.

- Flag: none — always mounted.
- Target audience: any client consuming risk data directly. There is no
  bundled web interface consuming these routes right now.
- Authentication: none, by design; this surface is meant to be public.
- Read-only: yes, all `GET`/`WS`.
- `GET /` returns `404` — there is no HTML page at the root anymore.

## BLUE's data collection keeps running, without an HTTP surface

`BLUE_CENTER_ENABLED` still exists as an `engine`-side config flag
(`crates/engine/src/config.rs`) and still gates the scheduler's
background evidence-archiving tasks (`poll_blue_evidence`, the daily
bulletin capture inside `poll_forecast`) — this is data collection into
the `blue.*` database schema, not a web interface, and was deliberately
left untouched when the API surfaces were removed. There is currently no
`/api/blue/*` route to read this data through; it will need a new
interface (or a direct database read) to inspect until one is built.

## Removed API surfaces (2026-08-30)

The following existed before this date and no longer do. Listed here so
anyone rebuilding the interface layer knows what shape the old contracts
had — this is historical reference, not a current or planned contract.

- **Scientific console** — `/science`, `/api/science/*` (~25 read-only
  endpoints: overview, progress, sources, imports, pipelines,
  data-quality, features, datasets, models, system, observability,
  snapshots). Internal dataset/pipeline/model-registry inspection tool.
- **BLUE forecast-evidence center** — `/blue`, `/api/blue/*`
  (`overview`, `bulletins`, `performance`, `ground-truth`, `cases`,
  `alerts`, `alerts/{id}`). The HTTP read surface over the `blue.*`
  archive; the archive itself still exists and is still being written to
  (see above).
- **Watch public map** — `/watch`, `/api/watch/*` (`communes` search,
  `communes/{insee_code}` bbox lookup). A public map console that reused
  `/risk`, `/risk/cell/{h3}`, `/sources`, `/config` directly.
- **Operational dashboard HTML shell** — `/`, `/dashboard.css`,
  `/dashboard.js`. The underlying `/risk`/`/alerts`/`/sources`/`/stream`
  routes it displayed are unaffected; only the page rendering them is
  gone.

## Stability

This project is pre-1.0. The Cargo workspace version is still `0.1.0`;
the most recent tagged repository release is `v0.5.0` — see
[`docs/project-identity.md`](project-identity.md) for how those two
numbers relate. No endpoint listed above is guaranteed stable across
releases yet; breaking changes will be called out in
[`CHANGELOG.md`](../CHANGELOG.md), not silently shipped.
