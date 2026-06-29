.PHONY: all build-rust build-go clean install cross-build

NAME    := terraform-provider-ytmusic
VERSION := 0.1.0
OS      := $(shell uname -s | tr A-Z a-z)
ARCH    := $(shell uname -m | sed 's/x86_64/amd64/')
PLUGIN_DIR := ~/.terraform.d/plugins/registry.terraform.io/caos-obliquo/ytmusic/$(VERSION)

all: build-dir build-rust build-go

build-dir:
	mkdir -p build/native

# Build Rust sidecar (native)
build-rust:
	cd ytmusic-cli && cargo build --release
	cp ytmusic-cli/target/release/ytmusic-cli build/native/

# Build Go provider (native)
build-go:
	go build -o build/native/$(NAME)

# ── Cross-compilation (Terraform registry) ────────────────────────────
# Targets: linux_amd64, linux_arm64, darwin_amd64, darwin_arm64
# Prerequisites:
#   Rust: rustup target add {target} for each
#   macOS cross: requires osxcross or build on macOS natively

CROSS_TARGETS := linux_amd64 linux_arm64 darwin_amd64 darwin_arm64

cross-build: $(CROSS_TARGETS)

linux_amd64:
	GOOS=linux GOARCH=amd64 go build -o build/linux_amd64/$(NAME)
	cd ytmusic-cli && cargo build --release --target x86_64-unknown-linux-gnu
	cp ytmusic-cli/target/x86_64-unknown-linux-gnu/release/ytmusic-cli build/linux_amd64/

linux_arm64:
	GOOS=linux GOARCH=arm64 go build -o build/linux_arm64/$(NAME)
	cd ytmusic-cli && cargo build --release --target aarch64-unknown-linux-gnu
	cp ytmusic-cli/target/aarch64-unknown-linux-gnu/release/ytmusic-cli build/linux_arm64/

darwin_amd64:
	GOOS=darwin GOARCH=amd64 go build -o build/darwin_amd64/$(NAME)
	cd ytmusic-cli && cargo build --release --target x86_64-apple-darwin
	cp ytmusic-cli/target/x86_64-apple-darwin/release/ytmusic-cli build/darwin_amd64/

darwin_arm64:
	GOOS=darwin GOARCH=arm64 go build -o build/darwin_arm64/$(NAME)
	cd ytmusic-cli && cargo build --release --target aarch64-apple-darwin
	cp ytmusic-cli/target/aarch64-apple-darwin/release/ytmusic-cli build/darwin_arm64/

# Install all cross-compiled builds into plugin directory
cross-install: cross-build
	mkdir -p $(PLUGIN_DIR)/linux_amd64 $(PLUGIN_DIR)/linux_arm64
	mkdir -p $(PLUGIN_DIR)/darwin_amd64 $(PLUGIN_DIR)/darwin_arm64
	cp build/linux_amd64/* $(PLUGIN_DIR)/linux_amd64/
	cp build/linux_arm64/* $(PLUGIN_DIR)/linux_arm64/
	cp build/darwin_amd64/* $(PLUGIN_DIR)/darwin_amd64/
	cp build/darwin_arm64/* $(PLUGIN_DIR)/darwin_arm64/

# ── Local install ─────────────────────────────────────────────────────

install: all
	mkdir -p $(PLUGIN_DIR)/$(OS)_$(ARCH)/
	cp build/native/$(NAME) $(PLUGIN_DIR)/$(OS)_$(ARCH)/
	cp build/native/ytmusic-cli $(PLUGIN_DIR)/$(OS)_$(ARCH)/

# ── Clean ─────────────────────────────────────────────────────────────

clean:
	rm -rf build/
	cd ytmusic-cli && cargo clean
