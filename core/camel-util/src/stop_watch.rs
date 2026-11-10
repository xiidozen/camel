//! A very simple stop watch. Port of `org.apache.camel.util.StopWatch`.

use std::time::Instant;

/// A very simple stop watch.
///
/// This implementation is not thread safe and can only time one task at any given time.
///
/// # Examples
///
/// ```
/// use camel_util::stop_watch::StopWatch;
/// let mut watch = StopWatch::new();
/// assert!(watch.is_started());
/// let _millis = watch.taken();
/// watch.stop();
/// assert_eq!(0, watch.taken());
/// ```
#[derive(Debug, Clone, Copy)]
pub struct StopWatch {
    start: Option<Instant>,
}

impl Default for StopWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl StopWatch {
    /// Starts the stop watch.
    pub fn new() -> Self {
        StopWatch {
            start: Some(Instant::now()),
        }
    }

    /// Creates the stop watch.
    ///
    /// # Arguments
    ///
    /// * `start` - whether it should start immediately
    pub fn with_start(start: bool) -> Self {
        StopWatch {
            start: start.then(Instant::now),
        }
    }

    /// Starts or restarts the stop watch.
    pub fn restart(&mut self) {
        self.start = Some(Instant::now());
    }

    /// Whether the watch is started.
    pub fn is_started(&self) -> bool {
        self.start.is_some()
    }

    /// Returns the time taken in millis.
    ///
    /// # Returns
    ///
    /// Time in millis, or `0` if not started yet.
    pub fn taken(&self) -> u64 {
        self.start
            .map(|s| s.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    /// Returns the time taken in millis and restarts the timer.
    ///
    /// # Returns
    ///
    /// Time in millis, or `0` if not started yet.
    pub fn taken_and_restart(&mut self) -> u64 {
        let answer = self.taken();
        self.restart();
        answer
    }

    /// Stops the stop watch.
    pub fn stop(&mut self) {
        self.start = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    // The Java tests slept 1 second per step; the sleep is what is being measured (not a wait
    // for a condition), so it stays, shortened to 200ms with the same 50% tolerance.
    const STEP: u64 = 200;

    fn assert_step(taken: u64) {
        assert!(
            taken >= STEP,
            "Elapsed time should be equal to or greater than {STEP} ms but was {taken}"
        );
        assert!(
            taken < STEP * 3 / 2,
            "Elapsed time should be smaller than {} ms but was {taken}",
            STEP * 3 / 2
        );
    }

    #[test]
    fn test_duration_matches_expect() {
        let watch = StopWatch::new();
        sleep(Duration::from_millis(STEP));
        assert_step(watch.taken());
    }

    #[test]
    fn test_duration_matches_expect_with_restart() {
        let mut watch = StopWatch::new();
        sleep(Duration::from_millis(STEP));
        assert_step(watch.taken());
        watch.restart();
        sleep(Duration::from_millis(STEP));
        assert_step(watch.taken());
    }

    #[test]
    fn test_duration_matches_expect_late_start() {
        let mut watch = StopWatch::with_start(false);
        sleep(Duration::from_millis(STEP));
        assert_eq!(0, watch.taken());
        watch.restart();
        sleep(Duration::from_millis(STEP));
        assert_step(watch.taken());
    }

    #[test]
    fn test_taken_and_restart() {
        let mut watch = StopWatch::new();
        sleep(Duration::from_millis(STEP));
        assert_step(watch.taken_and_restart());
        assert!(watch.taken() < STEP);
    }
}
