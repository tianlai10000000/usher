use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::Sender;

pub enum SessionEvent {
    Output(Vec<u8>),
    Exit,
}

pub struct PtySession {
    _pair: PtyPair,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    pub fn spawn(
        cwd: &str,
        cols: u16,
        rows: u16,
        tx: Sender<SessionEvent>,
    ) -> Result<Self, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("openpty failed: {error}"))?;

        let shell = default_shell();
        let mut cmd = CommandBuilder::new(shell.program.clone());
        apply_shell_mode_args(&mut cmd, &shell.program);
        if !cwd.trim().is_empty() {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|error| format!("spawn shell failed: {error}"))?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("clone reader failed: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("take writer failed: {error}"))?;

        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(SessionEvent::Exit);
                        break;
                    }
                    Ok(n) => {
                        let _ = tx.send(SessionEvent::Output(buf[..n].to_vec()));
                    }
                    Err(_) => {
                        let _ = tx.send(SessionEvent::Exit);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            _pair: pair,
            writer,
            child,
        })
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(bytes)
            .map_err(|error| format!("write failed: {error}"))?;
        self.writer
            .flush()
            .map_err(|error| format!("flush failed: {error}"))?;
        Ok(())
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), String> {
        self._pair
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("resize failed: {error}"))
    }

    pub fn terminate(&mut self) {
        let _ = self.child.kill();
    }
}

struct ShellProgram {
    program: String,
}

fn default_shell() -> ShellProgram {
    #[cfg(target_os = "windows")]
    {
        ShellProgram {
            program: std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string()),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        ShellProgram {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()),
        }
    }
}

fn apply_shell_mode_args(cmd: &mut CommandBuilder, program: &str) {
    #[cfg(target_os = "windows")]
    {
        let program_name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if program_name == "powershell.exe" || program_name == "pwsh.exe" {
            cmd.arg("-NoLogo");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let program_name = Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        // Start the shell in login + interactive mode so rc/profile files match
        // the user's normal terminal session as closely as possible.
        if matches!(program_name.as_str(), "zsh" | "bash" | "fish" | "sh") {
            if program_name == "fish" {
                cmd.arg("-i");
            } else {
                cmd.args(["-l", "-i"]);
            }
        }
    }
}
