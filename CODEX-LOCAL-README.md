# Codex-Local - Custom LLM Integration

Local fork of Codex configured for custom LLM providers (vLLM, Ollama, etc.)

## Quick Start

```bash
# Start codex-local
codex-local

# Start with specific profile
codex-local -p glm-default

# Show help
codex-local --help
```

## Slash Commands

Access all features via slash commands in the chat interface. Type `/` to see all available commands.

### Configuration Commands

#### `/config` - View and Edit Configuration
View current configuration or edit specific settings.

```
/config                    # Show current config
/config context 120000     # Set context window
/config tokens 65536       # Set max output tokens
/config compact 90000      # Set compaction limit
```

#### `/context` - Set Context Window
Configure the context window size for your model.

```
/context                   # Show current: 120K tokens
/context 150000            # Set to 150K tokens
/context 32000             # Set to 32K tokens
```

#### `/tokens` - Set Max Output Tokens
Configure maximum output tokens per response.

```
/tokens                    # Show current: 64K tokens
/tokens 32768              # Set to 32K tokens
/tokens 100000             # Set to 100K tokens
```

#### `/provider` - Configure API Provider
View or change the API provider and endpoint.

```
/provider                               # Show current provider
/provider url https://your-api.com/v1   # Set API endpoint
/provider model /path/to/model          # Set model path
```

#### `/models` - List Available Models
Fetch and display available models from your provider's `/v1/models` endpoint.

```
/models                    # List all available models
/models glm                # Filter models containing 'glm'
```

### Auto-Compaction Commands

#### `/compact-settings` - Configure Auto-Compaction
Manage automatic conversation compaction settings.

```
/compact-settings                # Show current settings
/compact-settings 90000          # Set compaction threshold to 90K tokens
/compact-settings off            # Disable auto-compaction
```

**How it works:**
- When conversation reaches the token limit, older messages are summarized
- Recent context is preserved
- Frees up space for continued conversation

**Profile Defaults:**
- `glm-default`: 90K tokens (75% of 120K context)
- `glm-fast`: 24K tokens (75% of 32K context)
- `glm-long`: 100K tokens (83% of 120K context)

### Visual Features

#### `/think` - Toggle XML Thinking Block Rendering
Enable or disable beautiful rendering of model thinking blocks.

```
/think                     # Show current state
/think on                  # Enable rendering
/think off                 # Disable rendering
```

**When enabled**, XML tags like `<think>` are rendered as:
```
╭─ 💭 Thinking ────────────────────────────────────────╮
│ Model's internal reasoning process...
╰──────────────────────────────────────────────────────╯
```

**Supported tags:** `<think>`, `<thinking>`, `<thought>`, `<reasoning>`, `<internal>`

### Status & Information

#### `/status` - Show Session Info
Display current session configuration and token usage.

```
/status
```

Shows:
- Current model and provider
- Context window size and usage
- Token counts (used/remaining/total)
- Auto-compaction settings
- MCP tools loaded

#### `/mcp` - List MCP Tools
Display all configured MCP (Model Context Protocol) servers and tools.

```
/mcp                       # List all MCP tools
```

**Configured MCP Servers:**
- Brave Search - Web search capability
- Context7 - Developer documentation
- Z.ai - Image recognition
- Memory - Conversation memory
- Puppeteer - Browser automation
- Supabase - Database operations

## Profiles

Use profiles to quickly switch between configurations:

### Available Profiles

**glm-default** (Balanced)
- 120K context window
- 64K max output tokens
- 90K auto-compact threshold
- Debug logging

**glm-fast** (Quick Responses)
- 32K context window
- 64K max output tokens
- 24K auto-compact threshold
- Info logging

**glm-long** (Extended Conversations)
- 120K context window
- 64K max output tokens
- 100K auto-compact threshold
- Debug logging

### Using Profiles

```bash
# Via command line
codex-local -p glm-default
codex-local -p glm-fast
codex-local -p glm-long

# Via shell aliases (if configured)
codex-default
codex-fast
codex-long
```

## Command Line Options

```bash
# Basic usage
codex-local [OPTIONS] [PROMPT]

# Common options
-p, --profile <PROFILE>              Use configuration profile
-m, --model <MODEL>                  Override model
-c, --config <key=value>             Override config value
-i, --image <FILE>...                Attach image(s)

# Context options
-c model_context_window=150000       Set context window
-c model_max_output_tokens=32768     Set max output
-c model_auto_compact_token_limit=100000  Set compaction

# Examples
codex-local -p glm-fast
codex-local -c model_context_window=150000
codex-local -i screenshot.png "analyze this"
```

## Configuration File

Location: `~/.codex-local/config.toml`

```toml
# Main configuration
model = "/mnt/llm_models/GLM-4.5-Air-AWQ-4bit"
model_provider = "custom-glm"
model_context_window = 120000
model_max_output_tokens = 65536
model_auto_compact_token_limit = 90000

[model_providers.custom-glm]
name = "Custom GLM"
base_url = "https://your-api.ngrok-free.dev/v1"
wire_api = "chat"
request_max_retries = 5
stream_max_retries = 5
stream_idle_timeout_ms = 300000

# MCP Servers
[mcp_servers.brave-search]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-brave-search"]

# ... more servers ...
```

## Updates

### Update to Latest Version

```bash
codex-local-update
```

This will:
1. Fetch latest changes from GitHub
2. Show what changed
3. Ask for confirmation
4. Rebuild binaries
5. Reinstall package

### Manual Update

```bash
cd ~/projects/codex-local
git pull origin main
cd codex-rs && cargo build --release --bin codex
cp target/release/codex ../codex-cli/vendor/aarch64-apple-darwin/codex/
cd ../codex-cli && npm link
```

## Utilities

### codex-local-switch
Switch configuration settings on the fly.

```bash
codex-local-switch profile                          # List profiles
codex-local-switch model /path/to/other-model      # Change model
codex-local-switch url https://new-api.com/v1      # Change API URL
codex-local-switch list                            # Show current config
```

### codex-local-verify
Verify your installation and configuration.

```bash
codex-local-verify
```

## Footer Status Line

The footer shows real-time context information:

```
72% context left · 33/120 tokens (33K used, 86K remaining, 45K total session) · ? for shortcuts
```

- **72% context left** - Percentage of context window remaining
- **33/120 tokens** - Current used / Max available (in thousands)
- **33K used** - Tokens used in current context
- **86K remaining** - Tokens still available
- **45K total session** - All tokens used this session

## Tips & Tricks

### Did You Know?

1. **Context Monitoring**: The footer updates in real-time showing exact token usage
2. **Auto-Compaction**: Conversations automatically compact when reaching 75% capacity
3. **XML Rendering**: Thinking blocks render beautifully when models use `<think>` tags
4. **Profile Switching**: Switch profiles mid-conversation with `/config profile <name>`
5. **Model Discovery**: Use `/models` to see all available models from your provider
6. **MCP Tools**: 51 tools available from 6 MCP servers for enhanced capabilities
7. **Image Analysis**: Attach images with `-i` flag or drag-drop in supported terminals

### Keyboard Shortcuts

- `?` - Show all shortcuts
- `Ctrl+C` - Interrupt current task
- `Esc` - Cancel input or go back
- `Shift+Enter` - New line in input
- `/` - Open slash command menu

## Troubleshooting

### Check Logs
```bash
tail -f ~/.codex-local/log/codex-tui.log
```

### Verify Configuration
```bash
codex-local-verify
```

### Reset to Defaults
```bash
cp ~/.codex-local-backup/config.toml ~/.codex-local/config.toml
```

## Architecture

**Codex-Local Structure:**
- `codex-rs/` - Rust TUI and core logic
- `codex-cli/` - Node.js CLI wrapper
- `~/.codex-local/` - Configuration and logs
- `/Users/sero/bin/` - Utility scripts

**Key Features:**
- Custom API provider support (vLLM, Ollama, etc.)
- Configurable context windows (32K - 120K+)
- Automatic conversation compaction
- XML thinking block visualization
- Real-time token usage display
- MCP server integration
- Profile-based configuration

## Support

For issues or questions:
1. Check logs: `~/.codex-local/log/codex-tui.log`
2. Run diagnostics: `codex-local-verify`
3. Review config: `~/.codex-local/config.toml`
4. Check status: `/status` command in chat

---

**Version**: 1.0.0-local-26f7c468
**Branch**: local/llms
**Model**: GLM-4.5-Air-AWQ-4bit
**Context**: 120K tokens | Output: 64K tokens
