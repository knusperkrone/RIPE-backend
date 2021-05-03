use once_cell::sync::Lazy;
use sloggers::{
    terminal::{Destination, TerminalLoggerBuilder},
    types::{Format, Severity},
    Build,
};

pub static APP_LOGGING: Lazy<slog::Logger> = Lazy::new(|| {
    let mut builder = TerminalLoggerBuilder::new();
    builder.destination(Destination::Stdout);
    if cfg!(test) {
        builder.level(Severity::Debug);
        builder.format(Format::Compact);
    } else {
        builder.level(Severity::Info);
    }
    builder.build().unwrap()
});
