//! Hello-world binary for Android NDK cross-compilation validation.
//!
//! This binary exists to verify that Fawx can cross-compile a runnable executable
//! for `aarch64-linux-android` and execute it on rooted arm64 Android devices
//! (for example Pixel 10 via adb push + shell run).

fn hello_message() -> &'static str {
    "hello from fawx android (aarch64-linux-android)"
}

fn main() {
    println!("{}", hello_message());
}

#[cfg(test)]
mod tests {
    use super::hello_message;

    #[test]
    fn hello_message_matches_expected_output() {
        assert_eq!(
            hello_message(),
            "hello from fawx android (aarch64-linux-android)"
        );
    }
}
