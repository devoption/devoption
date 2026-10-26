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
    if let [command, path, port, mode, records, https] = args.as_slice()
        && command == "capability-worker"
        && ["serve", "validate"].contains(&mode.as_str())
    {
        let Ok(port) = port.parse::<u16>() else {
            return std::process::ExitCode::from(2);
        };
        let record_path = (records != "-").then(|| std::path::Path::new(records));
        let https_path = (https != "-").then(|| std::path::Path::new(https));
        return match devoption_runtime::run_with_capabilities(
            std::path::Path::new(path),
            port,
            mode == "validate",
            record_path,
            https_path,
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("Capability worker failed");
                std::process::ExitCode::FAILURE
            }
        };
    }
    if let [command, path, port, mode, grant] = args.as_slice()
        && command == "records-worker"
        && ["serve", "validate"].contains(&mode.as_str())
    {
        let Ok(port) = port.parse::<u16>() else {
            return std::process::ExitCode::from(2);
        };
        return match devoption_runtime::run_with_records(
            std::path::Path::new(path),
            port,
            mode == "validate",
            std::path::Path::new(grant),
        ) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => {
                eprintln!("Record worker failed");
                std::process::ExitCode::FAILURE
            }
        };
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
