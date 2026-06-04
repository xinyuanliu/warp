use std::collections::HashMap;

use super::{
    has_long_context_usage, icon_for_context_window_usage, should_show_long_context_usage_warning,
};
use crate::ai::llms::{LLMContextWindow, LLMInfo, LLMProvider, LLMUsageMetadata};
use crate::persistence::model::ModelTokenUsage;
use crate::ui_components::icons::Icon;

fn model(provider: LLMProvider, is_configurable: bool) -> LLMInfo {
    LLMInfo {
        display_name: "test model".to_string(),
        base_model_name: "test model".to_string(),
        id: "test-model".into(),
        reasoning_level: None,
        usage_metadata: LLMUsageMetadata {
            request_multiplier: 1,
            credit_multiplier: None,
        },
        description: None,
        disable_reason: None,
        vision_supported: false,
        spec: None,
        provider,
        host_configs: HashMap::new(),
        discount_percentage: None,
        long_context_token_threshold: Some(272_000),
        context_window: LLMContextWindow {
            is_configurable,
            min: 200_000,
            max: 1_000_000,
            default_max: 272_000,
        },
    }
}

#[test]
fn warning_icon_uses_fully_lit_circle() {
    assert_eq!(
        icon_for_context_window_usage(0.0, true),
        Icon::ConversationContext100
    );
}

#[test]
fn context_window_icon_remains_percentage_based() {
    assert_eq!(
        icon_for_context_window_usage(0.0, false),
        Icon::ConversationContext0
    );
    assert_eq!(
        icon_for_context_window_usage(0.96, false),
        Icon::ConversationContext100
    );
}

#[test]
fn long_context_usage_does_not_match_display_name() {
    let active_model = model(LLMProvider::OpenAI, true);
    let model_usage = vec![
        ModelTokenUsage {
            model_id: "standard-model".to_string(),
            ..Default::default()
        },
        ModelTokenUsage {
            model_id: active_model.display_name.clone(),
            warp_tokens: 1,
            long_context_used: true,
            ..Default::default()
        },
    ];
    assert!(!has_long_context_usage(&model_usage, &active_model));
}

#[test]
fn long_context_usage_ignores_other_model_signal() {
    let active_model = model(LLMProvider::OpenAI, true);
    let model_usage = vec![ModelTokenUsage {
        model_id: "another model".to_string(),
        warp_tokens: 1,
        long_context_used: true,
        ..Default::default()
    }];

    assert!(!has_long_context_usage(&model_usage, &active_model));
}

#[test]
fn long_context_usage_matches_gpt54_xhigh_public_id_despite_different_names() {
    let mut active_model = model(LLMProvider::OpenAI, true);
    active_model.id = "gpt-5-4-xhigh".into();
    active_model.display_name = "gpt-5.4 (xhigh)".to_string();
    let model_usage = vec![ModelTokenUsage {
        model_id: "gpt-5-4-xhigh".to_string(),
        byok_tokens: 1,
        long_context_used: true,
        ..Default::default()
    }];

    assert!(has_long_context_usage(&model_usage, &active_model));
}

#[test]
fn long_context_usage_ignores_custom_endpoint_signal() {
    let active_model = model(LLMProvider::OpenAI, true);
    let model_usage = vec![ModelTokenUsage {
        model_id: active_model.display_name.clone(),
        custom_endpoint_tokens: 1,
        long_context_used: true,
        ..Default::default()
    }];
    assert!(!has_long_context_usage(&model_usage, &active_model));
}

#[test]
fn warning_requires_server_signal_and_configurable_openai_model() {
    let openai_model = model(LLMProvider::OpenAI, true);
    let model_usage = vec![ModelTokenUsage {
        model_id: openai_model.id.to_string(),
        warp_tokens: 1,
        long_context_used: true,
        ..Default::default()
    }];

    assert!(should_show_long_context_usage_warning(
        &model_usage,
        &openai_model
    ));
    assert!(!should_show_long_context_usage_warning(
        &model_usage,
        &model(LLMProvider::Anthropic, true)
    ));
    assert!(!should_show_long_context_usage_warning(
        &model_usage,
        &model(LLMProvider::OpenAI, false)
    ));
    assert!(!should_show_long_context_usage_warning(
        &[],
        &model(LLMProvider::OpenAI, true)
    ));
}

#[test]
fn warning_ignores_long_context_signal_from_other_model() {
    let active_model = model(LLMProvider::OpenAI, true);
    let model_usage = vec![ModelTokenUsage {
        model_id: "Gemini 3 Pro".to_string(),
        warp_tokens: 1,
        long_context_used: true,
        ..Default::default()
    }];

    assert!(!should_show_long_context_usage_warning(
        &model_usage,
        &active_model
    ));
}
