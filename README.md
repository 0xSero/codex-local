# Codex-Local: Enhanced OpenAI Codex CLI

> **Fork of OpenAI Codex CLI with advanced orchestrator features, child agent spawning, and enhanced tool capabilities**

## 🎯 Why This Fork Exists

Codex-Local exists to extend OpenAI's Codex CLI with powerful multi-agent capabilities and enhanced tooling that are not available in the main distribution. This fork adds:

- **Multi-Agent Orchestration** - Spawn and coordinate specialized child agents
- **Enhanced Tool System** - Advanced tools with progress reporting and parallel execution
- **Custom Configuration** - Full control over models, providers, and behavior
- **Non-Conflicting Installation** - Runs alongside your main Codex/Cursor installation

---

## 🚀 Key Features

### 🤖 Multi-Agent Orchestration System

**Core Innovation**: The ability to spawn specialized child agents that can work in parallel and report progress back to a parent agent.

#### How It Works:
1. **Parent Agent** receives a complex task
2. **Spawn Agent Tool** creates specialized child agents for different subtasks
3. **Child Agents** work independently with their own tool access
4. **Return Progress Tool** allows child agents to send status updates back to parent
5. **Parent Agent** coordinates all child agents and synthesizes results

#### Benefits:
- **Parallel Processing** - Multiple agents work simultaneously on different aspects of a task
- **Specialization** - Each agent can focus on a specific domain (e.g., frontend, backend, testing)
- **Scalability** - Complex projects can be broken down into manageable pieces
- **Real-time Updates** - Parent agent receives progress reports from all children

### 🛠️ Enhanced Tool System

#### Return Progress Tool
- **Purpose**: Child agents send progress updates to parent agents
- **Parameters**:
  - `task_id` (optional): Unique identifier for the task/agent
  - `progress` (required): Progress message
  - `is_final` (optional): Whether this is the final update
- **Use Case**: Long-running tasks where the parent needs to monitor progress

#### Spawn Agent Tool
- **Purpose**: Create new child agents with specific instructions and tools
- **Features**:
  - Parallel execution support
  - Customizable tool sets for each agent
  - Independent context management
  - Automatic cleanup when tasks complete

#### Enhanced Tool Registry
- **Dynamic Tool Loading**: Tools can be conditionally enabled based on configuration
- **Parallel Tool Execution**: Multiple tools can run simultaneously
- **Progress Tracking**: Tools can report progress during long operations
- **Error Handling**: Robust error recovery and reporting

### 🏗️ Advanced Architecture

#### Child Agent Bridge
- **Isolation**: Each child agent has its own context and session
- **Communication**: Secure bridge between parent and child agents
- **Progress Tracking**: Child agents can send real-time updates to parents
- **Resource Management**: Automatic cleanup of completed agents

#### Enhanced Conversation Manager
- **Multi-Agent Support**: Manage multiple concurrent agent conversations
- **Context Isolation**: Each agent maintains its own context
- **Progress Events**: Real-time progress reporting between agents
- **Session Management**: Advanced session lifecycle management

---

## 📦 Installation

### Prerequisites
- Rust 1.70+
- Node.js 18+
- Git

### Step 1: Clone Repository
```bash
git clone https://github.com/0xSero/codex-local.git
cd codex-local
```

### Step 2: Build and Install
```bash
# Build Rust components
cd codex-rs
cargo build --release

# Install Node.js wrapper
cd ../codex-cli
npm install
npm run build
npm link
```

### Step 3: Verify Installation
```bash
codex-local --version
# Should output: codex-cli 1.0.0-local-<commit-hash>
```

### Non-Conflicting Setup

**This fork is designed to NOT conflict with your existing Codex/Cursor installation:**

- **Separate Binary**: `codex-local` vs `codex`
- **Separate Config**: `~/.codex-local/` vs `~/.codex/`
- **Separate Data**: Isolated sessions, history, and cache
- **Independent Updates**: Won't affect your main Codex installation

**Your main Codex/Cursor remains completely untouched!**

---

## ⚙️ Configuration

### Config File Location
```
~/.codex-local/config.toml
```

### Basic Configuration
```toml
# Model Settings
model = "gpt-4"
model_provider = "openai"
model_context_window = 120000
model_max_output_tokens = 65536

# Multi-Agent Settings
[orchestrator]
max_concurrent_agents = 5
agent_timeout_seconds = 300
enable_progress_tracking = true

# Tool Settings
[tools]
enable_spawn_agent = true
enable_return_progress = true
enable_parallel_execution = true

# Custom Provider (if needed)
[model_providers.custom]
name = "Custom API"
base_url = "https://your-api-endpoint.com/v1"
wire_api = "chat"
request_max_retries = 5
```

### Advanced Configuration

#### Profiles for Different Use Cases
```toml
[profiles.research]
enable_spawn_agent = true
max_concurrent_agents = 10
enable_parallel_execution = true

[profiles.simple]
enable_spawn_agent = false
model_context_window = 32000

[profiles.debug]
enable_progress_tracking = true
agent_timeout_seconds = 600
```

#### Custom Model Providers
```toml
[model_providers.anthropic]
name = "Anthropic Claude"
base_url = "https://api.anthropic.com"
wire_api = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[model_providers.local]
name = "Local LLM"
base_url = "http://localhost:8080/v1"
wire_api = "chat"
request_max_retries = 1
```

### Environment Variables
```bash
# API Keys (if not in config)
export OPENAI_API_KEY="your-key"
export ANTHROPIC_API_KEY="your-key"

# Debug Mode
export CODEX_LOCAL_DEBUG=true

# Custom Config Location
export CODEX_LOCAL_CONFIG="/path/to/config.toml"
```

---

## 🎮 Usage

### Basic Usage
```bash
# Start with default configuration
codex-local

# Use a specific profile
codex-local --profile research

# Use custom config file
codex-local --config /path/to/config.toml
```

### Multi-Agent Examples

#### 1. Research Task with Multiple Agents
```
I need to research and implement a web scraping solution. Please:
1. Spawn an agent to research the best scraping libraries for Python
2. Spawn another agent to research legal considerations for web scraping
3. Spawn a third agent to implement a basic scraper example
4. Coordinate the results and provide a comprehensive recommendation
```

#### 2. Code Review and Testing
```
Please review this codebase and create tests:
1. Spawn an agent to analyze the codebase structure
2. Spawn another agent to identify areas needing tests
3. Spawn a third agent to write unit tests
4. Have all agents report progress and coordinate final test suite
```

### Progress Monitoring
When child agents are working, you'll see real-time progress updates:
```
🔄 Agent research-agent: Found 5 scraping libraries, evaluating...
🔄 Agent legal-agent: Reviewing robots.txt and terms of service...
🔄 Agent implementation-agent: Setting up virtual environment...
✅ Agent research-agent: Research complete, recommending BeautifulSoup4
```

---

## 🔧 Development

### Building from Source
```bash
cd codex-rs
cargo build --release --bin codex-local
```

### Running Tests
```bash
# Run all tests
cargo test

# Run specific test suites
cargo test spawn_agent_parallel
cargo test orchestrator_tests
```

### Project Structure
```
codex-local/
├── codex-rs/                    # Rust source code
│   ├── core/                   # Core logic and tools
│   │   ├── src/
│   │   │   ├── tools/          # Enhanced tool system
│   │   │   ├── conversation_manager.rs  # Multi-agent support
│   │   │   ├── child_agent_bridge.rs    # Agent communication
│   │   │   └── orchestrator.rs   # Agent coordination
│   │   └── tests/suite/         # Comprehensive test suite
│   ├── cli/                    # Command-line interface
│   └── tui/                    # Terminal UI
├── codex-cli/                  # Node.js wrapper
│   ├── src/                    # Wrapper logic
│   └── vendor/                 # Platform binaries
└── docs/                       # Documentation
```

### Key Architecture Components

#### Child Agent Bridge (`core/src/child_agent_bridge.rs`)
- Manages communication between parent and child agents
- Handles progress reporting and status updates
- Provides secure isolation between agent contexts

#### Enhanced Tools (`core/src/tools/`)
- **Spawn Agent Tool**: Creates new child agents with custom instructions
- **Return Progress Tool**: Allows agents to report progress
- **Parallel Execution**: Multiple tools can run simultaneously

#### Conversation Manager (`core/src/conversation_manager.rs`)
- Manages multiple concurrent agent conversations
- Handles context isolation and resource management
- Coordinates agent lifecycle and cleanup

---

## 🆚 Key Differences from Main Codex

| Feature | Main Codex | Codex-Local |
|---------|------------|-------------|
| **Multi-Agent Support** | ❌ | ✅ Native orchestrator system |
| **Child Agent Spawning** | ❌ | ✅ `spawn_agent` tool |
| **Progress Reporting** | ❌ | ✅ `return_progress` tool |
| **Parallel Tool Execution** | ❌ | ✅ Full parallel support |
| **Custom Configuration** | ⚠️ Limited | ✅ Advanced configuration system |
| **Installation** | System-wide | ✅ Non-conflicting, isolated |
| **Config Location** | `~/.codex/` | ✅ `~/.codex-local/` |
| **Binary Name** | `codex` | ✅ `codex-local` |

### Technical Enhancements
- **Enhanced Tool Registry**: Dynamic tool loading with conditional activation
- **Improved Error Handling**: Better error recovery and reporting
- **Performance Optimizations**: Faster tool execution and context management
- **Extensibility**: Plugin-like architecture for custom tools

---

## 🐛 Troubleshooting

### Common Issues

#### Installation Problems
```bash
# If cargo build fails, ensure Rust is up to date
rustup update

# If npm link fails, try with sudo
sudo npm link

# Clean build if needed
cargo clean && cargo build --release
```

#### Configuration Issues
```bash
# Check config syntax
codex-local --check-config

# Reset to default config
mv ~/.codex-local/config.toml ~/.codex-local/config.toml.backup
codex-local --init-config
```

#### Agent Issues
```bash
# Check agent logs
tail -f ~/.codex-local/log/agents.log

# Debug mode
CODEX_LOCAL_DEBUG=true codex-local

# Reset agent state
rm -rf ~/.codex-local/sessions/
```

### Performance Issues
- **Reduce concurrent agents**: Set `max_concurrent_agents = 2` in config
- **Increase timeouts**: Set `agent_timeout_seconds = 600` for slow tasks
- **Disable parallel tools**: Set `enable_parallel_execution = false`

### Getting Help
- **Check logs**: `~/.codex-local/log/`
- **Debug mode**: `CODEX_LOCAL_DEBUG=true codex-local`
- **Issue reporting**: GitHub issues with full logs

---

## 🤝 Contributing

### Development Setup
```bash
# Clone and setup
git clone https://github.com/0xSero/codex-local.git
cd codex-local

# Install development dependencies
cd codex-rs
cargo install cargo-watch

# Run tests in watch mode
cargo watch -x test

# Build and run locally
cargo run --bin codex-local -- --help
```

### Code Style
- Follow Rust formatting conventions
- Add comprehensive tests for new features
- Update documentation for API changes
- Ensure all existing tests pass

### Submitting Changes
1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality
4. Ensure all tests pass
5. Submit a pull request with detailed description

---

## 📄 License

Apache 2.0 License - inherited from the original OpenAI Codex project.

---

## 🙏 Acknowledgments

- **OpenAI** - Original Codex CLI project
- **Anthropic** - Claude assistance with documentation and features
- **Contributors** - Community members who have helped improve this fork

---

## 📞 Support

- **Issues**: [GitHub Issues](https://github.com/0xSero/codex-local/issues)
- **Discussions**: [GitHub Discussions](https://github.com/0xSero/codex-local/discussions)
- **Documentation**: [Wiki](https://github.com/0xSero/codex-local/wiki)

---

**⭐ Star this repository if you find it useful!**

*This fork extends OpenAI Codex with powerful multi-agent capabilities while maintaining full compatibility with existing workflows.*