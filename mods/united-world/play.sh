#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
python3 mods/united-world/build.py --if-needed
display_args=(--resolution 1920x1080)
for argument in "$@"; do
    case "$argument" in
        --resolution|--resolution=*|--capture|--capture=*) display_args=() ;;
    esac
done
cargo run --profile release-fast --bin resonance -- --mod mods/united-world/generated --test-united-world "${display_args[@]}" "$@"
