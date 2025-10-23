use std::time::{Duration, Instant};

#[inline]
pub fn now() -> Instant {
    Instant::now()
}

#[inline]
pub fn as_secs_u64(d: Duration) -> u64 {
    d.as_secs()
}
