//! Windowing logic for walk-forward analysis

use chrono::{NaiveDate, Duration};

#[derive(Debug, Clone)]
pub struct WalkForwardWindow {
    pub train_start: NaiveDate,
    pub train_end: NaiveDate,
    pub test_start: NaiveDate,
    pub test_end: NaiveDate,
}

/// Generate rolling train/test windows
pub fn generate_windows(
    start: NaiveDate,
    end: NaiveDate,
    train_size: Duration,
    test_size: Duration,
    step_size: Duration,
) -> Vec<WalkForwardWindow> {
    let mut windows = Vec::new();
    let mut train_start = start;
    loop {
        let train_end = train_start + train_size;
        let test_start = train_end;
        let test_end = test_start + test_size;
        if test_end > end {
            break;
        }
        windows.push(WalkForwardWindow {
            train_start,
            train_end,
            test_start,
            test_end,
        });
        train_start = train_start + step_size;
    }
    windows
}
