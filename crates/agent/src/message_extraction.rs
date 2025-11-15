//! Message text extraction for embedding generation

use crate::Message;
use agent_memory::embedding::normalize_text_for_embedding;

/// Extract text content from a message for embedding
pub fn extract_message_text(message: &Message) -> String {
    match message {
        Message::User(user_msg) => {
            let mut text_parts = Vec::new();
            for content in &user_msg.content {
                match content {
                    crate::UserMessageContent::Text(text) => {
                        text_parts.push(text.as_str());
                    }
                    // Skip other content types for now (images, etc.)
                    _ => {}
                }
            }
            text_parts.join(" ")
        }
        Message::Agent(agent_msg) => {
            let mut text_parts = Vec::new();
            for content in &agent_msg.content {
                match content {
                    crate::AgentMessageContent::Text(text) => {
                        text_parts.push(text.as_str());
                    }
                    crate::AgentMessageContent::Thinking { text, .. } => {
                        // Include thinking in embedding
                        text_parts.push(text.as_str());
                    }
                    // Skip tool uses and results for now (could include later)
                    _ => {}
                }
            }
            text_parts.join(" ")
        }
        Message::Resume => {
            // Resume messages don't have text content
            String::new()
        }
    }
}

/// Extract aggregated text from all messages in a session
pub fn extract_session_text(messages: &[Message]) -> String {
    let mut text_parts = Vec::new();
    for message in messages {
        let text = extract_message_text(message);
        if !text.is_empty() {
            text_parts.push(text);
        }
    }
    let combined = text_parts.join("\n");
    normalize_text_for_embedding(&combined)
}

