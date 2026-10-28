//! `kind = "exe"`: any program, started from its executable. Shown as
//! "Program" in the UI.

use std::path::{Path, PathBuf};

use toml::Table;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ELEVATION_REQUIRED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND,
};
use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
use windows::Win32::System::Threading::{
    CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_CONSOLE, CREATE_SUSPENDED, CreateProcessW,
    PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, ResumeThread, STARTUPINFOW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::write_line;
use crate::diskconfig::{ConfigError, InvalidReason, fields};
use crate::launcher::{LaunchError, Started};
use crate::win32::{OwnedHandle, win32_code};

pub const KIND: &str = "exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeGame {
    /// Absolute path of the executable. Specific to the PC the disk was
    /// configured on.
    pub path: PathBuf,
    /// Command-line arguments.
    pub args: Vec<String>,
}

impl ExeGame {
    /// Folder the program is started in: the executable's own folder, which
    /// many games require.
    pub fn working_dir(&self) -> Option<&Path> {
        self.path.parent()
    }

    pub(crate) fn from_fields(game: &mut Table) -> Result<Self, ConfigError> {
        let path = fields::take_string(game, "path")?.ok_or(ConfigError::MissingField("path"))?;
        let path = PathBuf::from(path.trim());
        if !path.is_absolute() {
            return Err(ConfigError::InvalidValue {
                field: "path",
                reason: InvalidReason::NotAbsolute,
            });
        }
        if !fields::has_exe_extension(&path) {
            return Err(ConfigError::InvalidValue {
                field: "path",
                reason: InvalidReason::NotExe,
            });
        }
        let args = fields::take_string_array(game, "args")?.unwrap_or_default();
        Ok(Self { path, args })
    }

    /// Starts the program in its own folder, inside a job so that the
    /// processes it starts can be closed with it. The job does not close the
    /// game when the app exits.
    pub(crate) fn launch(&self) -> Result<Started, LaunchError> {
        if !self.path.is_file() {
            return Err(LaunchError::NotFound);
        }
        // Started suspended, so it is in the job before it can start other
        // processes. Breaking away from a job the app itself may be in (such
        // as the one `cargo run` uses) keeps the game alive when the app
        // exits; not every job allows it.
        let flags = CREATE_SUSPENDED | CREATE_NEW_CONSOLE;
        let info = match self.create_process(flags | CREATE_BREAKAWAY_FROM_JOB) {
            Err(code) if code == ERROR_ACCESS_DENIED.0 => self.create_process(flags),
            result => result,
        }
        .map_err(|code| match code {
            code if code == ERROR_FILE_NOT_FOUND.0 || code == ERROR_PATH_NOT_FOUND.0 => {
                LaunchError::NotFound
            }
            code if code == ERROR_ELEVATION_REQUIRED.0 => LaunchError::ElevationRequired,
            code => LaunchError::Failed(code),
        })?;
        let process = OwnedHandle(info.hProcess);
        let thread = OwnedHandle(info.hThread);

        let job = match unsafe { CreateJobObjectW(None, PCWSTR::null()) } {
            Ok(job) => {
                let job = OwnedHandle(job);
                match unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } {
                    Ok(()) => Some(job),
                    Err(error) => {
                        log::warn!("could not put the game in a job: {error}");
                        None
                    }
                }
            }
            Err(error) => {
                log::warn!("could not create a job for the game: {error}");
                None
            }
        };
        unsafe { ResumeThread(thread.raw()) };
        Ok(Started {
            job,
            process: Some(process),
        })
    }

    fn create_process(&self, flags: PROCESS_CREATION_FLAGS) -> Result<PROCESS_INFORMATION, u32> {
        let application = HSTRING::from(self.path.as_os_str());
        let mut command_line: Vec<u16> = command_line(&self.path, &self.args)
            .encode_utf16()
            .chain([0])
            .collect();
        let directory = self.working_dir().map(|dir| HSTRING::from(dir.as_os_str()));
        let directory = directory
            .as_ref()
            .map_or(PCWSTR::null(), |dir| PCWSTR(dir.as_ptr()));
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        unsafe {
            CreateProcessW(
                &application,
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                flags,
                None,
                directory,
                &startup,
                &mut info,
            )
        }
        .map_err(|error| win32_code(&error))?;
        Ok(info)
    }

    pub(crate) fn write_fields(&self, out: &mut String) {
        write_line(
            out,
            "path",
            fields::quote_path(&self.path.to_string_lossy()),
        );
        if !self.args.is_empty() {
            write_line(out, "args", fields::quote_array(&self.args));
        }
    }
}

/// Builds a command line that programs split back into `path` and `args`,
/// following the Microsoft C runtime rules.
fn command_line(path: &Path, args: &[String]) -> String {
    // Paths cannot contain quotes, so quoting them is enough.
    let mut line = format!("\"{}\"", path.display());
    for arg in args {
        line.push(' ');
        quote_arg(arg, &mut line);
    }
    line
}

fn quote_arg(arg: &str, out: &mut String) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote are escaped, and so is the quote.
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    // Backslashes before the closing quote are escaped too.
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_the_program() {
        let path = Path::new(r"C:\Games\My Game\game.exe");
        assert_eq!(command_line(path, &[]), r#""C:\Games\My Game\game.exe""#);
        let args = ["-fullscreen".to_owned(), r"C:\saves".to_owned()];
        assert_eq!(
            command_line(path, &args),
            r#""C:\Games\My Game\game.exe" -fullscreen C:\saves"#
        );
    }

    #[test]
    fn quotes_args_with_spaces_and_quotes() {
        let cases = [
            ("-skill 3", r#""-skill 3""#),
            ("", r#""""#),
            (r#"say "hi""#, r#""say \"hi\"""#),
            (r"C:\My Saves\", r#""C:\My Saves\\""#),
            (r#"a\"b"#, r#""a\\\"b""#),
        ];
        for (arg, quoted) in cases {
            let mut out = String::new();
            quote_arg(arg, &mut out);
            assert_eq!(out, quoted, "{arg}");
        }
    }
}
