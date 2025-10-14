#!/usr/bin/env bash

echo "=== Testing Configuration Parsing ==="
echo "Config file: /Users/sero/.codex-local/config.toml"
echo ""

# Extract the relevant lines from config
echo "Raw config values:"
grep -n "orchestrator_profile\|agent_profiles\|profile.*=" /Users/sero/.codex-local/config.toml | head -10
echo ""

echo "Available profiles:"
grep -n "\[profiles\." /Users/sero/.codex-local/config.toml
echo ""

# Test with the debug binary
echo "Testing with debug binary..."
cd /Users/sero/projects/codex-local/codex-rs
./target/debug/codex-local --help 2>/dev/null | head -1
echo "Debug binary exists: $?"