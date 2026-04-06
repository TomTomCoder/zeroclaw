# Repository Analysis: ZeroClaw

## Overview

ZeroClaw is a Rust-first autonomous agent runtime designed for high performance, small binary size, and extensibility. It's a CLI tool built with Rust 1.87+ that provides AI assistant capabilities.

## Project Structure

```
zeroclaw/
├── src/
│   ├── agent/          # Orchestration loop
│   ├── providers/     # LLM provider integrations
│   ├── channels/      # Messaging integrations
│   ├── tools/         # Tool execution surface
│   ├── memory/        # Memory backends
│   ├── security/      # Policy, pairing, secrets
│   ├── gateway/       # HTTP webhook server
│   ├── runtime/       # Runtime adapters
│   ├── config/        # Configuration system
│   └── ...
├── docs/              # Documentation
├── scripts/           # Bootstrap scripts
└── ...
```

## Key Components

### Agent (`src/agent/`)
- `agent.rs` - Main agent implementation
- `loop_.rs` - Agent loop
- `dispatcher.rs` - Command dispatch
- `classifier.rs` - Message classification
- `prompt.rs` - Prompt management

### Providers (`src/providers/`)
- LLM provider integrations (OpenAI, Anthropic, etc.)
- Factory pattern for provider selection

### Channels (`src/channels/`)
- Telegram, Discord, Slack, Mattermost
- Matrix, Signal, WhatsApp
- Email, IRC, CLI

### Tools (`src/tools/`)
- Shell, file, memory tools
- Browser, git, cron tools

### Memory (`src/memory/`)
- SQLite hybrid search
- PostgreSQL backend
- Markdown files
- Vector embeddings

### Security (`src/security/`)
- Pairing system
- Policy enforcement
- Secrets management
- Sandboxing (firejail, bubblewrap, landlock)

### Gateway (`src/gateway/`)
- Axum-based HTTP server
- Webhook endpoints
- Pairing flow

## Architecture

The project uses a trait-driven modular architecture:

| Subsystem | Trait | Implementation |
|-----------|-------|----------------|
| AI Models | Provider | 29+ built-in providers |
| Channels | Channel | 15+ integrations |
| Memory | Memory | SQLite, PostgreSQL, Markdown |
| Tools | Tool | shell, file, browser, etc. |
| Runtime | RuntimeAdapter | native, docker |

## Build Targets

- Binary size: ~8.8 MB
- RAM footprint: <5 MB
- Supports: Linux (x86_64, aarch64, armv7), macOS (x86_64, aarch64), Windows (x86_64)

## Key Features

1. **Lean Runtime**: Small binary, fast startup, low memory
2. **Secure by Default**: Pairing, sandboxing, allowlists
3. **Swappable Core**: Trait-based architecture
4. **Multi-platform**: ARM, x86, RISC-V support

## Dependencies

Notable dependencies:
- `axum` - HTTP server
- `tokio` - Async runtime
- `serde` - Serialization
- `tracing` - Logging
- `sqlx` - Database
- `telegram-bot` - Telegram support
- `serenity` - Discord support