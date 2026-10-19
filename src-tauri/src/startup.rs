use tauri::AppHandle;

pub fn is_enabled(app: &AppHandle) -> Result<bool, String> {
    native::is_enabled(app)
}

pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    native::set_enabled(app, enabled)
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::path::Path;
    use windows_registry::{Type, CURRENT_USER};

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "QZ Flow";

    fn optional<T>(value: windows_registry::Result<T>) -> Result<Option<T>, String> {
        match value {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code().0 == 0x80070002_u32 as i32 => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn command_for(path: &Path) -> Result<String, String> {
        let path = path
            .to_str()
            .ok_or("Startup executable path is not valid Unicode.")?;
        let command = format!("\"{path}\" --autostart");
        if command.encode_utf16().count() > 260 {
            return Err("Windows startup commands are limited to 260 characters. Move QZ Flow to a shorter path before enabling launch at login.".into());
        }
        Ok(command)
    }

    fn command() -> Result<String, String> {
        command_for(&std::env::current_exe().map_err(|error| error.to_string())?)
    }

    fn approval_enabled(value: Option<&[u8]>) -> bool {
        // Windows Task Manager uses 3/7 for disabled Run entries; absent metadata permits startup.
        !matches!(value.and_then(|bytes| bytes.first()), Some(3 | 7))
    }

    pub fn is_enabled(_app: &AppHandle) -> Result<bool, String> {
        let entry = optional(CURRENT_USER.open(RUN).and_then(|key| key.get_string(NAME)))?;
        if entry.as_deref() != Some(command()?.as_str()) {
            return Ok(false);
        }
        let approval = optional(
            CURRENT_USER
                .open(APPROVED)
                .and_then(|key| key.get_value(NAME)),
        )?;
        Ok(approval_enabled(approval.as_deref()))
    }

    pub fn set_enabled(_app: &AppHandle, enabled: bool) -> Result<(), String> {
        if enabled {
            let value = command()?;
            CURRENT_USER
                .create(RUN)
                .and_then(|key| key.set_string(NAME, &value))
                .map_err(|error| error.to_string())?;
            if let Some(key) = optional(CURRENT_USER.options().write().open(APPROVED))? {
                key.set_bytes(NAME, Type::Bytes, &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                    .map_err(|error| error.to_string())?;
            }
        } else if let Some(key) = optional(CURRENT_USER.options().write().open(RUN))? {
            optional(key.remove_value(NAME))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn executable_with_spaces_is_quoted_and_login_flag_is_separate() {
            assert_eq!(
                command_for(Path::new(r"C:\Program Files\QZ Flow\QZ Flow.exe")).unwrap(),
                r#""C:\Program Files\QZ Flow\QZ Flow.exe" --autostart"#
            );
        }
        #[test]
        fn run_command_limit_counts_utf16_before_registration() {
            let overhead = command_for(Path::new("")).unwrap().encode_utf16().count();
            let path = "a".repeat(260 - overhead);
            assert_eq!(
                command_for(Path::new(&path))
                    .unwrap()
                    .encode_utf16()
                    .count(),
                260
            );
            assert!(command_for(Path::new(&format!("{path}a"))).is_err());
            let unicode = format!("{}\u{1f42d}", "a".repeat(259 - overhead));
            assert!(command_for(Path::new(&unicode)).is_err());
        }
        #[test]
        fn task_manager_disabled_entries_are_not_reported_enabled() {
            assert!(approval_enabled(None));
            assert!(approval_enabled(Some(&[2, 0])));
            assert!(approval_enabled(Some(&[6, 0])));
            assert!(!approval_enabled(Some(&[3, 0])));
            assert!(!approval_enabled(Some(&[7, 0])));
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    use tauri::Manager;

    const LABEL: &str = "dev.qzflow.desktop";

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    #[serde(rename_all = "PascalCase")]
    struct LaunchAgent {
        label: String,
        program_arguments: Vec<String>,
        run_at_load: bool,
        limit_load_to_session_type: String,
    }

    fn agent_for(executable: &Path) -> Result<LaunchAgent, String> {
        Ok(LaunchAgent {
            label: LABEL.into(),
            program_arguments: vec![
                executable
                    .to_str()
                    .ok_or("Startup executable path is not valid Unicode.")?
                    .into(),
                "--autostart".into(),
            ],
            run_at_load: true,
            limit_load_to_session_type: "Aqua".into(),
        })
    }

    fn location(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(app
            .path()
            .home_dir()
            .map_err(|error| error.to_string())?
            .join("Library/LaunchAgents/dev.qzflow.desktop.plist"))
    }

    pub fn is_enabled(app: &AppHandle) -> Result<bool, String> {
        let file = match fs::File::open(location(app)?) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.to_string()),
        };
        let actual: LaunchAgent = plist::from_reader(file).map_err(|error| error.to_string())?;
        Ok(actual == agent_for(&std::env::current_exe().map_err(|error| error.to_string())?)?)
    }

    fn write_agent(path: &Path, agent: &LaunchAgent) -> Result<(), String> {
        let parent = path.parent().ok_or("Invalid login settings directory.")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = parent.join(format!(".qz-flow-login-{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        let result = (|| {
            plist::to_writer_xml(&mut file, agent).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            drop(file);
            fs::rename(&temporary, path).map_err(|error| error.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
        let path = location(app)?;
        if enabled {
            write_agent(
                &path,
                &agent_for(&std::env::current_exe().map_err(|error| error.to_string())?)?,
            )
        } else {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn launch_agent_round_trip_preserves_xml_characters_and_arguments() {
            let agent = agent_for(Path::new(
                "/Users/A & B/<apps>/QZ Flow.app/Contents/MacOS/qz-flow",
            ))
            .unwrap();
            let mut bytes = Vec::new();
            plist::to_writer_xml(&mut bytes, &agent).unwrap();
            assert_eq!(plist::from_bytes::<LaunchAgent>(&bytes).unwrap(), agent);
        }
        #[test]
        fn failed_staging_preserves_existing_login_file() {
            let parent = std::env::temp_dir().join(format!("qz-login-test-{}", std::process::id()));
            fs::create_dir_all(&parent).unwrap();
            let path = parent.join("startup.plist");
            fs::write(&path, b"previous").unwrap();
            let temporary = parent.join(format!(".qz-flow-login-{}.tmp", std::process::id()));
            fs::write(&temporary, b"busy").unwrap();
            assert!(write_agent(
                &path,
                &agent_for(Path::new(
                    "/Applications/QZ Flow.app/Contents/MacOS/qz-flow"
                ))
                .unwrap()
            )
            .is_err());
            assert_eq!(fs::read(&path).unwrap(), b"previous");
            fs::remove_file(temporary).unwrap();
            fs::remove_file(path).unwrap();
            fs::remove_dir(parent).unwrap();
        }
    }
}
