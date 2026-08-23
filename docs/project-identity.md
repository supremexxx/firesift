# Project identity: naming and versions

FireSift's Git history spans three names. This document explains where
each one still appears, why, and what should (and should not) be renamed
without a migration plan.

## Naming

### FireSift — current public name

The name used in the README, documentation, GitHub repository
(`supremexxx/firesift`), and public-facing UI copy (dashboard, Watch,
BLUE). Use this name in anything user- or contributor-facing.

### Erytheon — historical name, persists as scientific/runtime identifiers

Erytheon was the project's name before the FireSift rename. Its GHCR
image name (`ghcr.io/supremexxx/erytheon`) was dual-published alongside
`ghcr.io/supremexxx/firesift` during the rename transition and has since
been dropped — `.github/workflows/container.yml` now publishes only
`ghcr.io/supremexxx/firesift`. Erytheon otherwise persists
**internal/runtime**:

- Environment variables actually read at runtime by
  `crates/engine/src/scheduler.rs` for deployment provenance:
  `ERYTHEON_ENVIRONMENT`, `ERYTHEON_GIT_REVISION`,
  `ERYTHEON_IMAGE_REFERENCE`, `ERYTHEON_IMAGE_DIGEST`,
  `ERYTHEON_APPLICATION_REVISION`, `ERYTHEON_APPLICATION_IMAGE`,
  `ERYTHEON_CADDY_STATE`. These are consumed code, not dead names — a
  deployment's `deploy/oracle/.env` sets them, and renaming the variable
  names would break existing deployment automation without a migration.
- The Docker image label prefix (`org.opencontainers.image.*` values
  populated from `OCI_*` build args) and some Oracle deployment doc
  prose still say "Erytheon" descriptively.
- Historical Git branches, commit messages, and the `docs/research/`
  archive predate the rename and use "Erytheon" throughout — this is a
  correct historical record and is not meant to be rewritten.

**Do not** do a blind global find-and-replace of `Erytheon` → `FireSift`.
The `ERYTHEON_*` environment variable names in particular are a live
contract between `deploy/oracle/.env` files already in use and
`crates/engine`'s scheduler — renaming them requires updating every
deployment's `.env` in lockstep, which is a deployment-coordination task,
not a documentation edit.

### PyroRisk — historical name, persists as binary/infrastructure conventions

PyroRisk predates Erytheon and is the oldest name still visibly present:

- The compiled binary name: `[[bin]] name = "pyrorisk"`
  (`crates/engine/Cargo.toml`).
- The default database name/user in `docker-compose.yml`,
  `deploy/oracle/.env.example`, and `crates/engine/src/config.rs`'s
  `DEFAULT_DATABASE_URL` (`postgres://pyrorisk:pyrorisk@...`).
- The deployment system user and remote directory convention
  (`/opt/pyrorisk`), referenced throughout `deploy/oracle/*.sh` and
  `deploy/oracle/README.md`.

**Do not** rename the binary, the default DB user/database name, or the
`/opt/pyrorisk` deployment path without a migration plan: existing
production deployments authenticate against a `pyrorisk` role, run a
`pyrorisk`-owned systemd timer and directory tree, and invoke a
`pyrorisk` binary. Renaming any of these is an infrastructure change
with a real rollback cost, not a text substitution.

## Version policy

Six different things are versioned independently in this project. They
are **not** kept in sync, and that is a known, current state — not an
oversight this document is trying to hide.

| What | Current value | Where | Notes |
|---|---|---|---|
| Cargo workspace version | `0.1.0` | `Cargo.toml` (`[workspace.package] version`) | Has not been bumped since the project's early phases. Not changed by this consolidation — see below. |
| Git tags / GitHub releases | `v0.5.0` (latest) | `git tag`, GitHub Releases | The user-facing release version. `v0.4.2`/`v0.4.2-app` and later tags must never be moved (see `ROADMAP.md`). |
| Application/runtime version | Cargo's `CARGO_PKG_VERSION` (`0.1.0`), embedded via `env!()` into scientific pipeline records (`CODE_VERSION` in `crates/engine/src/{dataset_pipeline,snapshot_pipeline,model_experiments,candidate_pipeline,quality_pipeline}.rs`) | Compiled in | Because Cargo's version is `0.1.0`, every one of these internal records currently says `0.1.0`, not `0.5.0` — they track the Cargo version, not the release tag. |
| Model versions | e.g. `v1` (active), `gbm_isotonic_v2` (candidate, inactive) | `ml.human_model_versions`, `ml.model_candidate_registry` | Independent of the application version; see `docs/models.md`. |
| Dataset versions | `ml.dataset_versions`, checksum-identified | Database registry | Independent of the application version; see `docs/research/reports/` for dataset construction reports. |
| Migration numbers | `0001` through `0032` (as of this writing) | `migrations/*.sql` | Sequential, immutable once applied; not a semantic version — see `CONTRIBUTING.md`. |

**The Cargo workspace version was intentionally left at `0.1.0` during
this consolidation.** Bumping it (and deciding whether it should track
the Git tag, and what that implies for `CARGO_PKG_VERSION`-derived
`CODE_VERSION` values already written into the database by past
pipeline runs) is a separate decision with its own consequences,
out of scope here. Treat it as open future work, not a bug fixed by this
document.
