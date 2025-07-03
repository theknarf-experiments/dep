use colored::Colorize;

pub fn log_error(color: bool, msg: &str) {
    if color {
        eprintln!("{}", msg.red());
    } else {
        eprintln!("{}", msg);
    }
}

pub fn log_verbose(color: bool, msg: &str) {
    if color {
        println!("{}", msg.cyan());
    } else {
        println!("{}", msg);
    }
}
