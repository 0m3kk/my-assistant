# ---------- Frontend build ----------
FROM node:26-alpine AS frontend-build
WORKDIR /app/frontend
# corepack no longer ships with Node >= 25 — install pnpm explicitly.
RUN npm install -g pnpm@12.16.1
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm run build

# ---------- Backend build (cargo-chef for layer caching) ----------
FROM lukemathwalker/cargo-chef:latest-rust-1.99 AS chef
WORKDIR /app/backend

FROM chef AS planner
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS backend-build
COPY --from=planner /app/backend/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY backend/ ./
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin assistant && mv target/release/assistant /usr/local/bin/assistant

# ---------- Runtime ----------
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 appuser
COPY --from=backend-build /usr/local/bin/assistant /usr/local/bin/assistant
COPY --from=frontend-build /app/frontend/dist /app/dist
USER appuser
WORKDIR /app
ENV STATIC_DIR=/app/dist
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://localhost:3000/api/health || exit 1
ENTRYPOINT ["/usr/local/bin/assistant"]
