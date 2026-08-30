# Security Policy

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability**, and do not
include secrets, credentials, or working exploit details in a public issue
or pull request.

Preferred channel, once this repository is hosted on GitHub: use
[GitHub's private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing/privately-reporting-a-security-vulnerability)
(the "Report a vulnerability" button under the repository's Security tab),
which needs to be enabled by the maintainer before publication.

If private reporting is not yet enabled at the time you find an issue, a
dedicated private reporting channel (e.g. a security contact address) must
be established before this repository goes public — until then, contact
the maintainer through whatever private channel you already have, and do
not post details anywhere public.

When reporting, please include:

- A description of the vulnerability and its potential impact.
- Steps to reproduce, or a minimal proof of concept.
- The affected component (crate, endpoint, deployment script).
- Whether you believe it affects the live production deployment, the
  codebase generally, or the reference deployment scripts under
  `deploy/oracle/`.

## Scope

In scope:

- The Rust codebase under `crates/`.
- The HTTP API surfaces (`crates/api`), including the science and
  territorial consoles.
- Database migrations and the deployment scripts under `deploy/`.
- The Docker build (`Dockerfile`, `docker-compose.yml`).

Out of scope / please don't:

- Attempting to access, disrupt, or degrade any live production
  deployment. If you believe you've found something affecting a real
  deployment, report it — do not test it against that deployment.
- Automated scanning that generates significant load.
- Social engineering against maintainers or contributors.

## Sensitive areas

- **The read-only operational API** (`/health`, `/config`, `/risk`,
  `/risk/cell/{h3}`, `/alerts`, `/sources`, `/stream`) is intended to be
  read-only. If you find a way to write, mutate state, trigger an
  import, retrain, or activate a model through it, that is a security bug
  — report it privately. No bundled web interface exists as of
  2026-08-30 (see [`ROADMAP.md`](ROADMAP.md)); this applies to whatever
  interface layer is rebuilt going forward too.
- **`.env.example` and `.env.production.example`** must never contain real
  credentials. If you spot one that does, report it privately and do not
  quote the value in any public channel.
- **Migrations** (`migrations/`) are meant to be applied once and never
  edited retroactively — a PR that edits an already-applied migration
  file, rather than adding a new one, is a correctness and safety issue on
  its own, independent of whether it's exploitable.

## Supported versions

This project is pre-1.0 (`v0.4.x`) and does not yet maintain multiple
supported release branches. Security fixes land on `main` and are called
out in [`CHANGELOG.md`](CHANGELOG.md) under a `Security` heading. Once the
project reaches a stable 1.0 line, this section will be updated with a
real support matrix.

## Historical infrastructure references

Some documents under `docs/research/` describe a specific past private VPS
deployment. Identifying details (public IP, hostname) have been redacted
to placeholders as part of preparing this repository for public release —
see [`OPEN_SOURCE_READINESS_REPORT.md`](OPEN_SOURCE_READINESS_REPORT.md).
If you find any remaining real infrastructure identifier, real credential,
or other sensitive value anywhere in this repository or its Git history,
please report it privately rather than opening a public issue, per the
process above.
