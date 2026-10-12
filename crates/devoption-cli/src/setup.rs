//! Terminal interaction for the explicit local/SSH setup command.

use devoption_server::{owner_auth::local_setup::LocalSetup, password::MAX_PASSWORD_BYTES};
use rustix::{
    process::getpgrp,
    termios::{self, OptionalActions, SpecialCodeIndex, Termios},
};
use signal_hook::{SigId, consts::signal::*};
use std::{
    fs::{File, OpenOptions},
    io::{self, IsTerminal, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub fn run(path: &Path) -> Result<(), String> {
    let setup = LocalSetup::open(path).map_err(|error| error.to_string())?;
    let mut terminal = Terminal::open().map_err(|error| error.to_string())?;
    let password = terminal
        .password("New owner password (15+ characters): ")
        .map_err(|error| error.to_string())?;
    let confirmation = terminal
        .password("Confirm owner password: ")
        .map_err(|error| error.to_string())?;
    // Do not publish an owner if terminal restoration failed or input was cancelled.
    terminal
        .restore()
        .map_err(|_| "could not restore terminal; setup cancelled".to_owned())?;
    terminal.signals.complete(|| {
        setup
            .complete(&password, &confirmation)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })?;
    println!("Owner created. Start or restart serve with the same --data-dir to enable sign-in.");
    Ok(())
}

fn input_error(message: &'static str) -> io::Error {
    io::Error::other(message)
}

struct Signals {
    cancelled: Arc<AtomicBool>,
    exit_on_signal: Arc<AtomicBool>,
    registrations: Vec<SigId>,
}

impl Signals {
    fn register() -> io::Result<Self> {
        let mut signals = Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            exit_on_signal: Arc::new(AtomicBool::new(false)),
            registrations: Vec::new(),
        };
        for signal in [SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGTSTP] {
            signals
                .registrations
                .push(signal_hook::flag::register_conditional_shutdown(
                    signal,
                    128 + signal,
                    Arc::clone(&signals.exit_on_signal),
                )?);
            signals.registrations.push(signal_hook::flag::register(
                signal,
                Arc::clone(&signals.cancelled),
            )?);
        }
        Ok(signals)
    }

    /// Call only after restoring the terminal. Cancellation during completion
    /// must stop execution, not set a flag that blocking hashing/storage ignores.
    fn complete(&self, operation: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        self.exit_on_signal.store(true, Ordering::SeqCst);
        // Preserve cancellation received before switching the handlers, including
        // the interval between the last input read and terminal restoration.
        if self.cancelled.load(Ordering::SeqCst) {
            return Err("setup cancelled".to_owned());
        }
        operation()
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

struct Terminal {
    file: File,
    original: Termios,
    signals: Signals,
    changed: bool,
}

impl Terminal {
    fn open() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            return Err(input_error(
                "setup requires an interactive terminal (use SSH with a PTY)",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(|_| input_error("setup requires a controlling terminal"))?;
        if termios::tcgetpgrp(&file)? != getpgrp() {
            return Err(input_error("run setup in the terminal foreground"));
        }
        let original = termios::tcgetattr(&file)?;
        let signals = Signals::register()?;
        let mut hidden = original.clone();
        hidden.make_raw();
        hidden.output_modes = original.output_modes;
        // Poll cancellation even while the user has not typed a byte. Noncanonical
        // input also avoids silent kernel line truncation at platform-specific limits.
        hidden.special_codes[SpecialCodeIndex::VMIN] = 0;
        hidden.special_codes[SpecialCodeIndex::VTIME] = 1;
        let mut terminal = Self {
            file,
            original,
            signals,
            changed: true,
        };
        termios::tcsetattr(&terminal.file, OptionalActions::Flush, &hidden)?;
        // A successful call may apply only supported settings: verify echo is off.
        let actual = termios::tcgetattr(&terminal.file)?;
        if actual
            .local_modes
            .intersects(termios::LocalModes::ECHO | termios::LocalModes::ECHONL)
            || actual
                .local_modes
                .intersects(termios::LocalModes::ICANON | termios::LocalModes::ISIG)
        {
            terminal.restore()?;
            return Err(input_error("could not configure hidden password input"));
        }
        Ok(terminal)
    }

    fn check_cancelled(&self) -> io::Result<()> {
        if self.signals.cancelled.load(Ordering::Relaxed) {
            Err(input_error("setup cancelled"))
        } else {
            Ok(())
        }
    }

    fn password(&mut self, prompt: &str) -> io::Result<String> {
        self.check_cancelled()?;
        self.file.write_all(prompt.as_bytes())?;
        self.file.flush()?;
        let mut bytes = Vec::with_capacity(MAX_PASSWORD_BYTES);
        let mut invalid = None;
        loop {
            self.check_cancelled()?;
            let mut byte = [0];
            match self.file.read(&mut byte) {
                Ok(0) => {
                    // VTIME expired, or the terminal hung up without a signal.
                    if termios::tcgetpgrp(&self.file).ok() != Some(getpgrp()) {
                        return Err(input_error("terminal unavailable; setup cancelled"));
                    }
                    continue;
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(input_error("could not read password; setup cancelled")),
            }
            match byte[0] {
                b'\r' | b'\n' => {
                    self.file.write_all(b"\n")?;
                    if let Some(message) = invalid {
                        return Err(input_error(message));
                    }
                    return String::from_utf8(bytes)
                        .map_err(|_| input_error("password must be valid UTF-8"));
                }
                // Handle cancellation ourselves so RAII can restore terminal echo.
                3 | 4 | 26 | 28 => return Err(input_error("setup cancelled")),
                // Finish consuming a rejected line while echo is still disabled.
                // Otherwise a long paste could continue after echo was restored.
                _ if invalid.is_some() => {}
                8 | 127 => while bytes.pop().is_some_and(|byte| byte & 0xc0 == 0x80) {},
                21 => bytes.clear(), // Ctrl-U clears the current input.
                0..=31 => invalid = Some("password cannot contain control characters"),
                byte => {
                    if bytes.len() == MAX_PASSWORD_BYTES {
                        invalid = Some("password exceeds 1024 UTF-8 bytes");
                    } else {
                        bytes.push(byte);
                    }
                }
            }
        }
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.changed {
            // Discard queued input before re-enabling echo, including overlong paste.
            termios::tcsetattr(&self.file, OptionalActions::Flush, &self.original)?;
            self.changed = false;
        }
        Ok(())
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::PermissionsExt, process::Command};

    #[test]
    fn completion_signal_fixture() {
        let Ok(signal) = std::env::var("DEVOPTION_FIXTURE_COMPLETION_SIGNAL") else {
            return;
        };
        let signal: i32 = signal.parse().unwrap();
        let path = std::env::var_os("DEVOPTION_FIXTURE_OWNER_DIR").unwrap();
        let setup = LocalSetup::open(Path::new(&path)).unwrap();
        let signals = Signals::register().unwrap();
        signals
            .complete(|| {
                // This callback is the same completion boundary as the production
                // hashing/storage call. A swallowed signal would publish an owner.
                signal_hook::low_level::raise(signal).unwrap();
                setup
                    .complete("public fixture phrase", "public fixture phrase")
                    .unwrap();
                Ok(())
            })
            .unwrap();
        panic!("cancellation returned to owner creation");
    }

    #[test]
    fn completion_signals_exit_before_publication_instead_of_being_swallowed() {
        for signal in [SIGINT, SIGTERM, SIGHUP, SIGQUIT, SIGTSTP] {
            let temporary = tempfile::tempdir().unwrap();
            std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let path = std::fs::canonicalize(temporary.path()).unwrap();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "setup::tests::completion_signal_fixture",
                    "--test-threads=1",
                ])
                .env("DEVOPTION_FIXTURE_COMPLETION_SIGNAL", signal.to_string())
                .env("DEVOPTION_FIXTURE_OWNER_DIR", &path)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(128 + signal),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(std::fs::read_dir(path).unwrap().count(), 0);
            assert!(!String::from_utf8_lossy(&output.stdout).contains("public fixture phrase"));
        }
    }

    #[test]
    fn cancellation_before_completion_never_runs_the_operation() {
        let signals = Signals {
            cancelled: Arc::new(AtomicBool::new(true)),
            exit_on_signal: Arc::new(AtomicBool::new(false)),
            registrations: Vec::new(),
        };
        assert!(
            signals
                .complete(|| panic!("cancelled completion ran"))
                .is_err()
        );
    }
}
