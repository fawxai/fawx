use super::{command, output, *};

pub async fn process_input_with_commands(
    app: &mut HeadlessApp,
    input: &str,
    source: Option<&InputSource>,
) -> Result<CycleResult, anyhow::Error> {
    if is_command_input(input) {
        return command::process_command_input(app, input).await;
    }

    match source {
        Some(source) => app.process_message_for_source(input, source).await,
        None => app.process_message(input).await,
    }
}

pub async fn process_input_with_commands_streaming(
    app: &mut HeadlessApp,
    input: &str,
    source: Option<&InputSource>,
    callback: StreamCallback,
) -> Result<CycleResult, anyhow::Error> {
    if is_command_input(input) {
        let result = command::process_command_input(app, input).await?;
        callback(fx_kernel::StreamEvent::Done {
            response: result.response.clone(),
        });
        return Ok(result);
    }

    match source {
        Some(source) => {
            app.process_message_for_source_streaming(input, source, callback)
                .await
        }
        None => app.process_message_streaming(input, callback).await,
    }
}

pub(super) fn is_quit_command(input: &str) -> bool {
    matches!(input, "/quit" | "/exit")
}

impl HeadlessApp {
    pub async fn run(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();
        self.print_startup_info();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            let Some(input) = self
                .read_repl_input(&mut reader, &mut line, json_mode)
                .await?
            else {
                break;
            };
            if is_quit_command(&input) {
                break;
            }
            self.process_input(&input, json_mode).await?;
        }

        Ok(0)
    }

    pub async fn run_single(&mut self, json_mode: bool) -> Result<i32, anyhow::Error> {
        install_sigpipe_handler();
        self.apply_custom_system_prompt();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let input = self.parse_input_line(&line, json_mode)?;
        if input.is_empty() {
            return Ok(0);
        }

        self.process_input(&input, json_mode).await?;
        Ok(0)
    }

    async fn process_input(&mut self, input: &str, json_mode: bool) -> Result<(), anyhow::Error> {
        let result = self.process_message(input).await?;
        output::write_cycle_output(result, &self.last_session_messages, json_mode)
    }

    pub(super) fn parse_json_input(&self, raw: &str) -> Result<String, serde_json::Error> {
        let parsed: JsonInput = serde_json::from_str(raw)?;
        Ok(parsed.message)
    }

    async fn read_repl_input(
        &self,
        reader: &mut BufReader<tokio::io::Stdin>,
        line: &mut String,
        json_mode: bool,
    ) -> Result<Option<String>, anyhow::Error> {
        loop {
            line.clear();
            let bytes_read = reader.read_line(line).await?;
            if bytes_read == 0 {
                return Ok(None);
            }

            match self.parse_input_line(line, json_mode) {
                Ok(input) if input.is_empty() => continue,
                Ok(input) => return Ok(Some(input)),
                Err(error) => {
                    eprintln!("error: invalid JSON input: {error}");
                    continue;
                }
            }
        }
    }

    fn parse_input_line(&self, line: &str, json_mode: bool) -> Result<String, serde_json::Error> {
        if json_mode {
            return self.parse_json_input(line);
        }
        Ok(line.trim().to_string())
    }
}
