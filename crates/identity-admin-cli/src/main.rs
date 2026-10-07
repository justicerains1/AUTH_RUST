use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().any(|arg| arg == "--check-config") {
        match identity_core::config::Config::from_env() {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        }
    } else {
        eprintln!("T01: administrator initialization is not implemented; see T14.");
        ExitCode::FAILURE
    }
}
