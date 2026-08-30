# Architecture

FireSift is a single Rust Cargo workspace of nine crates and one
PostgreSQL/PostGIS database. **As of 2026-08-30, the HTTP API has no
bundled web interface at all.** The dashboard, the scientific console,
BLUE, and Watch were all removed in one pass — the project owner wants
them rebuilt from scratch rather than incrementally reworked. What
remains is the bare read-only operational API described below; see
[`ROADMAP.md`](../ROADMAP.md) for the removal record and what comes
next.

## Crates

| Crate | Responsibility |
|---|---|
| `engine` | Configuration, CLI commands, scheduler (FIRMS/forecast polling, BLUE evidence and ground-truth refresh, operational snapshots), orchestration, binary entry point (`pyrorisk`). The only crate that writes on a recurring schedule. |
| `api` | Axum HTTP/WebSocket API, entirely read-only. No web interface — see [API surfaces](#api-surfaces). |
| `store` | PostgreSQL/PostGIS access, migrations, repositories — the only crate that talks to the database directly. |
| `ingest` | Source connectors and normalization (FIRMS, Météo-France, ECMWF, Open-Meteo, BDIFF, Prométhée, OSM, CORINE, INSEE, calendars) |
| `dataset` | Scientific dataset construction and versioning |
| `quality` | Data-quality audits and validation rules |
| `risk` | Operational (v1) scoring and explainable fusion |
| `fwi` | Canadian Fire Weather Index computation |
| `grid` | H3 grid, bounding boxes, geographic conversions |

```mermaid
flowchart LR
    subgraph Sources
        FIRMS[NASA FIRMS]
        MF[Meteo-France / ECMWF / Open-Meteo]
        HIST[BDIFF / Prometheee]
        TERR[OSM / CORINE / INSEE / calendars]
    end

    subgraph ingest_dataset["ingest + dataset"]
        ING[ingest]
        DS[dataset]
    end

    subgraph compute["fwi + risk + grid"]
        FWI[fwi]
        RISK[risk]
        GRID[grid]
    end

    subgraph store_layer["store (PostgreSQL / PostGIS)"]
        STORE[(raw / staging / fire / ml / ops schemas)]
    end

    subgraph api_layer["api (Axum) -- read-only data API, no web interface"]
        OPS[/health /config /risk /alerts /sources /stream]
    end

    ENGINE[engine: scheduler + CLI]

    Sources --> ING --> STORE
    STORE --> DS
    STORE --> FWI --> RISK
    GRID --> RISK
    RISK --> STORE
    STORE --> OPS
    ENGINE --> ING
    ENGINE --> FWI
    ENGINE --> RISK
    ENGINE -- "poll_blue_evidence / poll_blue_ground_truth (data collection, no UI)" --> STORE
```

## Storage

PostgreSQL/PostGIS separates raw ingested data, staging, fire event
records, validation and quality tables, ML dataset/model registries, and
operational tables. Migrations under [`migrations/`](../migrations) are
additive SQLx migrations applied by the `engine` binary at startup;
historical migrations are treated as immutable once applied (see
[`CONTRIBUTING.md`](../CONTRIBUTING.md)).

## API surfaces

Detailed routes: [`docs/api.md`](api.md).

The API is now a single, always-on, unauthenticated read-only surface —
`/health`, `/config`, `/risk`, `/risk/cell/{h3}`, `/alerts`, `/sources`,
`/stream`. No login, session, token, or API key exists anywhere in this
API. There is no route serving an HTML page; `/` returns `404` like any
other undefined path.

**BLUE's data collection keeps running even though its interface is
gone.** `BLUE_CENTER_ENABLED` (`crates/engine/src/config.rs`) still gates
the scheduler's background tasks — `poll_blue_evidence` and the daily
bulletin capture inside `poll_forecast` — which continue to archive
`+24h`/`+48h` forecast snapshots and ground-truth evidence into the
`blue.*` database schema (`crates/store/src/blue.rs`). This is data
collection, not a web interface, and was deliberately left untouched:
the immutable archive it builds is the whole point of a future honest
prospective-validation surface, and stopping it now would have thrown
away real, currently-irreplaceable data (a paused archive can't be
retroactively backfilled). There is simply no HTTP route left to read it
through until a new interface is built.

## Deployment shape

FireSift runs as a single container (see [`Dockerfile`](../Dockerfile))
against a private PostgreSQL/PostGIS instance, with a reverse proxy
(Caddy) as the only public entry point. See
[`docs/deployment.md`](deployment.md) for a generic deployment guide.
