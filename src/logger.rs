use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};
use owo_colors::OwoColorize;

pub struct Logger {
    level: LevelFilter,
}

impl Logger {
    pub fn new(level: LevelFilter) -> Self {
        Self { level }
    }

    pub fn init(self) -> Result<(), SetLoggerError> {
        log::set_max_level(self.level);
        log::set_boxed_logger(Box::new(self))
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let msg = record.args().to_string();

        match record.level() {
            Level::Error => eprintln!("{}", msg.bright_red()),
            Level::Warn => eprintln!("{}", msg.bright_yellow()),
            Level::Info => eprintln!("{}", msg.bright_blue()),
            Level::Debug => eprintln!("{}", msg.bright_magenta()),
            Level::Trace => eprintln!("{}", msg.bright_black()),
        }
    }

    fn flush(&self) {}
}
