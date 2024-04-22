use chrono::{Offset, Utc};
use chrono_tz::Tz;
use ripe_core::DAY_MS;

pub fn ms_add_offset(ms: u32, tz: Tz) -> u32 {
    let offset_ms = Utc::now()
        .with_timezone(&tz)
        .offset()
        .fix()
        .utc_minus_local()
        * 1000;

    let added = ms as i32 - offset_ms;
    if added < 0 {
        (DAY_MS as i32 + added) as u32
    } else if added > DAY_MS as i32 {
        (added - DAY_MS as i32) as u32
    } else {
        added as u32
    }
}

pub fn ms_clear_offset(ms: u32, tz: Tz) -> u32 {
    let offset_ms = Utc::now()
        .with_timezone(&tz)
        .offset()
        .fix()
        .utc_minus_local()
        * 1000;

    let added = ms as i32 + offset_ms;
    if added < 0 {
        (DAY_MS as i32 + added) as u32
    } else if added > DAY_MS as i32 {
        (added - DAY_MS as i32) as u32
    } else {
        added as u32
    }
}
