use super::{
    configured_data_dir, current_time_ms, fawx_data_dir, handle_headless_auth_command,
    handle_headless_keys_command, handle_headless_synthesis_command, headless_config_json,
    headless_config_path, headless_review_context, render_headless_config,
    sync_headless_model_from_config, CycleResult, HeadlessApp, ResultKind,
};
use crate::commands::slash::{
    client_only_command_message, config_reload_success_message, execute_command,
    init_default_config, parse_command, reload_runtime_config, render_budget_text,
    render_debug_dump, render_loop_status, render_signals_summary, CommandContext, CommandHost,
    ParsedCommand,
};
use crate::helpers::{
    available_provider_names, fetch_shared_available_models, read_router, render_model_menu_text,
    render_status_text, thinking_config_for_active_model,
};
use crate::proposal_review::{approve_pending, reject_pending, render_pending};
use fx_kernel::act::TokenUsage;
use fx_llm::ModelInfo;

pub(super) type HeadlessCommand = ParsedCommand;

pub(super) async fn process_command_input(
    app: &mut HeadlessApp,
    input: &str,
) -> Result<CycleResult, anyhow::Error> {
    app.last_session_messages.clear();
    let command = parse_headless_command(input);
    let response = match execute_headless_async_command(app, &command).await? {
        Some(response) => response,
        None => run_sync_command(app, &command)?,
    };
    Ok(command_cycle_result(app, response))
}

impl CommandHost for HeadlessApp {
    fn supports_embedded_slash_commands(&self) -> bool {
        true
    }

    fn list_models(&self) -> String {
        render_model_menu_text(Some(self.active_model.as_str()), &self.available_models())
    }

    fn set_active_model(&mut self, selector: &str) -> anyhow::Result<String> {
        HeadlessApp::set_active_model(self, selector)
    }

    fn proposals(&self, selector: Option<&str>) -> anyhow::Result<String> {
        render_pending(headless_review_context(&self.config), selector).map_err(anyhow::Error::new)
    }

    fn approve(&self, selector: &str, force: bool) -> anyhow::Result<String> {
        approve_pending(headless_review_context(&self.config), selector, force)
            .map_err(anyhow::Error::new)
    }

    fn reject(&self, selector: &str) -> anyhow::Result<String> {
        reject_pending(headless_review_context(&self.config), selector).map_err(anyhow::Error::new)
    }

    fn show_config(&self) -> anyhow::Result<String> {
        let config_path = headless_config_path(&self.config, self.config_manager.as_ref())?;
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        let json = headless_config_json(&self.config, self.config_manager.as_ref())?;
        render_headless_config(&config_path, &data_dir, &self.active_model, &json)
    }

    fn init_config(&mut self) -> anyhow::Result<String> {
        init_default_config(&fawx_data_dir())
    }

    fn reload_config(&mut self) -> anyhow::Result<String> {
        let config_path = headless_config_path(&self.config, self.config_manager.as_ref())?;
        self.config = reload_runtime_config(self.config_manager.as_ref(), &config_path)?;
        self.max_history = self.config.general.max_history;
        let thinking_budget = self.config.general.thinking.unwrap_or_default();
        sync_headless_model_from_config(self, self.config.model.default_model.clone())?;
        self.loop_engine
            .set_thinking_config(thinking_config_for_active_model(
                &thinking_budget,
                &self.active_model,
            ));
        Ok(config_reload_success_message(&config_path))
    }

    fn show_status(&self) -> String {
        let providers = read_router(&self.router, available_provider_names);
        render_status_text(
            &self.active_model,
            &providers,
            self.loop_engine.status(current_time_ms()),
        )
    }

    fn show_budget_status(&self) -> String {
        render_budget_text(self.loop_engine.status(current_time_ms()))
    }

    fn show_signals_summary(&self) -> String {
        render_signals_summary(&self.last_signals)
    }

    fn handle_thinking(&mut self, level: Option<&str>) -> anyhow::Result<String> {
        HeadlessApp::handle_thinking(self, level)
    }

    fn show_history(&self) -> anyhow::Result<String> {
        Ok(format!(
            "Conversation history: {} messages in current session",
            self.conversation_history.len()
        ))
    }

    fn new_conversation(&mut self) -> anyhow::Result<String> {
        self.conversation_history.clear();
        Ok("Started a new conversation.".to_string())
    }

    fn show_loop_status(&self) -> anyhow::Result<String> {
        Ok(render_loop_status(
            self.loop_engine.status(current_time_ms()),
        ))
    }

    fn show_debug(&self) -> anyhow::Result<String> {
        Ok(render_debug_dump(&self.last_signals))
    }

    fn handle_synthesis(&mut self, instruction: Option<&str>) -> anyhow::Result<String> {
        handle_headless_synthesis_command(&mut self.loop_engine, instruction)
    }

    fn handle_auth(
        &self,
        subcommand: Option<&str>,
        action: Option<&str>,
        value: Option<&str>,
        has_extra_args: bool,
    ) -> anyhow::Result<String> {
        read_router(&self.router, |router| {
            handle_headless_auth_command(router, subcommand, action, value, has_extra_args)
        })
    }

    fn handle_keys(
        &self,
        subcommand: Option<&str>,
        value: Option<&str>,
        option: Option<&str>,
        has_extra_args: bool,
    ) -> anyhow::Result<String> {
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        handle_headless_keys_command(&data_dir, subcommand, value, option, has_extra_args)
    }

    fn handle_sign(&self, target: Option<&str>, has_extra_args: bool) -> anyhow::Result<String> {
        let selection = crate::commands::skill_sign::parse_slash_selection(target, has_extra_args)?;
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        crate::commands::skill_sign::sign_output(selection, Some(&data_dir))
    }

    fn list_skills(&self) -> anyhow::Result<String> {
        crate::commands::marketplace::list_output()
    }

    fn install_skill(&self, name: &str) -> anyhow::Result<String> {
        let data_dir = configured_data_dir(&fawx_data_dir(), &self.config);
        crate::commands::marketplace::install_output(name, Some(&data_dir))
    }

    fn search_skills(&self, query: &str) -> anyhow::Result<String> {
        crate::commands::marketplace::search_output(query)
    }
}

impl HeadlessApp {
    async fn list_models_dynamic(&self) -> anyhow::Result<String> {
        let models = self.dynamic_models_or_fallback().await?;
        Ok(render_model_menu_text(
            Some(self.active_model.as_str()),
            &models,
        ))
    }

    async fn dynamic_models_or_fallback(&self) -> anyhow::Result<Vec<ModelInfo>> {
        let models = fetch_shared_available_models(&self.router).await;
        if models.is_empty() {
            return Ok(self.available_models());
        }
        Ok(models)
    }
}

fn parse_headless_command(input: &str) -> HeadlessCommand {
    parse_command(input)
}

fn run_sync_command(
    app: &mut HeadlessApp,
    command: &HeadlessCommand,
) -> Result<String, anyhow::Error> {
    match execute_command(&mut CommandContext { app }, command) {
        Some(result) => result.map(|value| value.response),
        None => Ok(client_only_command_message(command)
            .unwrap_or_else(|| "This command is only available in the TUI.".to_string())),
    }
}

async fn execute_headless_async_command(
    app: &mut HeadlessApp,
    command: &HeadlessCommand,
) -> Result<Option<String>, anyhow::Error> {
    match command {
        ParsedCommand::Model(None) => app.list_models_dynamic().await.map(Some),
        ParsedCommand::Analyze => app.analyze_signals_command().await.map(Some),
        ParsedCommand::Improve(flags) => app.improve_command(flags).await.map(Some),
        _ => Ok(None),
    }
}

fn command_cycle_result(app: &HeadlessApp, response: String) -> CycleResult {
    CycleResult {
        response,
        model: app.active_model().to_string(),
        iterations: 0,
        tokens_used: TokenUsage::default(),
        result_kind: ResultKind::Complete,
    }
}
