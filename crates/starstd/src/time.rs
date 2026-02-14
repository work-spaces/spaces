use anyhow::Context;
use anyhow_source_location::format_context;
use starlark::environment::GlobalsBuilder;
use starlark::values::none::NoneType;

// This defines the function that is visible to Starlark
#[starlark_module]
pub fn globals(builder: &mut GlobalsBuilder) {
    /// Returns the current system time as a tuple.
    ///
    /// This provides a high-precision snapshot of the current time,
    /// represented as two distinct integer values.
    ///
    /// ```python
    /// (secs, nsec) = time.now()
    /// print("Seconds:", secs)
    /// print("Nanoseconds:", nsec)
    /// ```
    ///
    /// # Returns
    /// * `(int, int)`: A tuple containing seconds since the Unix epoch and fractional nanoseconds.
    fn now() -> anyhow::Result<(u64, u32)> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context(format_context!("Failed to get current time"))?;
        Ok((current_time.as_secs(), current_time.subsec_nanos()))
    }

    /// Pauses the execution of the script for a specified duration.
    ///
    /// This is useful for waiting between retries, slowing down loops,
    /// or giving external processes time to initialize.
    ///
    /// ```python
    /// time.sleep(1000000000)
    /// ```
    ///
    /// # Arguments
    /// * `nanoseconds`: The number of nanoseconds to pause execution.
    fn sleep(nanoseconds: u64) -> anyhow::Result<NoneType> {
        std::thread::sleep(std::time::Duration::from_nanos(nanoseconds));
        Ok(NoneType)
    }
}
