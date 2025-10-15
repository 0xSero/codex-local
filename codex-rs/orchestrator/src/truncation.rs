//! Output truncation utilities for limiting subagent output to a token budget.

/// Token limit for subagent output (5000 tokens as specified in design).
pub const OUTPUT_TOKEN_LIMIT: usize = 5000;

/// Simple output truncator that limits content to a maximum token count.
/// Uses a line-based approximation where each line is estimated at ~20 tokens.
#[derive(Debug)]
pub struct OutputTruncator {
    limit: usize,
    accumulated_tokens: usize,
}

impl OutputTruncator {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            accumulated_tokens: 0,
        }
    }

    /// Estimates token count for a text string.
    /// Uses a simple heuristic: ~4 characters per token on average.
    fn estimate_tokens(text: &str) -> usize {
        // Simple estimation: divide character count by 4
        text.len().div_ceil(4)
    }

    /// Truncates content if it exceeds the token limit.
    /// Returns the truncated content and whether truncation occurred.
    pub fn truncate_if_needed(&mut self, content: &str) -> (String, bool) {
        let content_tokens = Self::estimate_tokens(content);

        if self.accumulated_tokens >= self.limit {
            // Already at limit, return truncation message
            return ("\n[Output truncated at 5k token limit]".to_string(), true);
        }

        if self.accumulated_tokens + content_tokens <= self.limit {
            // Content fits within limit
            self.accumulated_tokens += content_tokens;
            return (content.to_string(), false);
        }

        // Need to partially truncate
        let remaining_tokens = self.limit.saturating_sub(self.accumulated_tokens);
        let remaining_chars = remaining_tokens * 4;

        let mut truncated = String::new();
        let mut chars_used = 0;

        for line in content.lines() {
            let line_len = line.len() + 1; // +1 for newline
            if chars_used + line_len > remaining_chars {
                break;
            }
            if !truncated.is_empty() {
                truncated.push('\n');
            }
            truncated.push_str(line);
            chars_used += line_len;
        }

        self.accumulated_tokens = self.limit;
        truncated.push_str("\n\n[Output truncated at 5k token limit]");

        (truncated, true)
    }

    /// Returns the current token count.
    pub fn current_tokens(&self) -> usize {
        self.accumulated_tokens
    }

    /// Returns whether the limit has been reached.
    pub fn is_at_limit(&self) -> bool {
        self.accumulated_tokens >= self.limit
    }
}

impl Default for OutputTruncator {
    fn default() -> Self {
        Self::new(OUTPUT_TOKEN_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncator_basic() {
        let mut truncator = OutputTruncator::new(100);
        let content = "Hello, world!";
        let (result, truncated) = truncator.truncate_if_needed(content);
        assert_eq!(result, content);
        assert!(!truncated);
    }

    #[test]
    fn test_truncator_at_limit() {
        let mut truncator = OutputTruncator::new(10);
        let content = "a".repeat(50); // ~12 tokens
        let (result, truncated) = truncator.truncate_if_needed(&content);
        assert!(truncated);
        assert!(result.contains("[Output truncated at 5k token limit]"));
    }

    #[test]
    fn test_truncator_multiple_calls() {
        let mut truncator = OutputTruncator::new(20);
        let content1 = "a".repeat(40); // ~10 tokens
        let (result1, truncated1) = truncator.truncate_if_needed(&content1);
        assert!(!truncated1);
        assert_eq!(result1, content1);

        let content2 = "b".repeat(40); // ~10 tokens
        let (result2, truncated2) = truncator.truncate_if_needed(&content2);
        assert!(!truncated2);
        assert_eq!(result2, content2);

        // Third call should be truncated
        let content3 = "c".repeat(40);
        let (_result3, truncated3) = truncator.truncate_if_needed(&content3);
        assert!(truncated3);
    }
}
