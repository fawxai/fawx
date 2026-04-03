use super::*;

pub(super) fn preferred_supported_budget(levels: &[String]) -> ThinkingBudget {
    for budget in [
        ThinkingBudget::High,
        ThinkingBudget::Adaptive,
        ThinkingBudget::Low,
        ThinkingBudget::Off,
    ] {
        if levels.iter().any(|level| level == &budget.to_string()) {
            return budget;
        }
    }
    ThinkingBudget::Off
}

#[cfg(feature = "http")]
pub(super) fn thinking_adjustment_reason(
    from: ThinkingBudget,
    to: ThinkingBudget,
    provider: Option<&str>,
) -> String {
    let provider = provider.unwrap_or("unknown");
    format!("{} not supported by {}; adjusted to {}", from, provider, to)
}

pub(super) fn handle_headless_synthesis_command(
    loop_engine: &mut LoopEngine,
    instruction: Option<&str>,
) -> anyhow::Result<String> {
    match instruction {
        None => Ok("Usage: /synthesis <instruction> or /synthesis reset".to_string()),
        Some(value) if value.trim().is_empty() => {
            Ok("Synthesis instruction cannot be empty.".to_string())
        }
        Some(value) if value.eq_ignore_ascii_case("reset") => {
            reset_synthesis_instruction(loop_engine)
        }
        Some(value) => update_headless_synthesis_instruction(loop_engine, value),
    }
}

pub(super) fn resolve_headless_model_selector(
    router: &ModelRouter,
    selector: &str,
) -> anyhow::Result<String> {
    let model_ids = router
        .available_models()
        .into_iter()
        .map(|model| model.model_id)
        .collect::<Vec<_>>();
    if model_ids.iter().any(|model_id| model_id == selector) {
        return Ok(selector.to_string());
    }

    resolve_model_alias(selector, &model_ids)
        .ok_or_else(|| anyhow::anyhow!("model not found: {selector}"))
}

pub(super) fn sync_headless_model_from_config(
    app: &mut HeadlessApp,
    default_model: Option<String>,
) -> anyhow::Result<()> {
    let resolved = read_router(&app.router, |router| {
        resolve_requested_model(router, default_model.as_deref())
    })?;
    apply_headless_active_model(app, &resolved);
    Ok(())
}

pub(super) fn apply_headless_active_model(app: &mut HeadlessApp, model: &str) {
    let error_message = write_router(&app.router, |router| {
        if let Err(error) = router.set_active(model) {
            tracing::warn!(error = %error, model, "failed to apply reloaded model to router");
            Some(format!("Model reload failed after config change: {error}"))
        } else {
            None
        }
    });

    if let Some(message) = error_message {
        app.record_error(ErrorCategory::System, message, true);
    }

    app.active_model = model.to_string();
    update_context_limit_for_active_model(app);
}

pub(super) fn update_context_limit_for_active_model(app: &mut HeadlessApp) {
    let context_window = read_router(&app.router, |router| {
        router
            .context_window_for_model(&app.active_model)
            .unwrap_or(128_000)
    });
    app.loop_engine.update_context_limit(context_window);
}

pub(super) fn active_model_thinking_levels(router: &SharedModelRouter, model: &str) -> Vec<String> {
    read_router(router, |shared_router| {
        shared_router
            .thinking_levels_for_model(model)
            .unwrap_or(&["off"])
            .iter()
            .map(|level| (*level).to_string())
            .collect()
    })
}

fn reset_synthesis_instruction(loop_engine: &mut LoopEngine) -> anyhow::Result<String> {
    loop_engine
        .set_synthesis_instruction(DEFAULT_SYNTHESIS_INSTRUCTION.to_string())
        .map_err(|error| anyhow::anyhow!(error.reason))?;
    Ok("Synthesis instruction reset to default.".to_string())
}

fn update_headless_synthesis_instruction(
    loop_engine: &mut LoopEngine,
    value: &str,
) -> anyhow::Result<String> {
    if value.len() > MAX_SYNTHESIS_INSTRUCTION_LENGTH {
        return Ok(format!(
            "Synthesis instruction exceeds {} characters.",
            MAX_SYNTHESIS_INSTRUCTION_LENGTH
        ));
    }

    loop_engine
        .set_synthesis_instruction(value.to_string())
        .map_err(|error| anyhow::anyhow!(error.reason))?;
    Ok(format!("Synthesis instruction updated: {}", value.trim()))
}
