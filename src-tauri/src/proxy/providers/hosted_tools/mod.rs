//! Codex hosted tool bridge.
//!
//! 该模块只处理 OpenAI 托管工具在第三方上游中的本地桥接：第三方模型看到的
//! 永远是普通 function tool，本地代理负责调用真正的 OpenAI hosted tool。

pub(crate) mod bridge;
pub(crate) mod openai_client;
pub(crate) mod web_search;
