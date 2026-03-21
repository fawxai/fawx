use std::sync::Arc;

use async_trait::async_trait;
use fx_kernel::act::ToolCacheability;
use fx_kernel::cancellation::CancellationToken;
use fx_llm::ToolDefinition;
use serde::Deserialize;

use crate::skill::{Skill, SkillError};

const DEFAULT_NOTIFICATION_TITLE: &str = "Fawx";

#[async_trait]
pub trait NotificationSender: Send + Sync + std::fmt::Debug {
    async fn send(&self, title: &str, body: &str) -> Result<(), String>;
}

#[derive(Debug)]
pub struct NotifySkill {
    sender: Arc<dyn NotificationSender>,
}

#[derive(Debug, Deserialize)]
struct NotifyArgs {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

impl NotifySkill {
    #[must_use]
    pub fn new(sender: Arc<dyn NotificationSender>) -> Self {
        Self { sender }
    }

    async fn handle_notify(&self, arguments: &str) -> Result<String, SkillError> {
        let args: NotifyArgs = serde_json::from_str(arguments)
            .map_err(|error| format!("Invalid arguments: {error}"))?;
        let title = args
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .unwrap_or(DEFAULT_NOTIFICATION_TITLE);
        let body = args
            .body
            .map(|body| body.trim().to_string())
            .filter(|body| !body.is_empty())
            .ok_or_else(|| "Missing required field 'body'".to_string())?;

        self.sender.send(title, &body).await?;
        Ok("Notification sent".to_string())
    }
}

fn notify_definition() -> ToolDefinition {
    ToolDefinition {
        name: "notify".to_string(),
        description: concat!(
            "Send a native OS notification to the user. Use when completing a task ",
            "the user is waiting for, reporting important results, or when the app ",
            "may not be in focus. Do not use for trivial acknowledgements."
        )
        .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short notification title (1-2 words or a short phrase)"
                },
                "body": {
                    "type": "string",
                    "description": "Notification body with the key details"
                }
            },
            "required": ["body"]
        }),
    }
}

#[async_trait]
impl Skill for NotifySkill {
    fn name(&self) -> &str {
        "notify_skill"
    }

    fn description(&self) -> &str {
        "Sends native notifications to connected clients."
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![notify_definition()]
    }

    fn capabilities(&self) -> Vec<String> {
        vec!["notifications".to_string()]
    }

    fn cacheability(&self, tool_name: &str) -> ToolCacheability {
        match tool_name {
            "notify" => ToolCacheability::SideEffect,
            _ => ToolCacheability::SideEffect,
        }
    }

    async fn execute(
        &self,
        tool_name: &str,
        arguments: &str,
        _cancel: Option<&CancellationToken>,
    ) -> Option<Result<String, SkillError>> {
        match tool_name {
            "notify" => Some(self.handle_notify(arguments).await),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct RecordingSender {
        calls: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl NotificationSender for RecordingSender {
        async fn send(&self, title: &str, body: &str) -> Result<(), String> {
            self.calls
                .lock()
                .expect("lock")
                .push((title.to_string(), body.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn notify_skill_parses_args_calls_sender_and_returns_success() {
        let sender = Arc::new(RecordingSender::default());
        let skill = NotifySkill::new(sender.clone());

        let result = skill
            .execute(
                "notify",
                r#"{"title":"Build done","body":"Specs landed successfully"}"#,
                None,
            )
            .await
            .expect("notify handled")
            .expect("notify succeeds");

        assert_eq!(result, "Notification sent");
        assert_eq!(
            sender.calls.lock().expect("lock").as_slice(),
            &[(
                "Build done".to_string(),
                "Specs landed successfully".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn notify_skill_missing_body_returns_error() {
        let skill = NotifySkill::new(Arc::new(RecordingSender::default()));

        let error = skill
            .execute("notify", r#"{"title":"Build done"}"#, None)
            .await
            .expect("notify handled")
            .expect_err("missing body should fail");

        assert_eq!(error, "Missing required field 'body'");
    }

    #[tokio::test]
    async fn notify_skill_defaults_title_when_omitted() {
        let sender = Arc::new(RecordingSender::default());
        let skill = NotifySkill::new(sender.clone());

        let result = skill
            .execute("notify", r#"{"body":"Task complete"}"#, None)
            .await
            .expect("notify handled")
            .expect("notify succeeds");

        assert_eq!(result, "Notification sent");
        assert_eq!(
            sender.calls.lock().expect("lock").as_slice(),
            &[(
                DEFAULT_NOTIFICATION_TITLE.to_string(),
                "Task complete".to_string()
            )]
        );
    }
}
