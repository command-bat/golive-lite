pub fn cleanup_residual_logs() {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let p = std::path::PathBuf::from(appdata)
            .join("Go Live Lite - Desktop")
            .join("logs");
        if p.exists() {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("logs");
            if p.exists() {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
}

#[inline]
pub fn log_line(_level: &str, _scope: &str, _msg: &str) {}

#[macro_export]
macro_rules! diag_info {
    ($scope:expr, $($arg:tt)*) => {{
        let _ = $scope;
        if false {
            let _ = format_args!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! diag_warn {
    ($scope:expr, $($arg:tt)*) => {{
        let _ = $scope;
        if false {
            let _ = format_args!($($arg)*);
        }
    }};
}

#[macro_export]
macro_rules! diag_error {
    ($scope:expr, $($arg:tt)*) => {{
        let _ = $scope;
        if false {
            let _ = format_args!($($arg)*);
        }
    }};
}

pub fn init_logging() {
    cleanup_residual_logs();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diag_macros_are_zero_io_noops() {
        log_line("INFO", "test", "noop");
        crate::diag_info!("test", "fmt {}", 123);
        crate::diag_warn!("test", "fmt {}", 456);
        crate::diag_error!("test", "fmt {}", 789);
        assert!(!std::env::temp_dir()
            .join("Go Live Lite - Desktop")
            .join("logs")
            .exists());
    }
}
