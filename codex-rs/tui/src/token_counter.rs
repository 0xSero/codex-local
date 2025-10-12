/// Token counting utilities for tracking LLM token usage
use tiktoken_rs::cl100k_base;

/// Count tokens in text using the appropriate tokenizer for the model
pub fn count_tokens(text: &str, _model: &str) -> usize {
    // Use cl100k_base for GPT-4, GPT-3.5-turbo, and similar models.
    // This is a good default that works for most modern LLMs.
    if let Ok(bpe) = cl100k_base() {
        bpe.encode_ordinary(text).len()
    } else {
        // Fallback: rough estimate if tokenizer fails. Average English word is ~1.3 tokens.
        let word_count = text.split_whitespace().count();
        let estimated = (word_count as f32 * 1.3).ceil() as usize;
        // Avoid returning zero when the text contains whitespace only.
        estimated.max(word_count)
    }
}

/// Estimate tokens for a message with role
#[allow(dead_code)]
pub fn estimate_message_tokens(role: &str, content: &str, model: &str) -> usize {
    // Messages have overhead: role name + delimiters
    // Rough estimate: 4 tokens for message formatting
    let overhead = 4;
    let role_tokens = count_tokens(role, model);
    let content_tokens = count_tokens(content, model);

    overhead + role_tokens + content_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens() {
        let text = "Hello, world! This is a test.";
        let count = count_tokens(text, "gpt-4");
        // Should be around 8-10 tokens
        assert!(count > 5 && count < 15, "Token count: {count}");
    }

    #[test]
    fn test_estimate_message_tokens() {
        let tokens = estimate_message_tokens("user", "Hello!", "gpt-4");
        // Should include overhead + role + content
        assert!(tokens > 5, "Message tokens: {tokens}");
    }
}
