#!/usr/bin/env bash

echo "=== Simple test to verify our changes ==="
echo ""

# Check home directory
HOME_DIR=$(echo "$HOME")
EXPECTED_CONFIG_DIR="$HOME_DIR/.codex-local"
echo "Home directory: $HOME_DIR"
echo "Expected config directory: $EXPECTED_CONFIG_DIR"

echo ""
echo "Checking if config directory exists:"
if [ -d "$EXPECTED_CONFIG_DIR" ]; then
    echo "✅ $EXPECTED_CONFIG_DIR exists"
else
    echo "❌ $EXPECTED_CONFIG_DIR does not exist"
fi

echo ""
echo "Checking if config file exists:"
CONFIG_FILE="$EXPECTED_CONFIG_DIR/config.toml"
if [ -f "$CONFIG_FILE" ]; then
    echo "✅ $CONFIG_FILE exists"
    echo ""
    echo "Checking orchestrator config in $CONFIG_FILE:"
    if grep -q "orchestrator_profile.*=" "$CONFIG_FILE"; then
        ORCH_PROFILE=$(grep "orchestrator_profile.*=" "$CONFIG_FILE" | cut -d'=' -f2 | tr -d ' "')
        echo "✅ orchestrator_profile = $ORCH_PROFILE"
    else
        echo "❌ orchestrator_profile not found"
    fi

    if grep -q "agent_profiles.*=" "$CONFIG_FILE"; then
        AGENT_PROFILES=$(grep "agent_profiles.*=" "$CONFIG_FILE" | cut -d'=' -f2 | tr -d ' "[]')
        echo "✅ agent_profiles = [$AGENT_PROFILES]"
    else
        echo "❌ agent_profiles not found"
    fi

    echo ""
    echo "Checking if required profiles exist:"
    if grep -q "\[profiles\.orchestrator\]" "$CONFIG_FILE"; then
        echo "✅ [profiles.orchestrator] found"
    else
        echo "❌ [profiles.orchestrator] not found"
    fi

    if grep -q "\[profiles\.worker\]" "$CONFIG_FILE"; then
        echo "✅ [profiles.worker] found"
    else
        echo "❌ [profiles.worker] not found"
    fi

    if grep -q "\[profiles\.glm-fast\]" "$CONFIG_FILE"; then
        echo "✅ [profiles.glm-fast] found"
    else
        echo "❌ [profiles.glm-fast] not found"
    fi

else
    echo "❌ $CONFIG_FILE does not exist"
fi

echo ""
echo "=== Checking if old config still exists ==="
OLD_CONFIG_DIR="$HOME_DIR/.codex"
OLD_CONFIG_FILE="$OLD_CONFIG_DIR/config.toml"
if [ -f "$OLD_CONFIG_FILE" ]; then
    echo "⚠️  Old config file still exists at $OLD_CONFIG_FILE"
    echo "   This may interfere with testing"
else
    echo "✅ Old config file does not exist (good)"
fi