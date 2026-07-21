//! Working-day calendar: whether a given local date is a workday in mainland
//! China, accounting for statutory holidays and 调休 (makeup workdays, where a
//! Saturday/Sunday becomes a workday to bridge a holiday).
//!
//! A pure interface so the workday *gate* (`agent::daemon::WorkdayGated`) can be
//! tested without any network or disk, and so the data source (online API,
//! offline table) is swappable behind the trait.

use async_trait::async_trait;

#[async_trait]
pub trait WorkdayCalendar: Send + Sync {
    /// Whether `date` (a local calendar date) is a working day. Implementations
    /// degrade gracefully — a Monday–Friday default — when their data source is
    /// unavailable, so the gate never hard-fails on a network blip.
    async fn is_workday(&self, date: chrono::NaiveDate) -> bool;
}

/// The fallback rule when no holiday data covers a date: Monday–Friday are
/// working days, weekends are not. Shared by the gate and every calendar
/// implementation so "no data" behaves identically everywhere.
pub fn is_weekday(date: chrono::NaiveDate) -> bool {
    use chrono::Datelike;
    !matches!(date.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
}
