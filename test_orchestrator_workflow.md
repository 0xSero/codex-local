# Test Plan for Orchestrator Workflow Fix

## Test Scenario
1. Start codex-local with a model that supports the spawn_agent tool
2. Submit a prompt that will trigger spawn_agent usage
3. Verify that:
   - Subagent is spawned successfully
   - Main model waits for subagent completion
   - Subagent results are injected into main conversation
   - Main model continues and uses the subagent results

## Example Test Prompt
"Please use the spawn_agent tool to create a subagent that will analyze the current directory structure and return a summary. Then use that summary to create a README file."

## Expected Behavior
1. Main model calls spawn_agent with appropriate task
2. Subagent spawns and analyzes directory structure
3. Subagent completes and results are injected
4. Main model receives the injected results
5. Main model uses the results to create README file
6. No hanging or infinite waiting occurs

## Test Commands
```bash
# Build the project
cargo build --bin codex-local

# Run the test (this would require interactive testing)
./target/debug/codex-local
```

## Verification Points
- [ ] Subagent spawned without errors
- [ ] Subagent completes and produces results
- [ ] Results appear in main conversation context
- [ ] Main model continues after subagent completion
- [ ] No hanging or deadlocks occur
- [ ] UI shows proper agent lifecycle events

## Files Modified
- `codex-rs/core/src/tools/handlers/spawn_agent.rs`: Fixed injection timing and tool return behavior