# Codex-Local: Custom LLM Integration

Fork of OpenAI Codex CLI with custom model support and enhanced features.

## Features

### Custom LLM Support
- **120K Context Window** - Extended context for longer conversations
- **64K Output Tokens** - Large response capacity
- **Auto-Compaction** - Triggers at 75% context usage (90K tokens)
- **Custom API Provider** - Connect to any OpenAI-compatible endpoint

### Enhanced UI
- **Real-time Token Tracking** - See token usage as you chat
- **XML Thinking Blocks** - Beautiful bordered rendering of `<think>`, `<thinking>`, `<reasoning>` tags
- **Detailed Footer** - Shows: % left · current/max · used · remaining · session total
- **7 Custom Slash Commands** - Quick access to settings and info

### MCP Integration
Includes 6 pre-configured MCP servers:
- **brave-search** - Web search
- **context7** - Library documentation
- **image_recognition** - Z.AI image/video analysis
- **memory** - Knowledge graph persistence
- **puppeteer** - Browser automation
- **supabase** - Database operations

## Installation

```bash
# Clone the repository
git clone https://github.com/0xSero/codex-local.git
cd codex-local

# Build and install
~/bin/codex-local-build
```

The build script:
1. Builds Rust binaries in release mode
2. Copies to vendor directory
3. Runs `npm link` to install globally

## Configuration

Config file: `~/.codex-local/config.toml`

### Example Configuration

```toml
model = "/mnt/llm_models/GLM-4.5-Air-AWQ-4bit"
model_provider = "custom-glm"
model_context_window = 120000
model_max_output_tokens = 65536
model_auto_compact_token_limit = 90000

[model_providers.custom-glm]
name = "Custom GLM"
base_url = "https://your-api-endpoint.com/v1"
wire_api = "chat"
request_max_retries = 5
stream_max_retries = 5
stream_idle_timeout_ms = 300000
```

### Profiles

Create profiles for different use cases:

```toml
[profiles.glm-long]
model_auto_compact_token_limit = 100000  # 83% context

[profiles.glm-fast]
model_context_window = 32000
model_auto_compact_token_limit = 24000
```

Use with: `codex-local -p glm-long`

## Slash Commands

Access settings and info during chat:

- `/config` - View full configuration
- `/context` - Show context window size
- `/tokens` - Show max output tokens
- `/provider` - Show API provider details
- `/models` - Show model information
- `/compact-settings` - Show auto-compaction settings
- `/think` - Show XML rendering status

## Token Tracking

Codex-local tracks tokens in real-time:

### Input Tokens
Counted automatically when you send messages using tiktoken (cl100k_base)

### Output Tokens
Counted as the model streams responses back

### Footer Display
Shows live updates:
```
75% context left · 30/120 tokens (30K used, 90K remaining, 45K total session)
```

## Technical Details

### Architecture
- **Rust TUI** - Terminal UI built with Ratatui
- **Tokio async** - Non-blocking I/O
- **MCP Protocol** - Model Context Protocol for tool integrations
- **OpenAI Compatible** - Works with any `/v1/chat/completions` endpoint

### Token Counting
- Uses `tiktoken-rs` library
- cl100k_base tokenizer (GPT-4 compatible)
- Tracks per-message and session totals
- Updates on every stream chunk

### XML Rendering
Custom parser for thinking blocks:
- Detects `<think>`, `<thinking>`, `<thought>`, `<reasoning>`, `<internal>` tags
- Renders as bordered boxes with proper text wrapping
- 76-character fixed width for consistency

## Development

### Build from Source

```bash
cd codex-rs
cargo build --release --bin codex
```

### Run Tests

```bash
cargo test
```

### Project Structure

```
codex-local/
├── codex-rs/          # Rust source code
│   ├── tui/           # Terminal UI implementation
│   ├── core/          # Core Codex logic
│   ├── mcp-client/    # MCP integration
│   └── ...
├── codex-cli/         # npm wrapper
│   └── vendor/        # Platform binaries
└── README-CODEX-LOCAL.md
```

## Branch Information

**Branch**: `local/llms`
All custom modifications live here. Keep this branch separate from upstream updates.

## Troubleshooting

### Token counts not updating
- Ensure you're on the `local/llms` branch
- Rebuild: `~/bin/codex-local-build`
- Check that tiktoken-rs compiled successfully

### Thinking blocks look wrong
- Terminal must support Unicode box-drawing characters
- Try iTerm2 or Alacritty with `builtin_box_drawing` enabled

### Model not responding
- Check API endpoint in config: `base_url`
- Verify API is reachable: `curl $base_url/v1/models`
- Check logs: `~/.codex-local/log/codex-tui.log`

## Credits

- **Original Codex**: OpenAI
- **Custom Fork**: 0xSero
- **Enhancements**: Claude (token tracking, UI improvements, documentation)

## License

Apache 2.0 (inherited from upstream Codex project)
