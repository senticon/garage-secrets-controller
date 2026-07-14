use tracing::{info, warn};

const BUILD_VERSION: &str = env!("BUILD_VERSION");
const BUILD_DATE: &str = env!("BUILD_DATE");
const PR_BUILD_WARNING: &str = "This version is built from a pull request and therefore not intended to run in production. Please use a stable build.";

pub fn log_version_info() {
    log_version_info_values(BUILD_VERSION, BUILD_DATE);
}

fn log_version_info_values(version: &str, build_date: &str) {
    info!(
        version = version,
        build_date = build_date,
        "starting garage-secrets-controller"
    );
    if version.starts_with("PR-") {
        warn!(PR_BUILD_WARNING);
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use tracing::subscriber::with_default;

    use super::{log_version_info_values, PR_BUILD_WARNING};

    #[derive(Clone)]
    struct TestWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.lock().unwrap().flush()
        }
    }

    #[test]
    fn logs_version_and_build_date_without_pr_warning() {
        let logs = capture_logs(|| log_version_info_values("abc1234", "2026-07-14T12:34:56Z"));

        assert!(logs.contains("starting garage-secrets-controller"));
        assert!(logs.contains("abc1234"));
        assert!(logs.contains("2026-07-14T12:34:56Z"));
        assert!(!logs.contains(PR_BUILD_WARNING));
    }

    #[test]
    fn logs_pr_build_warning() {
        let logs =
            capture_logs(|| log_version_info_values("PR-42-abc1234", "2026-07-14T12:34:56Z"));

        assert!(logs.contains("starting garage-secrets-controller"));
        assert!(logs.contains("PR-42-abc1234"));
        assert!(logs.contains(PR_BUILD_WARNING));
    }

    fn capture_logs(test: impl FnOnce()) -> String {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(move || TestWriter(writer.clone()))
            .finish();

        with_default(subscriber, test);

        let logs = output.lock().unwrap().clone();
        String::from_utf8(logs).unwrap()
    }
}
