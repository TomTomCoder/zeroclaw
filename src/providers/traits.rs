use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::fmt::Write;

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
        }
    }
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// An LLM response that may contain text, tool calls, or both.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    /// Text content of the response (may be empty if only tool calls).
    pub text: Option<String>,
    /// Tool calls requested by the LLM.
    pub tool_calls: Vec<ToolCall>,
}

impl ChatResponse {
    /// True when the LLM wants to invoke at least one tool.
    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    /// Convenience: return text content or empty string.
    pub fn text_or_empty(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    pub fn builder() -> ChatResponseBuilder {
        ChatResponseBuilder::new()
    }
}

pub struct ChatResponseBuilder {
    text: Option<String>,
    tool_calls: Vec<ToolCall>,
}

impl ChatResponseBuilder {
    pub fn new() -> Self {
        Self {
            text: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn text(mut self, text: Option<String>) -> Self {
        self.text = text;
        self
    }

    pub fn tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    pub fn build(&self) -> ChatResponse {
        ChatResponse {
            text: self.text.clone(),
            tool_calls: self.tool_calls.clone(),
        }
    }
}

/// Request payload for provider chat calls.
#[derive(Debug, Clone, Copy)]
pub struct ChatRequest<'a> {
    pub messages: &'a [ChatMessage],
    pub tools: Option<&'a [ToolSpec]>,
}

/// A tool result to feed back to the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub content: String,
}

/// A cached provider response that enables single-pass parsing.
///
/// This type holds the raw response data along with cached parsed components,
/// avoiding redundant deserialization when the same response is accessed
/// multiple times (e.g., text extraction followed by tool call extraction).
#[derive(Debug, Clone)]
pub struct CachedResponse {
    raw: String,
    parsed_text: Option<String>,
    parsed_tool_calls: Option<Vec<ToolCall>>,
}

impl CachedResponse {
    /// Create a new cached response from raw text.
    pub fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            parsed_text: None,
            parsed_tool_calls: None,
        }
    }

    /// Parse the response once, extracting both text and tool calls.
    /// Subsequent calls return the cached result.
    pub fn parse_once(&mut self) -> &ChatResponse {
        if self.parsed_text.is_some() || self.parsed_tool_calls.is_some() {
            return ChatResponse::builder()
                .text(self.parsed_text.clone())
                .tool_calls(self.parsed_tool_calls.clone().unwrap_or_default())
                .build();
        }

        let extracted = Self::extract_content(&self.raw);
        self.parsed_text = extracted.text;
        self.parsed_tool_calls = Some(extracted.tool_calls);

        ChatResponse::builder()
            .text(self.parsed_text.clone())
            .tool_calls(self.parsed_tool_calls.clone().unwrap_or_default())
            .build()
    }

    /// Get the text content, parsing if needed.
    pub fn text(&mut self) -> &str {
        if self.parsed_text.is_none() {
            let extracted = Self::extract_content(&self.raw);
            self.parsed_text = extracted.text;
            self.parsed_tool_calls = Some(extracted.tool_calls);
        }
        self.parsed_text.as_deref().unwrap_or("")
    }

    /// Get tool calls, parsing if needed.
    pub fn tool_calls(&mut self) -> &[ToolCall] {
        if self.parsed_tool_calls.is_none() {
            let extracted = Self::extract_content(&self.raw);
            self.parsed_text = extracted.text;
            self.parsed_tool_calls = Some(extracted.tool_calls);
        }
        self.parsed_tool_calls.as_deref().unwrap_or(&[])
    }

    /// Extract content from raw response in a single pass.
    fn extract_content(raw: &str) -> ExtractResult {
        let mut text = None;
        let mut tool_calls = Vec::new();

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
            if let Some(content_val) = value.get("content") {
                if let Some(arr) = content_val.as_array() {
                    for block in arr {
                        if let Some(block_type) = block.get("type").and_then(|t| t.as_str()) {
                            match block_type {
                                "text" => {
                                    if text.is_none() {
                                        text = block
                                            .get("text")
                                            .and_then(|t| t.as_str())
                                            .filter(|t| !t.is_empty())
                                            .map(|t| t.to_string());
                                    }
                                }
                                "tool_use" => {
                                    if let Some(name) = block.get("name").and_then(|n| n.as_str()) {
                                        let id = block
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .unwrap_or_else(|| uuid::Uuid::new_v4().as_str())
                                            .to_string();
                                        let arguments = block
                                            .get("input")
                                            .map(|i| i.to_string())
                                            .unwrap_or_else(|| "{}".to_string());
                                        tool_calls.push(ToolCall { id, name: name.to_string(), arguments });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            if text.is_none() {
                for key in ["content", "text", "message", "choices"] {
                    if let Some(val) = value.get(key) {
                        if let Some(arr) = val.as_array() {
                            if let Some(first) = arr.first() {
                                for msg_key in ["message", "content"] {
                                    if let Some(msg) = first.get(msg_key) {
                                        if let Some(s) = msg.as_str().filter(|s| !s.is_empty()) {
                                            text = Some(s.to_string());
                                            break;
                                        }
                                    }
                                }
                            }
                        } else if let Some(s) = val.as_str().filter(|s| !s.is_empty()) {
                            text = Some(s.to_string());
                            break;
                        }
                    }
                }
            }

            if tool_calls.is_empty() {
                if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
                    if let Some(first) = choices.first() {
                        if let Some(msg) = first.get("message") {
                            if let Some(tc) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                                for call in tc {
                                    if let Some(func) = call.get("function") {
                                        let id = call
                                            .get("id")
                                            .and_then(|i| i.as_str())
                                            .unwrap_or_else(|| uuid::Uuid::new_v4().as_str())
                                            .to_string();
                                        let name = func
                                            .get("name")
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let arguments = func
                                            .get("arguments")
                                            .map(|a| a.to_string())
                                            .unwrap_or_else(|| "{}".to_string());
                                        tool_calls.push(ToolCall { id, name, arguments });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        ExtractResult {
            text,
            tool_calls,
        }
    }
}

struct ExtractResult {
    text: Option<String>,
    tool_calls: Vec<ToolCall>,
}

/// A message in a multi-turn conversation, including tool interactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ConversationMessage {
    /// Regular chat message (system, user, assistant).
    Chat(ChatMessage),
    /// Tool calls from the assistant (stored for history fidelity).
    AssistantToolCalls {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    /// Results of tool executions, fed back to the LLM.
    ToolResults(Vec<ToolResultMessage>),
}

/// A chunk of content from a streaming response.
#[derive(Debug, Clone)]
pub struct StreamChunk {
    /// Text delta for this chunk.
    pub delta: String,
    /// Whether this is the final chunk.
    pub is_final: bool,
    /// Approximate token count for this chunk (estimated).
    pub token_count: usize,
}

impl StreamChunk {
    /// Create a new non-final chunk.
    pub fn delta(text: impl Into<String>) -> Self {
        Self {
            delta: text.into(),
            is_final: false,
            token_count: 0,
        }
    }

    /// Create a final chunk.
    pub fn final_chunk() -> Self {
        Self {
            delta: String::new(),
            is_final: true,
            token_count: 0,
        }
    }

    /// Create an error chunk.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            delta: message.into(),
            is_final: true,
            token_count: 0,
        }
    }

    /// Estimate tokens (rough approximation: ~4 chars per token).
    pub fn with_token_estimate(mut self) -> Self {
        self.token_count = self.delta.len().div_ceil(4);
        self
    }
}

/// Options for streaming chat requests.
#[derive(Debug, Clone, Copy, Default)]
pub struct StreamOptions {
    /// Whether to enable streaming (default: true).
    pub enabled: bool,
    /// Whether to include token counts in chunks.
    pub count_tokens: bool,
}

impl StreamOptions {
    /// Create new streaming options with enabled flag.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            count_tokens: false,
        }
    }

    /// Enable token counting.
    pub fn with_token_count(mut self) -> Self {
        self.count_tokens = true;
        self
    }
}

/// Result type for streaming operations.
pub type StreamResult<T> = std::result::Result<T, StreamError>;

/// Errors that can occur during streaming.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("HTTP error: {0}")]
    Http(reqwest::Error),

    #[error("JSON parse error: {0}")]
    Json(serde_json::Error),

    #[error("Invalid SSE format: {0}")]
    InvalidSse(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Structured error returned when a requested capability is not supported.
#[derive(Debug, Clone, thiserror::Error)]
#[error("provider_capability_error provider={provider} capability={capability} message={message}")]
pub struct ProviderCapabilityError {
    pub provider: String,
    pub capability: String,
    pub message: String,
}

/// Provider capabilities declaration.
///
/// Describes what features a provider supports, enabling intelligent
/// adaptation of tool calling modes and request formatting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderCapabilities {
    /// Whether the provider supports native tool calling via API primitives.
    ///
    /// When `true`, the provider can convert tool definitions to API-native
    /// formats (e.g., Gemini's functionDeclarations, Anthropic's input_schema).
    ///
    /// When `false`, tools must be injected via system prompt as text.
    pub native_tool_calling: bool,
    /// Whether the provider supports vision / image inputs.
    pub vision: bool,
}

/// Provider-specific tool payload formats.
///
/// Different LLM providers require different formats for tool definitions.
/// This enum encapsulates those variations, enabling providers to convert
/// from the unified `ToolSpec` format to their native API requirements.
#[derive(Debug, Clone)]
pub enum ToolsPayload {
    /// Gemini API format (functionDeclarations).
    Gemini {
        function_declarations: Vec<serde_json::Value>,
    },
    /// Anthropic Messages API format (tools with input_schema).
    Anthropic { tools: Vec<serde_json::Value> },
    /// OpenAI Chat Completions API format (tools with function).
    OpenAI { tools: Vec<serde_json::Value> },
    /// Prompt-guided fallback (tools injected as text in system prompt).
    PromptGuided { instructions: String },
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Query provider capabilities.
    ///
    /// Default implementation returns minimal capabilities (no native tool calling).
    /// Providers should override this to declare their actual capabilities.
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    /// Convert tool specifications to provider-native format.
    ///
    /// Default implementation returns `PromptGuided` payload, which injects
    /// tool documentation into the system prompt as text. Providers with
    /// native tool calling support should override this to return their
    /// specific format (Gemini, Anthropic, OpenAI).
    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        ToolsPayload::PromptGuided {
            instructions: build_tool_instructions_text(tools),
        }
    }

    /// Simple one-shot chat (single user message, no explicit system prompt).
    ///
    /// This is the preferred API for non-agentic direct interactions.
    async fn simple_chat(
        &self,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        self.chat_with_system(None, message, model, temperature)
            .await
    }

    /// One-shot chat with optional system prompt.
    ///
    /// Kept for compatibility and advanced one-shot prompting.
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String>;

    /// Multi-turn conversation. Default implementation extracts the last user
    /// message and delegates to `chat_with_system`.
    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let system = messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.as_str());
        let last_user = messages
            .iter()
            .rfind(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("");
        self.chat_with_system(system, last_user, model, temperature)
            .await
    }

    /// Structured chat API for agent loop callers.
    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        // If tools are provided but provider doesn't support native tools,
        // inject tool instructions into system prompt as fallback.
        if let Some(tools) = request.tools {
            if !tools.is_empty() && !self.supports_native_tools() {
                let tool_instructions = match self.convert_tools(tools) {
                    ToolsPayload::PromptGuided { instructions } => instructions,
                    payload => {
                        anyhow::bail!(
                            "Provider returned non-prompt-guided tools payload ({payload:?}) while supports_native_tools() is false"
                        )
                    }
                };
                let mut modified_messages = request.messages.to_vec();

                // Inject tool instructions into an existing system message.
                // If none exists, prepend one to the conversation.
                if let Some(system_message) =
                    modified_messages.iter_mut().find(|m| m.role == "system")
                {
                    if !system_message.content.is_empty() {
                        system_message.content.push_str("\n\n");
                    }
                    system_message.content.push_str(&tool_instructions);
                } else {
                    modified_messages.insert(0, ChatMessage::system(tool_instructions));
                }

                let text = self
                    .chat_with_history(&modified_messages, model, temperature)
                    .await?;
                return Ok(ChatResponse {
                    text: Some(text),
                    tool_calls: Vec::new(),
                });
            }
        }

        let text = self
            .chat_with_history(request.messages, model, temperature)
            .await?;
        Ok(ChatResponse {
            text: Some(text),
            tool_calls: Vec::new(),
        })
    }

    /// Whether provider supports native tool calls over API.
    fn supports_native_tools(&self) -> bool {
        self.capabilities().native_tool_calling
    }

    /// Whether provider supports multimodal vision input.
    fn supports_vision(&self) -> bool {
        self.capabilities().vision
    }

    /// Warm up the HTTP connection pool (TLS handshake, DNS, HTTP/2 setup).
    /// Default implementation is a no-op; providers with HTTP clients should override.
    async fn warmup(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Chat with tool definitions for native function calling support.
    /// The default implementation falls back to chat_with_history and returns
    /// an empty tool_calls vector (prompt-based tool use only).
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[serde_json::Value],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let text = self.chat_with_history(messages, model, temperature).await?;
        Ok(ChatResponse {
            text: Some(text),
            tool_calls: Vec::new(),
        })
    }

    /// Whether provider supports streaming responses.
    /// Default implementation returns false.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Streaming chat with optional system prompt.
    /// Returns an async stream of text chunks.
    /// Default implementation falls back to non-streaming chat.
    fn stream_chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
        _options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        // Default: return an empty stream (not supported)
        stream::empty().boxed()
    }

    /// Streaming chat with history.
    /// Default implementation falls back to stream_chat_with_system with last user message.
    fn stream_chat_with_history(
        &self,
        _messages: &[ChatMessage],
        _model: &str,
        _temperature: f64,
        _options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        // For default implementation, we need to convert to owned strings
        // This is a limitation of the default implementation
        let provider_name = "unknown".to_string();

        // Create a single empty chunk to indicate not supported
        let chunk = StreamChunk::error(format!("{} does not support streaming", provider_name));
        stream::once(async move { Ok(chunk) }).boxed()
    }

    /// Parse a raw response once, caching intermediate results.
    ///
    /// This enables single-pass parsing for responses that may be accessed
    /// multiple times (e.g., text extraction followed by tool call extraction).
    /// The default implementation parses the raw string and caches the result.
    fn parse_response_once(&self, response: CachedResponse) -> CachedResponse {
        let mut response = response;
        response.parse_once();
        response
    }
}

/// Build tool instructions text for prompt-guided tool calling.
///
/// Generates a formatted text block describing available tools and how to
/// invoke them using XML-style tags. This is used as a fallback when the
/// provider doesn't support native tool calling.
pub fn build_tool_instructions_text(tools: &[ToolSpec]) -> String {
    let mut instructions = String::new();

    instructions.push_str("## Tool Use Protocol\n\n");
    instructions.push_str("To use a tool, wrap a JSON object in <tool_call></tool_call> tags:\n\n");
    instructions.push_str("<tool_call>\n");
    instructions.push_str(r#"{"name": "tool_name", "arguments": {"param": "value"}}"#);
    instructions.push_str("\n</tool_call>\n\n");
    instructions.push_str("You may use multiple tool calls in a single response. ");
    instructions.push_str("After tool execution, results appear in <tool_result> tags. ");
    instructions
        .push_str("Continue reasoning with the results until you can give a final answer.\n\n");
    instructions.push_str("### Available Tools\n\n");

    for tool in tools {
        writeln!(&mut instructions, "**{}**: {}", tool.name, tool.description)
            .expect("writing to String cannot fail");

        let parameters =
            serde_json::to_string(&tool.parameters).unwrap_or_else(|_| "{}".to_string());
        writeln!(&mut instructions, "Parameters: `{parameters}`")
            .expect("writing to String cannot fail");
        instructions.push('\n');
    }

    instructions
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CapabilityMockProvider;

    #[async_trait]
    impl Provider for CapabilityMockProvider {
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                native_tool_calling: true,
                vision: true,
            }
        }

        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("ok".into())
        }
    }

    #[test]
    fn chat_message_constructors() {
        let sys = ChatMessage::system("Be helpful");
        assert_eq!(sys.role, "system");
        assert_eq!(sys.content, "Be helpful");

        let user = ChatMessage::user("Hello");
        assert_eq!(user.role, "user");

        let asst = ChatMessage::assistant("Hi there");
        assert_eq!(asst.role, "assistant");

        let tool = ChatMessage::tool("{}");
        assert_eq!(tool.role, "tool");
    }

    #[test]
    fn chat_response_helpers() {
        let empty = ChatResponse {
            text: None,
            tool_calls: vec![],
        };
        assert!(!empty.has_tool_calls());
        assert_eq!(empty.text_or_empty(), "");

        let with_tools = ChatResponse {
            text: Some("Let me check".into()),
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "shell".into(),
                arguments: "{}".into(),
            }],
        };
        assert!(with_tools.has_tool_calls());
        assert_eq!(with_tools.text_or_empty(), "Let me check");
    }

    #[test]
    fn tool_call_serialization() {
        let tc = ToolCall {
            id: "call_123".into(),
            name: "file_read".into(),
            arguments: r#"{"path":"test.txt"}"#.into(),
        };
        let json = serde_json::to_string(&tc).unwrap();
        assert!(json.contains("call_123"));
        assert!(json.contains("file_read"));
    }

    #[test]
    fn conversation_message_variants() {
        let chat = ConversationMessage::Chat(ChatMessage::user("hi"));
        let json = serde_json::to_string(&chat).unwrap();
        assert!(json.contains("\"type\":\"Chat\""));

        let tool_result = ConversationMessage::ToolResults(vec![ToolResultMessage {
            tool_call_id: "1".into(),
            content: "done".into(),
        }]);
        let json = serde_json::to_string(&tool_result).unwrap();
        assert!(json.contains("\"type\":\"ToolResults\""));
    }

    #[test]
    fn provider_capabilities_default() {
        let caps = ProviderCapabilities::default();
        assert!(!caps.native_tool_calling);
        assert!(!caps.vision);
    }

    #[test]
    fn provider_capabilities_equality() {
        let caps1 = ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        };
        let caps2 = ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        };
        let caps3 = ProviderCapabilities {
            native_tool_calling: false,
            vision: false,
        };

        assert_eq!(caps1, caps2);
        assert_ne!(caps1, caps3);
    }

    #[test]
    fn supports_native_tools_reflects_capabilities_default_mapping() {
        let provider = CapabilityMockProvider;
        assert!(provider.supports_native_tools());
    }

    #[test]
    fn supports_vision_reflects_capabilities_default_mapping() {
        let provider = CapabilityMockProvider;
        assert!(provider.supports_vision());
    }

    #[test]
    fn tools_payload_variants() {
        // Test Gemini variant
        let gemini = ToolsPayload::Gemini {
            function_declarations: vec![serde_json::json!({"name": "test"})],
        };
        assert!(matches!(gemini, ToolsPayload::Gemini { .. }));

        // Test Anthropic variant
        let anthropic = ToolsPayload::Anthropic {
            tools: vec![serde_json::json!({"name": "test"})],
        };
        assert!(matches!(anthropic, ToolsPayload::Anthropic { .. }));

        // Test OpenAI variant
        let openai = ToolsPayload::OpenAI {
            tools: vec![serde_json::json!({"type": "function"})],
        };
        assert!(matches!(openai, ToolsPayload::OpenAI { .. }));

        // Test PromptGuided variant
        let prompt_guided = ToolsPayload::PromptGuided {
            instructions: "Use tools...".to_string(),
        };
        assert!(matches!(prompt_guided, ToolsPayload::PromptGuided { .. }));
    }

    #[test]
    fn build_tool_instructions_text_format() {
        let tools = vec![
            ToolSpec {
                name: "shell".to_string(),
                description: "Execute commands".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"}
                    }
                }),
            },
            ToolSpec {
                name: "file_read".to_string(),
                description: "Read files".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"}
                    }
                }),
            },
        ];

        let instructions = build_tool_instructions_text(&tools);

        // Check for protocol description
        assert!(instructions.contains("Tool Use Protocol"));
        assert!(instructions.contains("<tool_call>"));
        assert!(instructions.contains("</tool_call>"));

        // Check for tool listings
        assert!(instructions.contains("**shell**"));
        assert!(instructions.contains("Execute commands"));
        assert!(instructions.contains("**file_read**"));
        assert!(instructions.contains("Read files"));

        // Check for parameters
        assert!(instructions.contains("Parameters:"));
        assert!(instructions.contains(r#""type":"object""#));
    }

    #[test]
    fn build_tool_instructions_text_empty() {
        let instructions = build_tool_instructions_text(&[]);

        // Should still have protocol description
        assert!(instructions.contains("Tool Use Protocol"));

        // Should have empty tools section
        assert!(instructions.contains("Available Tools"));
    }

    // Mock provider for testing.
    struct MockProvider {
        supports_native: bool,
    }

    #[async_trait]
    impl Provider for MockProvider {
        fn supports_native_tools(&self) -> bool {
            self.supports_native
        }

        async fn chat_with_system(
            &self,
            _system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("response".to_string())
        }
    }

    #[test]
    fn provider_convert_tools_default() {
        let provider = MockProvider {
            supports_native: false,
        };

        let tools = vec![ToolSpec {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let payload = provider.convert_tools(&tools);

        // Default implementation should return PromptGuided.
        assert!(matches!(payload, ToolsPayload::PromptGuided { .. }));

        if let ToolsPayload::PromptGuided { instructions } = payload {
            assert!(instructions.contains("test_tool"));
            assert!(instructions.contains("A test tool"));
        }
    }

    #[tokio::test]
    async fn provider_chat_prompt_guided_fallback() {
        let provider = MockProvider {
            supports_native: false,
        };

        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Run commands".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let request = ChatRequest {
            messages: &[ChatMessage::user("Hello")],
            tools: Some(&tools),
        };

        let response = provider.chat(request, "model", 0.7).await.unwrap();

        // Should return a response (default impl calls chat_with_history).
        assert!(response.text.is_some());
    }

    #[tokio::test]
    async fn provider_chat_without_tools() {
        let provider = MockProvider {
            supports_native: true,
        };

        let request = ChatRequest {
            messages: &[ChatMessage::user("Hello")],
            tools: None,
        };

        let response = provider.chat(request, "model", 0.7).await.unwrap();

        // Should work normally without tools.
        assert!(response.text.is_some());
    }

    // Provider that echoes the system prompt for assertions.
    struct EchoSystemProvider {
        supports_native: bool,
    }

    #[async_trait]
    impl Provider for EchoSystemProvider {
        fn supports_native_tools(&self) -> bool {
            self.supports_native
        }

        async fn chat_with_system(
            &self,
            system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(system.unwrap_or_default().to_string())
        }
    }

    // Provider with custom prompt-guided conversion.
    struct CustomConvertProvider;

    #[async_trait]
    impl Provider for CustomConvertProvider {
        fn supports_native_tools(&self) -> bool {
            false
        }

        fn convert_tools(&self, _tools: &[ToolSpec]) -> ToolsPayload {
            ToolsPayload::PromptGuided {
                instructions: "CUSTOM_TOOL_INSTRUCTIONS".to_string(),
            }
        }

        async fn chat_with_system(
            &self,
            system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(system.unwrap_or_default().to_string())
        }
    }

    // Provider returning an invalid payload for non-native mode.
    struct InvalidConvertProvider;

    #[async_trait]
    impl Provider for InvalidConvertProvider {
        fn supports_native_tools(&self) -> bool {
            false
        }

        fn convert_tools(&self, _tools: &[ToolSpec]) -> ToolsPayload {
            ToolsPayload::OpenAI {
                tools: vec![serde_json::json!({"type": "function"})],
            }
        }

        async fn chat_with_system(
            &self,
            _system: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("should_not_reach".to_string())
        }
    }

    #[tokio::test]
    async fn provider_chat_prompt_guided_preserves_existing_system_not_first() {
        let provider = EchoSystemProvider {
            supports_native: false,
        };

        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Run commands".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let request = ChatRequest {
            messages: &[
                ChatMessage::user("Hello"),
                ChatMessage::system("BASE_SYSTEM_PROMPT"),
            ],
            tools: Some(&tools),
        };

        let response = provider.chat(request, "model", 0.7).await.unwrap();
        let text = response.text.unwrap_or_default();

        assert!(text.contains("BASE_SYSTEM_PROMPT"));
        assert!(text.contains("Tool Use Protocol"));
    }

    #[tokio::test]
    async fn provider_chat_prompt_guided_uses_convert_tools_override() {
        let provider = CustomConvertProvider;

        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Run commands".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let request = ChatRequest {
            messages: &[ChatMessage::system("BASE"), ChatMessage::user("Hello")],
            tools: Some(&tools),
        };

        let response = provider.chat(request, "model", 0.7).await.unwrap();
        let text = response.text.unwrap_or_default();

        assert!(text.contains("BASE"));
        assert!(text.contains("CUSTOM_TOOL_INSTRUCTIONS"));
    }

    #[tokio::test]
    async fn provider_chat_prompt_guided_rejects_non_prompt_payload() {
        let provider = InvalidConvertProvider;

        let tools = vec![ToolSpec {
            name: "shell".to_string(),
            description: "Run commands".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        }];

        let request = ChatRequest {
            messages: &[ChatMessage::user("Hello")],
            tools: Some(&tools),
        };

        let err = provider.chat(request, "model", 0.7).await.unwrap_err();
        let message = err.to_string();

        assert!(message.contains("non-prompt-guided"));
    }

    #[test]
    fn cached_response_parses_openai_response() {
        let raw = r#"{"choices":[{"message":{"content":"Hello world","tool_calls":[{"id":"call_123","function":{"name":"shell","arguments":"{\"cmd\":\"ls\"}"}}]}}]}"#;
        let mut cached = CachedResponse::new(raw);
        let response = cached.parse_once();
        assert_eq!(response.text.as_deref(), Some("Hello world"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "shell");
    }

    #[test]
    fn cached_response_parses_anthropic_response() {
        let raw = r#"{"content":[{"type":"text","text":"Hi there"},{"type":"tool_use","id":"call_456","name":"file_read","input":{"path":"/test.txt"}}]}"#;
        let mut cached = CachedResponse::new(raw);
        let response = cached.parse_once();
        assert_eq!(response.text.as_deref(), Some("Hi there"));
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "file_read");
    }

    #[test]
    fn cached_response_caches_on_subsequent_access() {
        let raw = r#"{"choices":[{"message":{"content":"cached text"}}]}"#;
        let mut cached = CachedResponse::new(raw);
        
        // First call parses
        let _ = cached.parse_once();
        
        // Second call should use cached
        let response = cached.parse_once();
        assert_eq!(response.text.as_deref(), Some("cached text"));
    }

    #[test]
    fn cached_response_text_method() {
        let raw = r#"{"choices":[{"message":{"content":"direct text"}}]}"#;
        let mut cached = CachedResponse::new(raw);
        assert_eq!(cached.text(), "direct text");
        // Second call should use cache
        assert_eq!(cached.text(), "direct text");
    }

    #[test]
    fn cached_response_tool_calls_method() {
        let raw = r#"{"choices":[{"message":{"tool_calls":[{"id":"call_1","function":{"name":"test","arguments":"{}"}}]}}]}"#;
        let mut cached = CachedResponse::new(raw);
        assert_eq!(cached.tool_calls().len(), 1);
        assert_eq!(cached.tool_calls()[0].name, "test");
        // Second call should use cache
        assert_eq!(cached.tool_calls().len(), 1);
    }
}
