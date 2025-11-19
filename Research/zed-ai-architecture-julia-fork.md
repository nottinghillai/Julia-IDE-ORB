# Zed AI Architecture: Foundation for Agentic Code Editing and the Path to Julia

## Abstract

Zed represents a paradigm shift in code editor architecture, built from the ground up in Rust with GPU-accelerated rendering and native AI integration. Unlike traditional editors that bolt AI features onto existing frameworks, Zed's AI capabilities are architected as first-class components within the editor's core. This paper examines Zed's AI architecture, its current capabilities for agentic code editing, and outlines a concrete implementation path for creating a Julia-style fork that extends Zed's foundation with per-agent memory, local model orchestration, and enhanced automation workflows.

---

## 1. Architectural Foundation

### 1.1 Core Design Philosophy

Zed's architecture is fundamentally different from editors like VS Code or Cursor. Rather than being a fork of Electron-based tooling, Zed is written from scratch in Rust with a focus on performance and native integration. The editor leverages GPU acceleration (DirectX/Vulkan) for rendering, creating a multi-buffer core that can handle complex AI interactions without performance degradation.

The workspace is organized into purpose-built crates that separate concerns cleanly: `agent`, `agent_ui`, `assistant_text_thread`, `language_models`, `ollama`, `open_ai`, `rules_library`, and others. This modular structure means the entire AI stack—from UI components to protocol handlers to provider bridges—lives within the main repository and can be modified directly without external dependencies.

### 1.2 AI as First-Class Citizen

Unlike editors where AI features are add-ons, Zed's AI capabilities are integrated at the architectural level. The Agent Panel, Inline Assistant, Edit Prediction, and tool system are all native Rust components that interact directly with the editor's core. This design enables:

- **Transparency**: Users can see and edit the exact prompts and context being sent to models
- **Control**: Full access to conversation history, message editing, and tool call inspection
- **Extensibility**: The agent system is built on open protocols (ACP, MCP) that allow custom extensions

---

## 2. Current AI Capabilities

### 2.1 Agent Panel: The Conversational Interface

The Agent Panel represents Zed's primary interface for AI interaction. It functions as a full text editor for conversations, not merely a chat interface. This design choice enables several powerful features:

**Editable Conversations**: Every message in a thread can be revised, creating a checkpoint system where users can iterate on prompts and see how changes affect responses. The panel tracks the agent's cursor position and provides visual and audio notifications when responses complete.

**Context Management**: Context is added through @-mentions that can reference files, directories, rules, prior threads, or selected text. The UI provides token usage indicators and context-window warnings, giving users visibility into the computational cost of their interactions.

**Multi-Model Support**: Users can swap models mid-thread, pin specific models for different features (inline assistant, summaries, commits), and configure global temperature settings. This flexibility is essential for workflows that require different models for different tasks.

### 2.2 Smart Edit Mechanisms

Zed provides multiple pathways for AI-driven code editing:

**Inline Assistant**: Invoked directly within any buffer, the Inline Assistant sends selected code plus an instruction to the chosen model. It supports multiple cursors and can fan out to multiple models simultaneously for comparison. The assistant applies edits as inline replacements, maintaining the user's workflow context.

**Agentic Editing**: The newer Agent Panel supports multi-step reasoning where agents can call tools to inspect the project tree, read multiple files, plan changes, and apply edits via structured patches. This creates a review workflow where users see per-file diff drawers and can accept or reject individual hunks or entire batches.

**Edit Prediction**: Zed's proprietary Zeta model provides streaming code suggestions on each keystroke, complementing the deliberate prompt-and-review flow of the Agent Panel. This dual approach—predictive and agentic—covers both rapid iteration and thoughtful refactoring.

### 2.3 Tool System and Extensibility

Zed's built-in tools span diagnostics, search, filesystem operations (CRUD), file editing, and terminal access. These are encoded as `AgentTool` implementations that models can call through standard function calling protocols.

The system supports external extensions through:
- **Model Context Protocol (MCP) servers**: Can be installed as extensions or custom commands, allowing domain-specific tools and workflows
- **Agent Context Protocol (ACP) agents**: External agents like Gemini CLI or Claude Code can run within Zed with their own authentication flows
- **Custom agent servers**: Any command can be registered as an agent server, enabling bespoke integrations

Rules provide another layer of extensibility. Projects can define `.rules`, `.cursorrules`, or `CLAUDE.md` files that preload per-project instructions. The agent crate has these filenames baked in, so every interaction automatically inherits project-specific guidance.

---

## 3. Local and Offline Model Support

### 3.1 Provider Abstraction

Zed's provider system is designed for flexibility. Settings accept API keys for hosted providers (Anthropic, OpenAI, Google, GitHub Copilot) and also support custom `openai_compatible` providers that take a custom `api_url` plus model metadata. This abstraction means any OpenAI-style gateway—whether running on localhost, LAN, or remote infrastructure—can service the Agent Panel.

### 3.2 Offline Workflows

For truly offline operation, Zed supports:
- **Ollama integration**: The `ollama` crate defines HTTP transport, default context windows (clamped to 16k for 16GB machines), and model capability detection (tools, vision, thinking). Users can pull local models, configure `max_tokens` and `keep_alive`, and authenticate against remote Ollama servers if needed.
- **Custom gateways**: Any service implementing `/v1/chat/completions` can be configured, enabling Flask/MLX/Docker Model Runner setups that keep all LLM calls on the local machine or LAN.

The architecture separates "network-offline" (disabling outbound calls) from "local models" (self-hosted LLMs), giving users granular control over where computation happens.

---

## 4. Current Limitations and Gaps

### 4.1 Workflow Friction

While Zed's AI integration is powerful, it requires more manual steps than fully automated systems like Cursor's Composer. The typical workflow involves:
1. Discussing changes in the Agent Panel
2. Moving to the target file
3. Selecting code
4. Invoking the Inline Assistant
5. Reviewing and accepting changes

This multi-step process, while transparent and controllable, creates friction for users accustomed to "chat applies instantly" workflows. The Agent Panel's newer tool-based editing reduces this friction, but still requires explicit accept/reject steps.

### 4.2 Memory and Persistence

Zed does not currently provide persistent per-agent memory or vector database integration. Each conversation thread is independent, and there's no built-in mechanism for agents to remember past interactions, learn from project history, or maintain long-term context across sessions. This is a significant gap for workflows that require agents to build understanding over time.

### 4.3 External Agent Limitations

The Inline Assistant cannot invoke external ACP agents, limiting offline "smart edits" to models configured through Zed's provider abstraction. External agents also lack some features like checkpoints and message editing that are available in the native Agent Panel.

---

## 5. Implementation Path for Julia-Style Fork

### 5.1 Architecture Overview

A Julia-style fork would extend Zed's foundation with three key additions:
1. **Local LLM Orchestration**: A gateway service that manages local models (Julia, DeepSeek, MLX) and exposes them via OpenAI-compatible endpoints
2. **Per-Agent Memory System**: A vector database (SQLite-vec, Qdrant, or Chroma) that maintains context per agent and per session
3. **Enhanced Automation**: Workflows that reduce friction while maintaining Zed's transparency and control

### 5.2 Component Breakdown

**Local LLM Gateway**: The gateway would run as a separate service (Flask, FastAPI, or Rust-based) that implements `/v1/chat/completions`. It would handle model loading, context management, and tool calling. The gateway would be configured in Zed's `settings.json` as an `openai_compatible` provider, making it transparent to the editor.

**Memory Store**: A vector database keyed by `agent_id`, `session_id`, and `resource_type` (file, note, document, previous tasks). This would be exposed to agents through custom MCP tools (`memory.search`, `memory.write`, `memory.tag_session`) that allow agents to retrieve relevant past context and store new information.

**Agent Server Extension**: A custom ACP agent server that implements Julia-specific behaviors:
- System prompts that encode Julia's persona and tool usage rules
- Safety and style rules for edits (don't break tests, prefer diff-style edits)
- Integration with the memory store for context retrieval
- Custom tools for domain-specific workflows

**UI Enhancements**: While Zed's Agent Panel provides a solid base, Julia-specific enhancements could include:
- Agent profile switching (clicking "agent bubbles" switches profiles)
- Visual indicators for memory retrieval and context injection
- Streamlined accept/reject workflows for common edit patterns

### 5.3 Implementation Strategy

The beauty of Zed's architecture is that most Julia-specific functionality can be built outside the core editor:

1. **Phase 1: Local Gateway + Basic Agent**
   - Set up OpenAI-compatible gateway with local models
   - Configure Zed to use the gateway
   - Create basic Julia agent profile with system prompt

2. **Phase 2: Memory Integration**
   - Implement vector database with per-agent/session keys
   - Create MCP server with memory tools
   - Integrate memory retrieval into agent prompts

3. **Phase 3: Enhanced Automation**
   - Build custom ACP agent with Julia-specific tools
   - Implement streamlined edit workflows
   - Add UI enhancements for agent switching and memory visualization

4. **Phase 4: Core Modifications (Optional)**
   - If deeper integration is needed, modify relevant crates (`agent`, `agent_ui`, `assistant_text_thread`)
   - Add native support for per-agent memory in the editor core
   - Enhance Inline Assistant to support external ACP agents

### 5.4 Key Advantages of This Approach

Building on Zed's foundation provides several advantages:
- **Open Source**: The entire AI stack is GPL-licensed, allowing full modification
- **Modularity**: Most functionality can be added via extensions without forking core
- **Transparency**: Users maintain visibility into all AI operations
- **Performance**: GPU-accelerated rendering ensures AI interactions don't degrade editor performance
- **Standards-Based**: ACP and MCP protocols ensure compatibility with ecosystem tools

---

## 6. Conclusion

Zed's AI architecture provides a robust foundation for building agentic code editing experiences. Its modular design, first-class AI integration, and support for local models make it an ideal base for specialized forks like Julia. While current workflows require more manual steps than fully automated systems, the transparency and control they provide are valuable trade-offs.

The path to a Julia-style fork is clear: leverage Zed's provider abstraction for local models, extend through MCP/ACP protocols for memory and custom tools, and enhance the UI incrementally. Most functionality can be added without modifying core editor code, making the fork maintainable and allowing it to benefit from upstream improvements.

The key insight is that Zed's architecture separates concerns effectively: the editor provides the infrastructure (rendering, buffers, UI), while AI behavior lives in configurable, extensible layers. This design enables specialized agents like Julia to be built as extensions rather than requiring a complete rewrite, making the vision of "Julia-as-a-child" not just possible, but practical.

---

## References

- Zed Repository: Architecture and crate structure
- Zed Documentation: Agent Panel, Inline Assistant, Tools, MCP/ACP protocols
- Model Context Protocol: Standard for agent-editor communication
- Agent Context Protocol: Extension protocol for external agents



