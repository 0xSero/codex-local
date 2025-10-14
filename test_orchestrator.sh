#!/usr/bin/env bash

echo "=== Testing orchestrator configuration fix ==="
echo ""

# First, let's check what config the binary is reading
echo "1. Checking if debug binary can read config:"
cd /Users/sero/projects/codex-local/codex-rs
timeout 5 ./target/debug/codex-local --help >/dev/null 2>&1
if [ $? -eq 0 ]; then
    echo "✅ Debug binary runs"
else
    echo "❌ Debug binary failed to run"
fi

echo ""
echo "2. Config file locations:"
echo "   ~/.codex/config.toml exists: $([ -f ~/.codex/config.toml ] && echo 'YES' || echo 'NO')"
echo "   ~/.codex-local/config.toml exists: $([ -f ~/.codex-local/config.toml ] && echo 'YES' || echo 'NO')"

echo ""
echo "3. Orchestrator config in ~/.codex-local/config.toml:"
if [ -f ~/.codex-local/config.toml ]; then
    grep -n "orchestrator_profile\|agent_profiles" ~/.codex-local/config.toml | head -5
fi

echo ""
echo "4. Checking if the old config has orchestrator settings:"
if [ -f ~/.codex/config.toml ]; then
    if grep -q "orchestrator_profile\|agent_profiles" ~/.codex/config.toml; then
        echo "✅ Old config has orchestrator settings"
    else
        echo "❌ Old config missing orchestrator settings"
    fi
else
    echo "❌ Old config file doesn't exist"
fi