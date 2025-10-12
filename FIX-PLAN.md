# Codex-Local UI Fixes - Implementation Plan

## Summary of Findings

After analyzing the code and researching Ratatui best practices, I've identified the root causes and solutions for all three issues.

---

## Issue 1: Thinking Block Rendering ("Weird Square")

### Root Cause
File: `/Users/sero/projects/codex-local/codex-rs/tui/src/text_formatting.rs`

**Problems**:
1. **Line 53-54**: Top border uses fixed 60 dashes (`"─".repeat(60)`)
2. **Line 68**: Bottom border uses fixed 70 dashes (`"─".repeat(70)`) - inconsistent!
3. **Lines 61-65**: Content wrapping doesn't calculate terminal width
4. **Lines 84-89**: Flawed logic for tracking when inside thinking tags

### Solution
Replace the simple string-based XML parser with proper box rendering:
1. Calculate the actual terminal width
2. Use Ratatui's word wrap to properly break lines
3. Ensure top and bottom borders match
4. Fix the state machine for tracking tag boundaries

### Code Changes
```rust
pub(crate) fn format_xml_thinking_blocks(text: &str, width: usize) -> String {
    const THINKING_TAGS: &[&str] = &["think", "thinking", "thought", "reasoning", "internal"];

    let box_width = width.saturating_sub(4); // Account for "│ " prefix
    let mut result = String::new();
    let mut in_thinking_block = false;
    let mut current_tag = String::new();
    let mut content = String::new();

    // Parse XML properly
    // When opening tag found:
    result.push_str(&format!("\n╭─ 💭 Thinking {}╮\n", "─".repeat(box_width.saturating_sub(14))));

    // For each content line (with proper wrapping):
    for line in wrap_text(&content, box_width) {
        result.push_str(&format!("│ {:width$} │\n", line, width = box_width));
    }

    // When closing tag found:
    result.push_str(&format!("╰{}╯\n", "─".repeat(box_width + 2)));

    result
}
```

---

## Issue 2: Footer Token Display Missing

### Root Cause
Files analyzed:
- `/Users/sero/projects/codex-local/codex-rs/tui/src/chatwidget.rs:424-442`
- `/Users/sero/projects/codex-local/codex-rs/tui/src/bottom_pane/footer.rs:227-249`

**Status**: Code is CORRECT! The footer rendering logic works.

**Problem**: Token info is not being set initially or being cleared somewhere.

### Investigation Needed
1. Check if `set_token_info()` is called on first message
2. Check if token info is cleared inadvertently
3. Verify TokenUsageInfo is being received from the model

### Solution
Debug why token info isn't available:
1. Add logging to `set_token_info()` to see when it's called
2. Check if the GLM model is sending token usage in responses
3. If model doesn't send tokens, calculate them client-side based on config

### Code Changes
Add fallback token calculation in `chatwidget.rs`:
```rust
pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
    if let Some(info) = info {
        // Existing code...
    } else {
        // Fallback: Use config values when model doesn't send token info
        if let (Some(context_window), Some(max_output)) = (
            self.config.model_context_window,
            self.config.model_max_output_tokens
        ) {
            // Estimate tokens based on message history
            let estimated_used = self.estimate_tokens_in_history();
            self.bottom_pane.set_context_tokens(
                Some(estimated_used),
                Some(context_window),
                Some(estimated_used)
            );
        }
    }
}
```

---

## Issue 3: Message Rendering Order

### Root Cause
File: `/Users/sero/projects/codex-local/codex-rs/tui/src/chatwidget.rs` (WidgetRef implementation)

**Problem**: From Ratatui research, render order is controlled by the sequence of render calls. The first message might be rendering before the header/layout is fully established.

### Investigation Needed
1. Check the `render_ref()` method in chatwidget.rs
2. Verify the order of widget rendering
3. Look for race conditions in initial message display

### Solution (Per Ratatui docs)
"Render order is controlled by call sequence - later renders overwrite earlier ones in the same Buffer."

Ensure proper render order:
1. Render layout/header first
2. Then render history
3. Then render active cell
4. Finally render footer

### Code Changes
Review and fix render order in `/Users/sero/projects/codex-local/codex-rs/tui/src/chatwidget.rs`:
```rust
impl WidgetRef for &ChatWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        // 1. First: Layout background/header
        self.render_header(area, buf);

        // 2. Second: History (scrollable area)
        let [header_area, history_area, bottom_pane_area] = self.layout_areas(area);
        self.render_history(history_area, buf);

        // 3. Third: Active cell (if any)
        if !active_cell_area.is_empty() && let Some(cell) = &self.active_cell {
            // ... render active cell
        }

        // 4. Finally: Footer (should be on top)
        (&self.bottom_pane).render(bottom_pane_area, buf);
    }
}
```

---

## Implementation Priority

### Phase 1: Fix Thinking Block Rendering (HIGH)
- Rewrite `text_formatting.rs::format_xml_thinking_blocks()`
- Add width parameter
- Fix border calculations
- Add proper word wrapping

### Phase 2: Fix Footer Token Display (HIGH)
- Debug why token info is missing
- Add fallback token estimation
- Test with GLM model to verify token reporting

### Phase 3: Fix Message Rendering Order (MEDIUM)
- Review WidgetRef implementation
- Ensure proper render sequence
- Test initial message display

---

## Testing Checklist

After each fix:
- [ ] Build succeeds without warnings
- [ ] Thinking blocks render with correct borders
- [ ] Footer shows detailed token counts
- [ ] Messages render in correct order
- [ ] Layout doesn't break on terminal resize
- [ ] All slash commands still work

---

## Next Steps

1. Start with Phase 1 - most visually obvious issue
2. Research proper word wrapping in Rust (textwrap crate?)
3. Test each fix incrementally
4. Commit changes with clear messages
