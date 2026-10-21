fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag] if flag == "--version" => {
            println!("devoption-runtime {}", env!("CARGO_PKG_VERSION"));
            return std::process::ExitCode::SUCCESS;
        }
        [flag] if flag == "--help" => {
            println!("Usage: devoption-runtime app-worker COMPONENT PORT serve|validate");
            return std::process::ExitCode::SUCCESS;
        }
        _ => {}
    }
    let [command, path, port, mode] = args.as_slice() else {
        return std::process::ExitCode::from(2);
    };
    if command != "app-worker" || !["serve", "validate"].contains(&mode.as_str()) {
        return std::process::ExitCode::from(2);
    }
    let Ok(port) = port.parse::<u16>() else {
        return std::process::ExitCode::from(2);
    };
    match devoption_runtime::run(std::path::Path::new(path), port, mode == "validate") {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("App worker failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
