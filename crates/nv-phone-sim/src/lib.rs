//! Phone simulator for pre-PoC testing.
//!
//! Provides a mock phone environment for validating the agent's reasoning
//! and action planning without requiring actual Android hardware.

use nv_core::error::Result;
use nv_core::types::{
    ActionResult, Notification, PhoneActions, ScreenState, SwipeDirection, UiElement,
};

/// Simulated phone state.
///
/// Implements the same `PhoneActions` trait as the real phone,
/// allowing the agent to be tested without hardware.
pub struct SimulatedPhone {
    current_app: String,
    screen_elements: Vec<UiElement>,
    notifications: Vec<Notification>,
}

impl SimulatedPhone {
    /// Create a new simulated phone in the initial state.
    pub fn new() -> Self {
        Self {
            current_app: "launcher".to_string(),
            screen_elements: Vec::new(),
            notifications: Vec::new(),
        }
    }

    /// Get a human-readable description of the current phone state.
    pub fn describe(&self) -> String {
        format!(
            "Current app: {}, {} elements visible, {} notifications",
            self.current_app,
            self.screen_elements.len(),
            self.notifications.len()
        )
    }
}

impl Default for SimulatedPhone {
    fn default() -> Self {
        Self::new()
    }
}

impl PhoneActions for SimulatedPhone {
    fn tap(&mut self, target: &str) -> Result<ActionResult> {
        tracing::info!("[SIM] Tapping: {}", target);
        Ok(ActionResult {
            step_id: "sim_tap".to_string(),
            success: true,
            message: format!("Simulated tap on {}", target),
            data: None,
        })
    }

    fn swipe(&mut self, direction: SwipeDirection) -> Result<ActionResult> {
        tracing::info!("[SIM] Swiping: {:?}", direction);
        Ok(ActionResult {
            step_id: "sim_swipe".to_string(),
            success: true,
            message: format!("Simulated swipe {:?}", direction),
            data: None,
        })
    }

    fn type_text(&mut self, text: &str) -> Result<ActionResult> {
        tracing::info!("[SIM] Typing: {}", text);
        Ok(ActionResult {
            step_id: "sim_type".to_string(),
            success: true,
            message: format!("Simulated typing: {}", text),
            data: None,
        })
    }

    fn launch_app(&mut self, name: &str) -> Result<ActionResult> {
        tracing::info!("[SIM] Launching app: {}", name);
        self.current_app = name.to_string();
        Ok(ActionResult {
            step_id: "sim_launch".to_string(),
            success: true,
            message: format!("Launched app: {}", name),
            data: None,
        })
    }

    fn go_home(&mut self) -> Result<ActionResult> {
        tracing::info!("[SIM] Going home");
        self.current_app = "launcher".to_string();
        Ok(ActionResult {
            step_id: "sim_home".to_string(),
            success: true,
            message: "Returned to home screen".to_string(),
            data: None,
        })
    }

    fn go_back(&mut self) -> Result<ActionResult> {
        tracing::info!("[SIM] Going back");
        Ok(ActionResult {
            step_id: "sim_back".to_string(),
            success: true,
            message: "Pressed back button".to_string(),
            data: None,
        })
    }

    fn read_screen(&self) -> Result<ScreenState> {
        Ok(ScreenState {
            current_app: self.current_app.clone(),
            elements: self.screen_elements.clone(),
            text_content: "Simulated screen content".to_string(),
        })
    }

    fn get_notifications(&self) -> Result<Vec<Notification>> {
        Ok(self.notifications.clone())
    }

    fn set_setting(&mut self, key: &str, value: &str) -> Result<ActionResult> {
        tracing::info!("[SIM] Setting {} = {}", key, value);
        Ok(ActionResult {
            step_id: "sim_setting".to_string(),
            success: true,
            message: format!("Set {} to {}", key, value),
            data: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_phone_basic_actions() {
        let mut phone = SimulatedPhone::new();

        // Test launch app
        let result = phone.launch_app("Chrome").unwrap();
        assert!(result.success);
        assert_eq!(phone.current_app, "Chrome");

        // Test go home
        let result = phone.go_home().unwrap();
        assert!(result.success);
        assert_eq!(phone.current_app, "launcher");
    }
}
