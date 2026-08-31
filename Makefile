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

CHROME := /Applications/Google Chrome.app/Contents/MacOS/Google Chrome

icons: ## Regenerate all app icons from assets/icon.svg (transparent bg)
	"$(CHROME)" --headless --disable-gpu --screenshot=/tmp/inky-icon.png \
		--window-size=1024,1024 --default-background-color=00000000 \
		--hide-scrollbars "file://$(PWD)/assets/icon.svg" 2>/dev/null
	pnpm tauri icon /tmp/inky-icon.png
	rm -f /tmp/inky-icon.png

open: ## Open the last built release app
	open $(BUNDLE_DIR)/macos/Inky.app

mcp: ## Run the MCP server on stdio (for manual testing)
	node mcp/server.mjs

clean: ## Remove frontend and bundle build outputs
	rm -rf build dist $(BUNDLE_DIR)
