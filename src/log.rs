#[macro_export]
macro_rules! log {
    () => {
        println!();
    };
    ($($arg:tt)+) => {
        println!($($arg)*);
    }
}

#[macro_export]
macro_rules! debug {
    () => {
        if std::env::var("MULTIP_DEBUG").is_ok() {
            println!();
        }
    };
    ($($arg:tt)+) => {
        if std::env::var("MULTIP_DEBUG").is_ok() {
            print!("<DEBUG> ");
            println!($($arg)*);
        }
    }
}
