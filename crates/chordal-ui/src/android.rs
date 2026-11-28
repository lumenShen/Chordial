//! Android entry point invoked from the JNI library thread.

/// Runs the shared Chordial application against the Android Surface.
pub fn run() {
    crate::desktop::run_app(true);
}
