use colored::Colorize;

/// Logging level for [`Logger`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Info,
    Debug,
}

/// Simple logging trait.
pub trait Logger: Send + Sync {
    fn log(&self, level: LogLevel, msg: &str);
}

/// Logger implementation that discards all messages.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyLogger;

impl Logger for EmptyLogger {
    fn log(&self, _level: LogLevel, _msg: &str) {}
}

/// Logger that writes to stdout/stderr.
#[derive(Clone, Copy, Debug)]
pub struct ConsoleLogger {
    pub color: bool,
    pub verbose: bool,
}

impl Logger for ConsoleLogger {
    fn log(&self, level: LogLevel, msg: &str) {
        if !self.verbose && matches!(level, LogLevel::Debug) {
            return;
        }
        let output = if self.color {
            match level {
                LogLevel::Error => msg.red().to_string(),
                LogLevel::Info => msg.cyan().to_string(),
                LogLevel::Debug => msg.green().to_string(),
            }
        } else {
            msg.to_string()
        };
        match level {
            LogLevel::Error => eprintln!("{}", output),
            _ => println!("{}", output),
        }
    }
}
