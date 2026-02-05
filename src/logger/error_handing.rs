use anyhow::Result;

/// Example: a business operation that may fail
///
/// In real projects, replace with actual business logic.
/// This is only for demonstrating error handling and panic.
#[allow(dead_code)]
pub fn do_something_that_might_fail() -> Result<()> {
    // Simulate two failure paths:
    // 1. Return Err (normal error, should be handled by caller)
    // 2. Panic directly (unhandled, should be captured by global panic hook)

    let should_panic = cfg!(debug_assertions) && rand::random::<bool>(); // Easier to trigger in debug

    if should_panic {
        panic!("Simulated unhandled fatal error (should be captured by global hook)");
    }

    Err(anyhow::anyhow!(
        "Simulated business operation failed (normal error)"
    ))
}
