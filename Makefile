.PHONY: help dev stop dev-frontend dev-backend kill-port migrate migrate-revert db-reset test lint fmt check build docker-up docker-down docker-searxng tool-builder sqlx-prepare

# Dev server ports (override: make dev-backend BACKEND_PORT=3001)
BACKEND_PORT ?= 3000
FRONTEND_PORT ?= 5173

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

# ---------- Dev ----------

dev: ## Run backend (:3000) + frontend (:5173) concurrently
	$(MAKE) -j2 dev-backend dev-frontend

dev-backend: ## Run backend (stops anything already on :$(BACKEND_PORT) first)
	$(MAKE) kill-port PORT=$(BACKEND_PORT)
	cd backend && APP_MODE=local cargo run

dev-frontend: ## Run Vite dev server (stops anything already on :$(FRONTEND_PORT) first)
	$(MAKE) kill-port PORT=$(FRONTEND_PORT)
	cd frontend && pnpm dev

stop: ## Stop dev servers (backend + frontend ports)
	$(MAKE) kill-port PORT=$(BACKEND_PORT)
	$(MAKE) kill-port PORT=$(FRONTEND_PORT)

kill-port: ## Kill whatever listens on PORT (used before starting dev servers)
	@PIDS=$$(lsof -nP -t -iTCP:$(PORT) -sTCP:LISTEN 2>/dev/null); \
	if [ -n "$$PIDS" ]; then \
		echo "Stopping processes on port $(PORT): $$(echo $$PIDS | tr '\n' ' ')"; \
		kill $$PIDS 2>/dev/null || true; \
		sleep 1; \
		REMAINING=$$(lsof -nP -t -iTCP:$(PORT) -sTCP:LISTEN 2>/dev/null); \
		if [ -n "$$REMAINING" ]; then \
			echo "Force killing on port $(PORT): $$REMAINING"; \
			kill -9 $$REMAINING 2>/dev/null || true; \
		fi; \
	else \
		echo "Port $(PORT) is free"; \
	fi

# ---------- Database ----------

migrate: ## Run sqlx migrations (requires DATABASE_URL or .env)
	cd backend && sqlx migrate run

migrate-revert: ## Revert last migration (dev only)
	cd backend && sqlx migrate revert

db-reset: ## Drop + recreate the dev database, then run the baseline migration
	cd backend && sqlx database drop -y && sqlx database create && sqlx migrate run

# ---------- Quality ----------

test: ## Run backend tests
	cd backend && cargo test

lint: ## clippy + fmt check (backend)
	cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt --check

fmt: ## Format backend
	cd backend && cargo fmt

check-frontend: ## Typecheck frontend
	cd frontend && pnpm typecheck

# ---------- Build ----------

build: ## Production build backend + frontend
	cd backend && cargo build --release
	cd frontend && pnpm build

sqlx-prepare: ## Generate offline query data (needs live DB)
	cd backend && cargo sqlx prepare

# ---------- Docker ----------

docker-up: ## Start postgres + app (production mode, APP_MODE=server)
	docker compose -f deploy/docker-compose.yml up --build -d

docker-down: ## Stop compose stack
	docker compose -f deploy/docker-compose.yml down

docker-postgres: ## Start only postgres for local dev
	docker compose -f deploy/docker-compose.yml up -d postgres

docker-searxng: ## Start only SearXNG for local dev (web_search → :8080)
	docker compose -f deploy/docker-compose.yml up -d searxng

tool-builder: ## Build the sandboxed wasm tool builder image
	docker build -t assistant-tool-builder -f deploy/tool-builder/Dockerfile deploy/tool-builder
