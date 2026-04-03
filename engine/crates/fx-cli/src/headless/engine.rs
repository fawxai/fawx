use super::*;

impl HeadlessApp {
    pub async fn process_message(&mut self, input: &str) -> Result<CycleResult, anyhow::Error> {
        let source = InputSource::Text;
        self.process_message_for_source(input, &source).await
    }

    pub async fn process_message_streaming(
        &mut self,
        input: &str,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        let source = InputSource::Text;
        self.process_message_for_source_streaming(input, &source, callback)
            .await
    }

    pub async fn process_message_for_source(
        &mut self,
        input: &str,
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result(input, source).await
    }

    pub async fn process_message_with_attachments(
        &mut self,
        input: &str,
        images: &[ImageAttachment],
        documents: &[DocumentAttachment],
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result_with_attachments(input, images, documents, source, None)
            .await
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub async fn process_message_with_images(
        &mut self,
        input: &str,
        images: &[ImageAttachment],
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.process_message_with_attachments(input, images, &[], source)
            .await
    }

    pub async fn process_message_with_context(
        &mut self,
        input: &str,
        images: Vec<ImageAttachment>,
        documents: Vec<DocumentAttachment>,
        context: Vec<Message>,
        source: &InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<(CycleResult, Vec<Message>), anyhow::Error> {
        let original_history = std::mem::replace(&mut self.conversation_history, context);
        let result = match (images.is_empty() && documents.is_empty(), callback) {
            (true, Some(callback)) => {
                process_input_with_commands_streaming(self, input, Some(source), callback).await
            }
            (true, None) => process_input_with_commands(self, input, Some(source)).await,
            (false, _) => {
                self.process_message_with_attachments(input, &images, &documents, source)
                    .await
            }
        };
        let updated_history = self.conversation_history.clone();
        self.conversation_history = original_history;
        result.map(|cycle| (cycle, updated_history))
    }

    pub async fn process_message_for_source_streaming(
        &mut self,
        input: &str,
        source: &InputSource,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result_streaming(input, source, callback)
            .await
    }

    #[cfg(test)]
    pub(super) fn finalize_cycle(&mut self, input: &str, result: &LoopResult) -> CycleResult {
        let timestamp = current_epoch_secs();
        self.finalize_cycle_with_turn_messages(
            input,
            result,
            FinalizeTurnContext {
                images: &[],
                documents: &[],
                collector: None,
                user_timestamp: timestamp,
                assistant_timestamp: timestamp,
            },
        )
    }

    fn finalize_cycle_with_turn_messages(
        &mut self,
        input: &str,
        result: &LoopResult,
        context: FinalizeTurnContext<'_>,
    ) -> CycleResult {
        let response = extract_response_text(result);
        let result_kind = extract_result_kind(result);
        let iterations = extract_iterations(result);
        let tokens_used = extract_token_usage(result);
        self.cumulative_tokens.input_tokens = self
            .cumulative_tokens
            .input_tokens
            .saturating_add(tokens_used.input_tokens);
        self.cumulative_tokens.output_tokens = self
            .cumulative_tokens
            .output_tokens
            .saturating_add(tokens_used.output_tokens);
        self.last_signals = result.signals().to_vec();
        let signals = self.last_signals.clone();
        persist_headless_signals(self, &signals);
        let session_messages = build_turn_messages(input, context, &response);
        self.record_session_turn_messages(session_messages);
        CycleResult {
            response,
            model: self.active_model.clone(),
            iterations,
            tokens_used,
            result_kind,
        }
    }

    async fn run_cycle_result(
        &mut self,
        input: &str,
        source: &InputSource,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result_with_attachments(input, &[], &[], source, None)
            .await
    }

    async fn run_cycle_result_streaming(
        &mut self,
        input: &str,
        source: &InputSource,
        callback: StreamCallback,
    ) -> Result<CycleResult, anyhow::Error> {
        self.run_cycle_result_with_attachments(input, &[], &[], source, Some(callback))
            .await
    }

    async fn run_cycle_result_with_attachments(
        &mut self,
        input: &str,
        images: &[ImageAttachment],
        documents: &[DocumentAttachment],
        source: &InputSource,
        callback: Option<StreamCallback>,
    ) -> Result<CycleResult, anyhow::Error> {
        self.last_session_messages.clear();
        let user_timestamp = current_epoch_secs();
        let execution = self.prepare_cycle_execution(input, callback);
        let result = self
            .execute_cycle(input, images, documents, source, &execution)
            .await?;
        let assistant_timestamp = current_epoch_secs();
        self.set_stream_callback(None);
        self.evaluate_canary(&result);
        Ok(self.finalize_cycle_with_turn_messages(
            input,
            &result,
            FinalizeTurnContext {
                images,
                documents,
                collector: Some(&execution.collector),
                user_timestamp,
                assistant_timestamp,
            },
        ))
    }

    pub(super) fn apply_custom_system_prompt(&mut self) {
        if self.custom_system_prompt.is_some() {
            self.update_memory_context("");
        }
    }

    fn update_memory_context(&mut self, input: &str) {
        let mut context_parts: Vec<String> = Vec::new();
        if let Some(prompt) = &self.custom_system_prompt {
            context_parts.push(prompt.clone());
        }
        if let Some(mem) = self.relevant_memory_context(input) {
            context_parts.push(mem);
        }
        self.loop_engine
            .set_memory_context(context_parts.join("\n\n"));
    }

    #[cfg(test)]
    pub(super) fn build_perception_snapshot(
        &self,
        input: &str,
        source: &InputSource,
    ) -> PerceptionSnapshot {
        self.build_perception_snapshot_with_attachments(input, source, &[], &[])
    }

    #[cfg(test)]
    pub(super) fn record_turn(&mut self, user_text: &str, assistant_text: &str) {
        let timestamp = current_epoch_secs();
        self.record_session_turn_messages(text_turn_messages(
            user_text,
            assistant_text,
            timestamp,
            timestamp,
        ));
    }

    pub(super) fn record_session_turn_messages(&mut self, session_messages: Vec<SessionMessage>) {
        self.last_session_messages = session_messages.clone();
        self.conversation_history
            .extend(session_messages.iter().map(SessionMessage::to_llm_message));
        trim_history(&mut self.conversation_history, self.max_history);
    }

    fn set_stream_callback(&self, callback: Option<fx_kernel::streaming::StreamCallback>) {
        if let Ok(mut guard) = self.stream_callback_slot.lock() {
            *guard = callback;
        }
    }

    fn evaluate_canary(&mut self, result: &LoopResult) {
        let Some(monitor) = self.canary_monitor.as_mut() else {
            return;
        };
        if let Some(verdict) = monitor.on_cycle_complete(result.signals().to_vec()) {
            tracing::info!(?verdict, "canary verdict");
        }
    }

    fn relevant_memory_context(&self, input: &str) -> Option<String> {
        let entries = self.search_memory_entries(input)?;
        format_memory_for_prompt(&entries, self.config.memory.max_snapshot_chars)
    }

    fn search_memory_entries(&self, input: &str) -> Option<Vec<(String, String)>> {
        let memory = self.memory.as_ref()?;
        match memory.lock() {
            Ok(store) => {
                let max = self.config.memory.max_relevant_results;
                Some((*store).search_relevant(input, max))
            }
            Err(error) => {
                eprintln!("warning: failed to lock memory store: {error}");
                None
            }
        }
    }

    fn build_perception_snapshot_with_attachments(
        &self,
        input: &str,
        source: &InputSource,
        images: &[ImageAttachment],
        documents: &[DocumentAttachment],
    ) -> PerceptionSnapshot {
        let timestamp_ms = current_time_ms();
        let image_pairs = images.to_vec();
        let document_pairs = documents.to_vec();
        PerceptionSnapshot {
            screen: ScreenState {
                current_app: "fawx.headless".to_string(),
                elements: Vec::new(),
                text_content: input.to_string(),
            },
            notifications: Vec::new(),
            active_app: "fawx.headless".to_string(),
            timestamp_ms,
            sensor_data: None,
            user_input: Some(UserInput {
                text: input.to_string(),
                source: source.clone(),
                timestamp: timestamp_ms,
                context_id: None,
                images: image_pairs,
                documents: document_pairs,
            }),
            conversation_history: self.conversation_history.clone(),
            steer_context: None,
        }
    }

    fn prepare_cycle_execution(
        &mut self,
        input: &str,
        callback: Option<StreamCallback>,
    ) -> CycleExecutionContext {
        let callback = callback.map(headless_stream_callback);
        let collector = SessionTurnCollector::default();
        let combined_callback = collector.callback(callback.clone());
        self.set_stream_callback(Some(Arc::clone(&combined_callback)));
        self.emit_cycle_startup_warnings(callback.is_some(), &combined_callback);
        self.update_memory_context(input);
        CycleExecutionContext {
            collector,
            callback: combined_callback,
        }
    }

    fn emit_cycle_startup_warnings(&mut self, streaming: bool, combined_callback: &StreamCallback) {
        if streaming {
            self.emit_startup_warnings(Some(combined_callback));
        } else {
            self.clear_startup_warnings();
        }
    }

    async fn execute_cycle(
        &mut self,
        input: &str,
        images: &[ImageAttachment],
        documents: &[DocumentAttachment],
        source: &InputSource,
        execution: &CycleExecutionContext,
    ) -> Result<LoopResult, anyhow::Error> {
        let snapshot =
            self.build_perception_snapshot_with_attachments(input, source, images, documents);
        let llm = RecordingLoopLlmProvider::new(
            RouterLoopLlmProvider::new(Arc::clone(&self.router), self.active_model.clone()),
            execution.collector.clone(),
        );
        self.loop_engine
            .run_cycle_streaming(snapshot, &llm, Some(Arc::clone(&execution.callback)))
            .await
            .map_err(|error| {
                anyhow::anyhow!("loop error: stage={} reason={}", error.stage, error.reason)
            })
    }
}

struct CycleExecutionContext {
    collector: SessionTurnCollector,
    callback: StreamCallback,
}

fn build_turn_messages(
    input: &str,
    context: FinalizeTurnContext<'_>,
    response: &str,
) -> Vec<SessionMessage> {
    context
        .collector
        .map(|collector| {
            collector.session_messages_for_turn(
                input,
                context.images,
                context.documents,
                response,
                context.user_timestamp,
                context.assistant_timestamp,
            )
        })
        .unwrap_or_else(|| {
            text_turn_messages(
                input,
                response,
                context.user_timestamp,
                context.assistant_timestamp,
            )
        })
}
