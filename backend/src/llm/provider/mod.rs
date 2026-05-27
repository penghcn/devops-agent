//! Provider implementations and configuration.

pub mod anthropic;
pub mod base;
pub mod client;
pub mod config;
pub mod openai;
pub mod openai_compat;
pub use anthropic::{AnthropicAdapter, AnthropicProvider};
pub use base::{BaseConfig, GenericProvider, ProviderAdapter};
pub use config::{LlmConfigSnapshot, LlmConfigStore, ProviderConfig, build_model_router};
pub use openai::{OpenAIAdapter, OpenAIProvider};
pub use openai_compat::{
    DeepSeekAdapter, DeepSeekProvider, LLaMAAdapter, LLaMAProvider, NVIDIAAdapter, NVIDIAProvider,
    VLLMAdapter, VLLMProvider,
};
