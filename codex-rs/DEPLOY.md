# Deployment Guide for codex-local

## Quick Install (Development)

For fast iteration during development, use the **install script** which installs the debug build:

```bash
# Build and install in one step
cargo build --bin codex-local && ./install.sh
```

Or if already built:
```bash
./install.sh
```

This installs to `~/.local/bin/codex-local`.

## Production Deploy

For production use with optimizations, use the **deploy script**:

```bash
./deploy.sh
```

This will:
1. Build with `--release` flag (slower build, faster runtime)
2. Install to `~/.local/bin/codex-local`
3. Make the binary executable
4. Check if `~/.local/bin` is in your PATH

## What Changed

- **Binary name**: Changed from `codex` to `codex-local` in `cli/Cargo.toml`
- **Installation**: Automated via `install.sh` (debug) and `deploy.sh` (release)

## Running codex-local

After installation:

```bash
codex-local
```

If you get "command not found", ensure `~/.local/bin` is in your PATH:

```bash
# Add to ~/.bashrc or ~/.zshrc
export PATH="$HOME/.local/bin:$PATH"

# Reload shell
source ~/.bashrc  # or ~/.zshrc
```

## Features Now Available

### 1. spawn_agent Tool
When you have `orchestrator_profile` and `agent_profiles` configured in `~/.codex-local/config.toml`, the AI can spawn child agents:

```toml
orchestrator_profile = "orchestrator"
agent_profiles = ["worker", "worker-fast"]
```

### 2. Context Tracking in Footer
The footer shows detailed token usage:
```
72% context left · 33/120 tokens (33K used, 86K remaining, 45K total session)
```

This displays:
- Percentage of context window remaining
- Used/max tokens for current conversation
- Total tokens used in session

Both features are fully implemented and ready to use!
