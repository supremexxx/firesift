# FireSift

FireSift is an experimental open-source platform for modelling and mapping
**wildfire ignition risk** from weather, satellite observations, territorial
features, and historical fire records. It combines a deterministic fire
weather index, a learned human-ignition component, and an H3 grid to
produce a relative risk score — not a forecast guarantee, and not an
official alert.

[![CI](https://github.com/supremexxx/firesift/actions/workflows/ci.yml/badge.svg)](https://github.com/supremexxx/firesift/actions/workflows/ci.yml)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.97.1-orange)

## What FireSift is

- A Rust workspace that ingests weather, satellite, and territorial data,
  computes the Canadian Fire Weather Index, and fuses it with a learned
  human-ignition-propensity score over an H3 hex grid.
- A research codebase with a documented, versioned scientific foundation:
  labeled datasets, a negative-sampling design, a trained candidate model,
  and a paired historical comparison against the operational model.
- A platform meant to be inspected, reproduced, and extended — the
  scientific console (`/science`) exposes dataset, pipeline, and model
  registry state as structured, read-only data, not just a dashboard.

## What FireSift is not

- **Not an official wildfire warning or civil-security alert.** Always
  follow guidance from competent authorities.
- **Not a validated operational forecasting product.** It has not
  undergone the kind of real-world, prospective validation that would
  justify presenting it as professionally reliable — see
  [Scientific status](#scientific-status) below.
- **Not a probability.** The risk score is *relative* — it ranks
  conditions against each other, it does not state "there is an X% chance
  of a fire here."
- **Not a commercial product.** FireSift was previously explored as a
  potential product for municipalities, insurers, and public institutions.
  That direction is on hold: the project does not yet have enough
  real-world validation to be presented honestly as a professional tool.
  It is repositioned here as an open research platform first.

## Features

- H3 hexagonal risk surfaces at `nowcast`, `+6h`, `+24h`, and `+48h`
  horizons.
- Canadian Fire Weather Index (FFMC, DMC, DC, ISI, BUI, FWI) computed in
  pure Rust, with a documented reference-fixture validation.
- Multi-source ingestion: NASA FIRMS satellite detections, Météo-France
  observations, ECMWF IFS Open Data forecasts (credential-free) with an
  Open-Meteo fallback, BDIFF and Prométhée historical fire records,
  OpenStreetMap / CORINE Land Cover / INSEE / territorial calendars for
  static features.
- Explainable scoring: every risk cell can be broken down into its
  contributing factors via `GET /risk/cell/{h3}`.
- A read-only scientific console exposing dataset versions, pipeline and
  data-quality status, model registry state, and system integrity.
- An experimental, non-active `gbm_isotonic_v2` candidate model, trained
  and evaluated but never served — see [Scientific status](#scientific-status).

## Interfaces

Five HTTP surfaces, one always on and four behind a deployment flag that
is **not** authentication — see
[`docs/architecture.md#api-surfaces`](docs/architecture.md#api-surfaces)
for the full picture, including expected reverse-proxy protection:

| Interface | Flag | Status |
|---|---|---|
| Operational dashboard | always on | Public |
| Scientific console (`/science`) | `SCIENCE_CONSOLE_ENABLED` | Experimental, disabled by default |
| BLUE forecast-evidence center (`/blue`) | `BLUE_CENTER_ENABLED` | Experimental, partial foundation — see [Scientific status](#scientific-status) |
| Watch public map (`/watch`) | `WATCH_CONSOLE_ENABLED` | Experimental, present in `Unreleased`, disabled by default |

## Architecture

Nine crates in one Cargo workspace (`engine`, `api`, `store`, `ingest`,
`dataset`, `quality`, `risk`, `fwi`, `grid`) over PostgreSQL/PostGIS. Full
diagram and crate responsibilities: [`docs/architecture.md`](docs/architecture.md).

## Quick start

Requirements: Rust `1.97.1` (pinned by [`rust-toolchain.toml`](rust-toolchain.toml)),
Docker with Compose, `curl`, and (only when running the engine directly on
the host, as below, rather than inside the container) `gdal` and `eccodes`
for decoding ECMWF GRIB2 forecasts — `sudo apt-get install gdal-bin
libeccodes-tools` on Debian/Ubuntu, `brew install gdal eccodes` on macOS.
Without them the service still starts and `/health`/`/risk`/FIRMS ingestion
work, but weather/forecast ingestion silently fails over to an empty
result — see [`docs/deployment.md`](docs/deployment.md) if you hit this.

```sh
git clone https://github.com/supremexxx/firesift.git
cd firesift
cp .env.example .env
docker compose up -d
cargo run -p engine -- run
```

If `docker compose up -d` fails to bind port 5432 because something else on
your machine is already listening there, either stop that service or point
`DATABASE_URL` in `.env` at a different host port mapped in
`docker-compose.yml`.

The service then answers on:

- `GET http://localhost:8080/health` — service and data-source health;
- `GET http://localhost:8080/` — operational dashboard;
- `GET /risk` — GeoJSON H3 risk surfaces;
- `GET /alerts` — cells above a configured threshold;
- `GET /risk/cell/{h3}` — explained score for one cell;
- `WS /stream` — live risk updates.

The default profile (`DATA_PROFILE=fixture`) runs entirely on the small,
versioned fixtures under `testdata/` — no API keys or real datasets
required. `DATA_PROFILE=production` refuses fixtures, missing static
layers, and silent ingestion failures; see [`docs/deployment.md`](docs/deployment.md).

### Local demo: scientific console

```sh
docker compose up -d
cargo run -p engine -- preview-science-console --bind 127.0.0.1:8081
```

Then open <http://127.0.0.1:8081/science>. This preview mode runs without
the scheduler and without loading a model. In a normal deployment the
console is mounted at `/science` only when `SCIENCE_CONSOLE_ENABLED=true`
(default `false`) — see [`docs/api.md`](docs/api.md).

## API

Summary and stability tiers: [`docs/api.md`](docs/api.md). Every route in
the codebase is a `GET` or `WS` read — there is no write, import,
training, migration, or model-activation endpoint.

## Models

| Model | Status | Served |
|---|---|---|
| v1 (logistic regression + FWI fusion) | **Active** | Yes — the only model served |
| `gbm_isotonic_v2` (candidate) | **Inactive** | No |
| Shadow scoring | Not implemented | N/A |

Full detail, including the historical comparison metrics and why the
candidate remains inactive: [`docs/models.md`](docs/models.md).

## Scientific status

- **v1 is the sole model serving risk scores.** It has not been modified
  by any of the scientific-foundation work in this repository.
- **The `gbm_isotonic_v2` candidate is registered `inactive`.** It has
  been trained, calibrated, and compared against v1 on historical data,
  but never activated, never served, and never shadow-scored against live
  data. Promotion is a separate, explicit, documented decision — never an
  automatic consequence of a good historical metric. See
  [`GOVERNANCE.md`](GOVERNANCE.md).
- **Shadow scoring has not started.** Its design exists
  ([`docs/research/reports/SHADOW_SCORING_DESIGN.md`](docs/research/reports/SHADOW_SCORING_DESIGN.md))
  but nothing runs it yet.
- **BLUE is a partial prospective-validation foundation, not a complete
  system.** It archives immutable `+24h`/`+48h` forecast evidence and
  supports terrain/community-reported confirmations, but reverse
  matching for recall/specificity and a published aggregate track record
  do not exist yet — see
  [`docs/architecture.md#blue-forecast-evidence-center`](docs/architecture.md#blue-forecast-evidence-center).

## Limitations

- The score is a **relative** risk indicator, not an absolute probability.
- Historical validation is not the same as live operational validation.
- Some static territorial features (WUI, roads, population, land cover)
  are applied across multiple years without a per-year historical
  snapshot — a known, documented temporal approximation.
- Training datasets use negative sampling; reported class ratios reflect a
  sampling design choice, not real-world fire prevalence.
- NASA FIRMS reports observed thermal anomalies, not predictions.
- Prospective (forecast-vs-observed) validation is only partially
  implemented (BLUE); no published aggregate track record exists yet.

Full detail: [`docs/scientific-limitations.md`](docs/scientific-limitations.md).

## Data sources

NASA FIRMS, Météo-France, ECMWF IFS Open Data (FireSift's primary,
credential-free forecast source), BDIFF, Prométhée, OpenStreetMap, CORINE
Land Cover, and INSEE. Each has its own license, attribution requirements,
and redistribution restrictions — **the code license does not cover the
data**. See [`docs/data-sources.md`](docs/data-sources.md) for the full
per-source breakdown and [`NOTICE.md`](NOTICE.md) for a quick attribution
reference. Real datasets are not committed to this repository (only small
development fixtures under `testdata/`).

FireSift can optionally use **Open-Meteo** as a fallback provider when
ECMWF/Météo-France are unavailable. Its weather data is CC-BY-4.0, but its
free API service is contractually limited to non-commercial use — users
are responsible for complying with the terms applicable to their own
usage and plan; see [`docs/data-sources.md#open-meteo`](docs/data-sources.md#open-meteo).

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — system design
- [`docs/scientific-methodology.md`](docs/scientific-methodology.md) — what is modelled and how
- [`docs/scientific-limitations.md`](docs/scientific-limitations.md) — honest limits
- [`docs/models.md`](docs/models.md) — v1 and candidate v2 detail
- [`docs/data-sources.md`](docs/data-sources.md) — licensing and attribution per source
- [`docs/reproducibility.md`](docs/reproducibility.md) — reproducing the service and reported metrics
- [`docs/deployment.md`](docs/deployment.md) — generic production deployment guide
- [`docs/api.md`](docs/api.md) — endpoint reference and stability tiers
- [`docs/project-identity.md`](docs/project-identity.md) — the FireSift/Erytheon/PyroRisk naming, and how Cargo/tag/model/dataset versions relate
- [`docs/public-platform.md`](docs/public-platform.md) — vision for a future public site (not implemented)
- [`docs/research/`](docs/research/) — the full phase-by-phase engineering and research archive
- [`ROADMAP.md`](ROADMAP.md) — what's done and what's next
- [`OPEN_SOURCE_READINESS_REPORT.md`](OPEN_SOURCE_READINESS_REPORT.md) — audit of this repository's readiness for public release

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Any change touching models,
labels, datasets, features, scoring, sampling, or calibration needs a
scientific justification and tests, not just a passing benchmark — see
[`GOVERNANCE.md`](GOVERNANCE.md).

## Security

See [`SECURITY.md`](SECURITY.md) for how to report a vulnerability. Please
do not open a public issue containing a secret or a live exploit.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
```

Integration tests require PostgreSQL/PostGIS (see
[`docker-compose.yml`](docker-compose.yml)). Versioned fixtures live under
`testdata/`; real data, build output, and secrets must never be committed
(see [`.gitignore`](.gitignore)).

## License

Code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option — see [`COPYRIGHT`](COPYRIGHT). Data sources are licensed
separately; see [`docs/data-sources.md`](docs/data-sources.md).

---

*FireSift is an experimental research project. Its outputs are not
official wildfire warnings, emergency alerts, or guarantees that a fire
will or will not occur. Always follow information and instructions issued
by competent authorities.*
