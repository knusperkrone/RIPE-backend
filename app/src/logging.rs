use once_cell::sync::Lazy;
use sloggers::{
    terminal::{Destination, TerminalLoggerBuilder},
    types::{Format, Severity},
    Build,
};

pub static APP_LOGGING: Lazy<slog::Logger> = Lazy::new(|| {
    let mut builder = TerminalLoggerBuilder::new();
    builder.destination(Destination::Stdout);
    builder.level(Severity::Info);
    if cfg!(test) {
        builder.format(Format::Compact);
    } else if cfg!(prod) {
        builder.level(Severity::Info);
    }
    builder.build().unwrap()
});
