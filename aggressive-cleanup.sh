#!/bin/bash
# Manual cleanup script for cache/toolchain directories

targets=(
    "/Users/sac/.colima"          # Docker images: 24GB
    "/Users/sac/.gemini"          # Unknown cache: 14GB
    "/Users/sac/.rustup"          # Rust toolchains: 10GB
    "/Users/sac/.cache"           # General cache: 10GB
    "/Users/sac/.cargo/registry"  # Crate registry: ~3GB of 5.9GB
    "/Users/sac/.m2/repository"   # Maven cache: 3.4GB
    "/Users/sac/.codeium"         # Codeium cache: 3.2GB
    "/Users/sac/.asdf/installs"   # Version manager: ~2GB of 3GB
    "/Users/sac/.npm"             # NPM cache: 859MB
    "/Users/sac/.pnpm-store"      # PNPM store: 1GB
    "/Users/sac/.duckdb"          # DuckDB cache: 813MB
    "/Users/sac/.traycer"         # Unknown: 883MB
)

total=0
for target in "${targets[@]}"; do
    if [ -d "$target" ]; then
        size=$(du -sh "$target" 2>/dev/null | cut -f1)
        bytes=$(du -sb "$target" 2>/dev/null | cut -f1)
        echo "Removable: $size | $target"
        total=$((total + bytes))
    fi
done

echo "---"
printf "Total recoverable: %.2f GB\n" $((total / 1000000000))
