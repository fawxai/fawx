/// Trait for errors returned to the agent.
/// `user_message()` is safe to show — no internal details.
/// `internal_message()` is for logs only.
pub trait UserFacingError: std::fmt::Display {
    fn user_message(&self) -> String;

    fn internal_message(&self) -> String {
        self.to_string()
    }
}
