#[macro_export]
macro_rules! log {
    () => {
        println!();
    };
    ($($arg:tt)+) => {
        println!($($arg)*);
    }
}
