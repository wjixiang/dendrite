# llm_api

Comprehensive, type-safe Rust SDK for the Anthropic API.

This project forks from https://github.com/dimichgh/anthropic-sdk-rust

## Features

- Streaming responses (SSE)
- Tool use / function calling
- Vision / image input
- File uploads
- Batch processing
- Async/await based
- **Agent module** (behind `agent` feature): provider abstraction, model pool, Anthropic-compatible provider implementations

## Usage

```toml
[dependencies]
llm_api = "0.3"
```

```rust
use llm_api::Anthropic;

let client = Anthropic::new("your-api-key")?;
let response = client.messages().create(&request).await?;
```

## Agent Feature

Enable the `agent` feature for provider abstraction and model management:

```toml
[dependencies]
llm_api = { version = "0.3", features = ["agent"] }
```

This adds:
- `Role::System` and `Role::Tool` for internal agent bookkeeping
- `Message` constructors: `system()`, `user()`, `assistant_text()`, `tool_result()`, etc.
- `agent::model::{Model, ModelInfo}`
- `agent::model::model_pool::{ModelPool, ModelPoolError}`
- `agent::client::{ApiClient, AnthropicApiClient, MockApiClient}`
- `agent::provider::{LlmProvider, ProviderInfo, ProviderError}`
- `agent::minimax::{MinimaxProvider, MODEL_MINIMAX_M2_7}`
