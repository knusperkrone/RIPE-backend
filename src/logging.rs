use once_cell::sync::Lazy;
use sloggers::{
    terminal::{Destination, TerminalLoggerBuilder},
    types::Severity,
    Build,
};

pub static APP_LOGGING: Lazy<slog::Logger> = Lazy::new(|| {
    let mut builder = TerminalLoggerBuilder::new();
    if cfg!(test) {
        builder.level(Severity::Debug);
        builder.destination(Destination::Stdout);
    } else {
        builder.level(Severity::Debug);
        builder.destination(Destination::Stdout);
    }

    builder.build().unwrap()
});
