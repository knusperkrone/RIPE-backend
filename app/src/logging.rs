use once_cell::sync::Lazy;
use sloggers::{
    terminal::{Destination, TerminalLoggerBuilder},
    types::{Format, Severity},
    Build,
};

pub static APP_LOGGING: Lazy<slog::Logger> = Lazy::new(|| {
    let mut builder = TerminalLoggerBuilder::new();
    builder.destination(Destination::Stdout);
    builder.level(Severity::Debug);
    if cfg!(test) {
        builder.level(Severity::Critical);
        builder.format(Format::Compact);
    }
    builder.build().unwrap()
});
