#[macro_export]
macro_rules! log {
    () => {
        println!();
    };
    ($($arg:tt)+) => {
        print!("[multip] ");
        println!($($arg)*);
    }
}
