# Codex-Local UI/Rendering Issues

## Issues Identified from User Testing

### 1. **Thinking Block Rendering - "Weird Square"**
**Problem**: The thinking block (`<think>`, `<thinking>`, `<reasoning>` tags) is rendering as a "weird square" instead of a proper bordered box.

**Current behavior**:
```
╭─ 💭 Thinking ────────────────────────────────────────────────────────────╮
│ The user is asking what model I am...
│ ...
╰──────────────────────────────────────────────────────────────────────────╯
```

**What's wrong**:
- Box characters might be misaligned
- Width calculation might be incorrect
- Border style might not be rendering properly
- May be conflicting with the terminal's character encoding

**Location**: `/Users/sero/projects/codex-local/codex-rs/tui/src/text_formatting.rs` and `/Users/sero/projects/codex-local/codex-rs/tui/src/markdown.rs`

---

### 2. **Footer Token Display Missing**
**Problem**: The footer shows "100% context left" but doesn't show the detailed token information that was supposed to be added.

**Expected behavior**:
```
75% left · 90K/120K · used: 30K · remaining: 90K · session: 45K
```

**Current behavior**:
```
100% context left · ? for shortcuts
```

**What's wrong**:
- The token usage data isn't being passed to the footer
- The footer rendering code might not be using the new fields
- Token tracking might not be initialized properly

**Location**: `/Users/sero/projects/codex-local/codex-rs/tui/src/bottom_pane/footer.rs`

---

### 3. **Message Rendering Order Issue**
**Problem**: "The first message always renders before the chart" - messages appear out of order or before the UI is fully initialized.

**Expected behavior**:
- Welcome message appears after the header chart/box
- Messages should render in chronological order
- UI should be fully rendered before showing messages

**Current behavior**:
- First message ("Hello! How can I help you today?") appears before or alongside the header
- Layout seems to be rendering in wrong order

**What's wrong**:
- Widget rendering order might be incorrect
- Message history might be rendering before layout is established
- Race condition between UI initialization and first message

**Location**: `/Users/sero/projects/codex-local/codex-rs/tui/src/chatwidget.rs` (main widget rendering)

---

### 4. **Slash Command Info Display**
**Observed**: When typing `/context` and `/compact-settings`, the information displays correctly in markdown format.

**Status**: ✅ **Working correctly** - This feature is functioning as intended.

---

## Research Questions

1. **Ratatui Box Drawing**: How to properly render bordered boxes in Ratatui/tui-rs?
2. **Widget Rendering Order**: What's the correct way to control render order in Ratatui?
3. **State Management**: How to properly pass token usage state to footer widgets?
4. **Terminal Compatibility**: How to ensure box-drawing characters render correctly across terminals?
5. **Layout Constraints**: How to properly size and position bordered blocks in Ratatui?

---

## Priority

1. **HIGH**: Footer token display (core functionality missing)
2. **HIGH**: Thinking block rendering (visual corruption)
3. **MEDIUM**: Message rendering order (UX issue but not breaking)

---

## Next Steps

1. Research Ratatui documentation for:
   - Block widget and border rendering
   - State passing between widgets
   - Widget render order control

2. Search for examples of:
   - Token counter displays in TUI apps
   - Bordered text boxes in Ratatui
   - Proper widget composition patterns

3. Review existing codebase:
   - How does status output render correctly?
   - How do other bordered elements work?
   - What's the current widget hierarchy?
