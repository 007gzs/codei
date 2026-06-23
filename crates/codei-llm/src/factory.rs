use std::sync::Arc;

use codei_config::ResolvedConfig;

use crate::provider::{AnthropicProvider, LlmProvider, OpenAiProvider};
use crate::{LlmError, ToolFormat};

pub fn create_provider(config: &ResolvedConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    create_provider_by_name(config, &config.config.defaults.provider)
}

pub fn create_provider_by_name(
    config: &ResolvedConfig,
    provider_name: &str,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let provider_cfg = config.config.providers.get(provider_name).ok_or_else(|| {
        LlmError::ProviderNotConfigured {
            provider: provider_name.to_string(),
        }
    })?;

    let api_key = provider_cfg
        .resolve_api_key()
        .map_err(|err| match err {
            codei_config::ConfigError::MissingApiKey { env } => LlmError::MissingApiKey { env },
            other => LlmError::Config(other.to_string()),
        })?;

    let api_style = provider_cfg.api_style.as_deref().unwrap_or("openai");

    match api_style {
        "openai" => {
            let tool_format = ToolFormat::parse(provider_cfg.tool_format.as_deref());
            let openai = OpenAiProvider::from_config(
                provider_name.to_string(),
                api_key,
                provider_cfg.base_url.as_deref(),
                tool_format,
            )?;
            Ok(Arc::new(openai))
        }
        "anthropic" => {
            let anthropic = AnthropicProvider::from_config(
                provider_name.to_string(),
                api_key,
                provider_cfg.base_url.as_deref(),
            )?;
            Ok(Arc::new(anthropic))
        }
        other => Err(LlmError::UnsupportedProvider(other.to_string())),
    }
}
