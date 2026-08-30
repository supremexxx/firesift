# syntax=docker/dockerfile:1.7

FROM rust:1.98-bookworm AS builder

# Per-architecture cache scope: without it, concurrent `docker buildx build
# --platform linux/amd64,linux/arm64` runs share one cache mount and race
# writing into the same cargo registry directory (`File exists (os error
# 17)` unpacking a crate). TARGETARCH ("amd64"/"arm64") is a standard
# BuildKit-provided build arg; it just needs declaring to use it here.
ARG TARGETARCH

ARG FIRESIFT_GIT_COMMIT=unknown
ENV FIRESIFT_GIT_COMMIT=${FIRESIFT_GIT_COMMIT}

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations

RUN --mount=type=cache,id=cargo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=cargo-target-${TARGETARCH},target=/src/target \
    cargo build --locked --release -p engine && \
    cp /src/target/release/pyrorisk /tmp/pyrorisk

FROM node:22-bookworm-slim AS web-builder

WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM debian:bookworm-slim AS runtime

ARG OCI_REVISION=unknown
ARG OCI_CREATED=unknown
ARG OCI_TITLE=firesift
ARG FIRESIFT_PHASE=unknown

LABEL org.opencontainers.image.revision="${OCI_REVISION}" \
      org.opencontainers.image.created="${OCI_CREATED}" \
      org.opencontainers.image.title="${OCI_TITLE}" \
      firesift.phase="${FIRESIFT_PHASE}"

RUN apt-get update && \
    apt-get install --yes --no-install-recommends ca-certificates curl gdal-bin libeccodes-tools && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --create-home --uid 10001 --shell /usr/sbin/nologin pyrorisk && \
    mkdir -p /app/out /data && \
    chown -R pyrorisk:pyrorisk /app /data

WORKDIR /app
COPY --from=builder /tmp/pyrorisk /usr/local/bin/pyrorisk
COPY --chown=pyrorisk:pyrorisk testdata ./testdata
COPY --from=web-builder --chown=pyrorisk:pyrorisk /web/dist ./web/dist

USER pyrorisk

ENV API_BIND=0.0.0.0:8080 \
    WEB_ASSETS_DIR=/app/web/dist \
    RUST_LOG=info,pyrorisk=info

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:8080/health >/dev/null || exit 1

ENTRYPOINT ["pyrorisk"]
CMD ["run"]
