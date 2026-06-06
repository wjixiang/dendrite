use std::sync::Arc;

use agentik_core::agent::Agent;
use agent_kms::KmsContext;
use agentik_core::model::model_pool::ModelPool;
use agentik_core::provider::LlmProvider;
use agentik_core::provider::mimo::{MODEL_MIMO_V2_5, MimoProvider};
use agentik_core::provider::minimax::{MODEL_MINIMAX_M2_7, MinimaxProvider};
use types::messages::ContentBlock;

/// Integration test that exercises a full agent workflow against a real LLM API.
///
/// Requires `MINIMAX_API_KEY` environment variable.
/// Run with: `cargo test -p agent -- basic_agent --ignored`
#[tokio::test]
#[ignore]
async fn basic_agent() {
    dotenvy::dotenv_override().ok();

    let api_key = std::env::var("MINIMAX_API_KEY").expect("MINIMAX_API_KEY must be set");

    let provider = MinimaxProvider::new(
        Some("https://api.minimaxi.com/anthropic".to_string()),
        Some(api_key),
        None,
    );

    let mimo_provider = MimoProvider::new(None, None, None);
    let mimo_model = mimo_provider.get_model(MODEL_MIMO_V2_5).unwrap();

    let model = provider.get_model(MODEL_MINIMAX_M2_7).unwrap();
    let mut pool = ModelPool::new();
    // pool.add_model(model);
    pool.add_model(mimo_model);

    let ctx = Arc::new(KmsContext::from_path("data/kms_sqlite.db").await.unwrap());
    let mut agent = Agent::builder()
        .with_model_pool(Arc::new(pool))
        .with_context(ctx)
        .build()
        .await
        .unwrap();

    agent
        .inject_message(vec![ContentBlock::Text {
            text:
                "write 10 poems one by one, each response only return one, no need to use any tool"
                    .to_string(),
        }])
        .unwrap();

    agent.start().await.unwrap();
}
