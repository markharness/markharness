//! `markharness gui`: launches the separate GUI executable on a project.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// File name of the GUI executable, without the platform's executable suffix.
pub const GUI_EXE_NAME: &str = "markharness-gui";

/// Environment variable through which the launched GUI learns which
/// `markharness` binary started it. The GUI calls that binary, so it always
/// talks to the CLI of the same distribution.
pub const MARKHARNESS_BIN_ENV: &str = "MARKHARNESS_BIN";

/// A fully assembled child-process invocation, kept as plain data so it can
/// be checked without starting a process.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub envs: Vec<(OsString, OsString)>,
}

/// Looks for the GUI executable next to the running `markharness` first, then
/// on `PATH`. Preferring the sibling keeps a distribution's own GUI ahead of
/// any other install, so the two stay the same release.
pub fn find_gui(own_exe_dir: Option<&Path>, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let file_name = format!("{GUI_EXE_NAME}{}", std::env::consts::EXE_SUFFIX);
    let path_dirs = path_var.into_iter().flat_map(std::env::split_paths);
    own_exe_dir
        .map(Path::to_path_buf)
        .into_iter()
        .chain(path_dirs)
        .map(|dir| dir.join(&file_name))
        .find(|candidate| candidate.is_file())
}

pub fn plan(gui: &Path, root: &Path, own_exe: &Path) -> LaunchPlan {
    LaunchPlan {
        program: gui.to_path_buf(),
        args: vec![OsString::from("--dir"), root.as_os_str().to_owned()],
        envs: vec![(
            OsString::from(MARKHARNESS_BIN_ENV),
            own_exe.as_os_str().to_owned(),
        )],
    }
}

pub fn not_bundled_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "the GUI executable '{GUI_EXE_NAME}' was not found next to this markharness binary or on PATH; \
             it is included in the GUI-bundled release archive, not in the CLI-only archive"
        ),
    )
}

/// Runs the plan, waits for the child, and returns its exit code. A child
/// ended by a signal has no exit code and is reported as 1.
pub fn execute(plan: &LaunchPlan) -> io::Result<i32> {
    let status = std::process::Command::new(&plan.program)
        .args(&plan.args)
        .envs(plan.envs.iter().map(|(key, value)| (key, value)))
        .status()?;
    Ok(status.code().unwrap_or(1))
}

/// Starts the GUI on the project at `root` and returns its exit code.
pub fn launch(root: &Path) -> io::Result<i32> {
    let own_exe = std::env::current_exe()?;
    let gui = find_gui(own_exe.parent(), std::env::var_os("PATH").as_deref())
        .ok_or_else(not_bundled_error)?;
    execute(&plan(&gui, root, &own_exe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn gui_file_name() -> String {
        format!("markharness-gui{}", env::consts::EXE_SUFFIX)
    }

    fn touch_gui(dir: &Path) -> PathBuf {
        let path = dir.join(gui_file_name());
        std::fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn find_gui_prefers_the_executable_next_to_markharness() {
        let own = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let sibling = touch_gui(own.path());
        touch_gui(other.path());
        let path_var = env::join_paths([other.path()]).unwrap();

        assert_eq!(find_gui(Some(own.path()), Some(&path_var)), Some(sibling));
    }

    #[test]
    fn find_gui_falls_back_to_path_when_there_is_no_sibling() {
        let own = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        let on_path = tempfile::tempdir().unwrap();
        let expected = touch_gui(on_path.path());
        let path_var = env::join_paths([empty.path(), on_path.path()]).unwrap();

        assert_eq!(find_gui(Some(own.path()), Some(&path_var)), Some(expected));
    }

    #[test]
    fn find_gui_returns_none_when_it_is_nowhere() {
        let own = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let path_var = env::join_paths([other.path()]).unwrap();

        assert_eq!(find_gui(Some(own.path()), Some(&path_var)), None);
        assert_eq!(find_gui(None, None), None);
    }

    #[test]
    fn find_gui_ignores_a_directory_with_the_executable_name() {
        let own = tempfile::tempdir().unwrap();
        std::fs::create_dir(own.path().join(gui_file_name())).unwrap();

        assert_eq!(find_gui(Some(own.path()), None), None);
    }

    #[test]
    fn plan_passes_the_root_and_names_the_launching_binary() {
        let gui = Path::new("/dist/markharness-gui");
        let root = Path::new("/work/project");
        let own = Path::new("/dist/markharness");

        let plan = plan(gui, root, own);

        assert_eq!(plan.program, gui);
        assert_eq!(
            plan.args,
            vec![OsString::from("--dir"), OsString::from(root)]
        );
        assert_eq!(
            plan.envs,
            vec![(OsString::from(MARKHARNESS_BIN_ENV), OsString::from(own))]
        );
    }

    #[test]
    fn not_bundled_error_says_what_is_missing_and_how_to_get_it() {
        let error = not_bundled_error();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let message = error.to_string();
        assert!(message.contains("markharness-gui"), "{message}");
        assert!(message.contains("GUI-bundled"), "{message}");
    }

    #[cfg(unix)]
    fn exit_with(code: i32) -> LaunchPlan {
        LaunchPlan {
            program: PathBuf::from("sh"),
            args: vec![OsString::from("-c"), OsString::from(format!("exit {code}"))],
            envs: vec![],
        }
    }

    #[cfg(windows)]
    fn exit_with(code: i32) -> LaunchPlan {
        LaunchPlan {
            program: PathBuf::from("cmd"),
            args: vec![OsString::from("/C"), OsString::from(format!("exit {code}"))],
            envs: vec![],
        }
    }

    #[test]
    fn execute_waits_and_returns_the_childs_exit_code() {
        assert_eq!(execute(&exit_with(7)).unwrap(), 7);
        assert_eq!(execute(&exit_with(0)).unwrap(), 0);
    }

    #[test]
    fn execute_fails_when_the_program_cannot_be_started() {
        let plan = LaunchPlan {
            program: PathBuf::from("/no/such/markharness-gui"),
            args: vec![],
            envs: vec![],
        };

        assert!(execute(&plan).is_err());
    }
}
