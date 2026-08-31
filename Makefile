SHELL := /bin/bash
KEY ?= $(HOME)/.tauri/inky.key
BUNDLE_DIR := src-tauri/target/release/bundle

.DEFAULT_GOAL := help
.PHONY: help dev check dmg release icons open mcp clean

help: ## Show available targets
	@echo "Inky — available targets:" && echo
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

dev: ## Run the app in development mode (hot reload)
	pnpm tauri dev

check: ## Type-check the frontend and the Rust backend
	pnpm check
	cd src-tauri && cargo check

dmg: ## Build the signed .app, .dmg and updater artifacts
	TAURI_SIGNING_PRIVATE_KEY=$(KEY) TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" pnpm tauri build
	@echo && echo "Artifacts:" && ls -1 $(BUNDLE_DIR)/dmg/*.dmg $(BUNDLE_DIR)/macos/Inky.app.tar.gz*

release: dmg ## Build and assemble a GitHub-release folder in dist/release
	node scripts/release.mjs

icons: ## Regenerate all app icons from assets/icon.svg
	qlmanage -t -s 1024 -o /tmp assets/icon.svg >/dev/null
	pnpm tauri icon /tmp/icon.svg.png
	rm -f /tmp/icon.svg.png

open: ## Open the last built release app
	open $(BUNDLE_DIR)/macos/Inky.app

mcp: ## Run the MCP server on stdio (for manual testing)
	node mcp/server.mjs

clean: ## Remove frontend and bundle build outputs
	rm -rf build dist $(BUNDLE_DIR)
