//! Android phone control (PoC only - 0% reuse).
//!
//! This crate provides Android-specific phone control via touch injection,
//! screen capture, and accessibility services. This is PoC scaffolding that
//! will be replaced in Horizon 2 with native OS integration.

use ct_core::error::Result;
use ct_core::types::{ActionResult, Notification, PhoneActions, ScreenState, SwipeDirection};

/// Android phone controller.
///
/// Implements the `PhoneActions` trait using Android-specific APIs.
/// This is PoC-only code that will not carry forward to Horizon 2.
pub struct AndroidPhone {
    // Placeholder - will be implemented in Horizon 1 Phase 2
}

impl PhoneActions for AndroidPhone {
    fn tap(&mut self, _target: &str) -> Result<ActionResult> {
        todo!("Android touch injection will be implemented in Horizon 1 Phase 2")
    }

    fn swipe(&mut self, _direction: SwipeDirection) -> Result<ActionResult> {
        todo!()
    }

    fn type_text(&mut self, _text: &str) -> Result<ActionResult> {
        todo!()
    }

    fn launch_app(&mut self, _name: &str) -> Result<ActionResult> {
        todo!()
    }

    fn go_home(&mut self) -> Result<ActionResult> {
        todo!()
    }

    fn go_back(&mut self) -> Result<ActionResult> {
        todo!()
    }

    fn read_screen(&self) -> Result<ScreenState> {
        todo!()
    }

    fn get_notifications(&self) -> Result<Vec<Notification>> {
        todo!()
    }

    fn set_setting(&mut self, _key: &str, _value: &str) -> Result<ActionResult> {
        todo!()
    }
}
