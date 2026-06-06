use std::env;
use std::sync::Arc;

use agentik_core::agent::Agent;
use agent_kms::KmsContext;
use agentik_core::model::model_pool::ModelPool;
use agentik_core::provider::LlmProvider;
use agentik_core::provider::mimo::{MODEL_MIMO_V2_5, MimoProvider};
use types::messages::ContentBlock;

/// Integration test: Agent uses KMS tools to organize medical textbook content.
/// Run with: `MIMO_API_KEY=xxx cargo test -p agent -- kms_agent_flow --ignored`
#[tokio::test]
#[ignore]
async fn kms_agent_flow() {
    dotenvy::dotenv_override().ok();

    let mimo_provider = MimoProvider::new(None, None, None);
    let mimo_model = mimo_provider.get_model(MODEL_MIMO_V2_5).unwrap();

    let mut pool = ModelPool::new();
    pool.add_model(mimo_model);

    let ctx = Arc::new(KmsContext::from_path(&env::var("KMS_DB_PATH").unwrap_or_else(|_| "data/kms_sqlite.db".to_string())).await.unwrap());
    let mut agent = Agent::builder()
        .with_model_pool(Arc::new(pool))
        .with_context(ctx)
        .build()
        .await
        .unwrap();

    let chf_text = include_str!("chf.md");
    let ahf_text = include_str!("ahf.md"); // 急性心衰人卫教材章节内容，用于测试知识的增量联结能力

    // agent
    //     .inject_message(vec![ContentBlock::Text {
    //         text: format!("请你修复当前的诊断错误",),
    //     }])
    //     .unwrap();
    //
    // agent.start().await.unwrap();

    agent
        .inject_message(vec![ContentBlock::Text {
            text: format!(
                "请将以下医学文本整理到知识库中。要求：\n\
                1. 在 Root 下创建索引\"急性心力衰竭\"作为主题根节点\n\
                2. 根据文本结构建立子索引（如流行病学、临床表现、分期分级等），注意同层索引概念平级不重叠\n\
                3. 提取关键实体并创建，关联到对应索引节点\n\
                4. 将各章节的核心内容创建为knowledge条目，挂载到对应索引下\n\
                5. 每步操作后用kms_navigate移动到对应节点继续构建\n\
                文本如下：\n{}", ahf_text),
        }])
        .unwrap();

    agent.start().await.unwrap();
}
