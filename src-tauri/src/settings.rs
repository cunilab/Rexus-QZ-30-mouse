use crate::engine::Mappings;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub mappings: Mappings,
    pub paused: bool,
}

impl Settings {
    pub fn new(mappings: Mappings, paused: bool) -> Self {
        Self { mappings, paused }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u32,
    mappings: Mappings,
    paused: bool,
}

fn current(bytes: &[u8]) -> Result<Settings, String> {
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if envelope.version != 1 && envelope.version != 2 {
        return Err(format!(
            "Unsupported settings version: {}",
            envelope.version
        ));
    }
    envelope.mappings.validate()?;
    Ok(Settings::new(envelope.mappings, envelope.paused))
}

pub fn decode(bytes: &[u8]) -> Result<Settings, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if value.get("version").is_some()
        || value.get("mappings").is_some()
        || value.get("paused").is_some()
    {
        current(bytes)
    } else {
        let mappings: Mappings = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        mappings.validate()?;
        Ok(Settings::new(mappings, false))
    }
}

pub fn load(path: &Path) -> Result<Settings, String> {
    match fs::read(path) {
        Ok(bytes) => decode(&bytes),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Cannot read settings: {error}")),
    }
}

fn unique_sibling(path: &Path, suffix: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(
        ".{name}-{}-{time}-{}.{suffix}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn preserve(backup: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup)
        .map_err(|e| format!("Cannot preserve original settings: {e}"))?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = fs::remove_file(backup);
        return Err(format!("Cannot preserve original settings: {error}"));
    }
    Ok(())
}

fn replace(path: &Path, bytes: &[u8], temporary: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary)
        .map_err(|e| format!("Cannot stage settings: {e}"))?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|e| format!("Cannot replace settings: {e}"))
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.mappings.validate()?;
    let parent = path.parent().ok_or("Invalid settings directory.")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings directory: {e}"))?;
    let bytes = serde_json::to_vec_pretty(&Envelope {
        version: 2,
        mappings: settings.mappings.clone(),
        paused: settings.paused,
    })
    .map_err(|e| format!("Cannot encode settings: {e}"))?;
    match fs::read(path) {
        Ok(original)
            if current(&original).is_err()
                || serde_json::from_slice::<Envelope>(&original)
                    .is_ok_and(|envelope| envelope.version != 2) =>
        {
            preserve(&unique_sibling(path, "backup"), &original)?;
        }
        Ok(_) => (),
        Err(error) if error.kind() == ErrorKind::NotFound => (),
        Err(error) => return Err(format!("Cannot read existing settings: {error}")),
    }
    replace(path, &bytes, &unique_sibling(path, "tmp"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Action;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = unique_sibling(&std::env::temp_dir().join("qz-settings"), "test");
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> PathBuf {
            self.0.join("mappings.json")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_and_current_round_trip_preserve_mappings_and_pause() {
        let fixture = Fixture::new();
        assert_eq!(load(&fixture.path()).unwrap(), Settings::default());
        let mappings = Mappings {
            back: Action::Copy,
            ..Mappings::default()
        };
        let legacy = serde_json::to_vec(&mappings).unwrap();
        assert_eq!(
            decode(&legacy).unwrap(),
            Settings::new(mappings.clone(), false)
        );
        let settings = Settings::new(mappings, true);
        save(&fixture.path(), &settings).unwrap();
        assert_eq!(load(&fixture.path()).unwrap(), settings);
    }

    #[test]
    fn version_one_migrates_with_exact_backup_and_custom_actions_round_trip() {
        let fixture = Fixture::new();
        let original = serde_json::to_vec(
            &serde_json::json!({"version":1,"mappings":Mappings::default(),"paused":true}),
        )
        .unwrap();
        fs::write(fixture.path(), &original).unwrap();
        let mut settings = load(&fixture.path()).unwrap();
        assert!(settings.paused);
        settings.mappings.back = Action::CustomShortcut {
            key: "KeyC".into(),
            modifiers: vec!["control".into()],
        };
        save(&fixture.path(), &settings).unwrap();
        assert_eq!(load(&fixture.path()).unwrap(), settings);
        let backup = fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "backup"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), original);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(value["version"], 2);
        let mut invalid = value;
        invalid["mappings"]["back"]["custom_shortcut"]["key"] = serde_json::json!("Unknown");
        assert!(decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }

    #[test]
    fn rejects_unsupported_unknown_duplicate_and_malformed_settings() {
        let valid = serde_json::json!({"version":1,"mappings":Mappings::default(),"paused":false});
        for (field, value) in [
            ("version", serde_json::json!(3)),
            ("extra", serde_json::json!(true)),
            ("paused", serde_json::json!("yes")),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(decode(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
        let duplicate = serde_json::to_string(&valid).unwrap().replacen(
            "\"version\":1",
            "\"version\":1,\"version\":1",
            1,
        );
        assert!(decode(duplicate.as_bytes()).is_err());
        assert!(decode(b"garbage").is_err());
        assert!(decode(br#"{"unknown":true}"#).is_err());
    }

    #[test]
    fn repeated_replacement_does_not_leave_staging_or_backup_files() {
        let fixture = Fixture::new();
        let mut settings = Settings::default();
        save(&fixture.path(), &settings).unwrap();
        settings.paused = true;
        settings.mappings.middle = Action::Paste;
        save(&fixture.path(), &settings).unwrap();
        assert_eq!(load(&fixture.path()).unwrap(), settings);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }

    #[test]
    fn legacy_corrupt_and_unsupported_originals_are_preserved_exactly() {
        let fixture = Fixture::new();
        let legacy = serde_json::to_vec(&Mappings::default()).unwrap();
        let unsupported = br#"{"version":42,"mappings":{},"paused":false}"#;
        for original in [legacy.as_slice(), b"corrupt\x00original", unsupported] {
            fs::write(fixture.path(), original).unwrap();
            save(&fixture.path(), &Settings::default()).unwrap();
        }
        let backups: Vec<_> = fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "backup")
            })
            .map(|path| fs::read(path).unwrap())
            .collect();
        assert_eq!(backups.len(), 3);
        for original in [legacy.as_slice(), b"corrupt\x00original", unsupported] {
            assert!(backups.iter().any(|backup| backup == original));
        }
    }

    #[test]
    fn failed_staging_preserves_previous_file_and_preexisting_staging() {
        let fixture = Fixture::new();
        let staging = fixture.0.join("occupied.tmp");
        fs::write(fixture.path(), b"previous").unwrap();
        fs::write(&staging, b"other operation").unwrap();
        assert!(replace(&fixture.path(), b"replacement", &staging).is_err());
        assert_eq!(fs::read(fixture.path()).unwrap(), b"previous");
        assert_eq!(fs::read(staging).unwrap(), b"other operation");
    }

    #[test]
    fn backup_creation_never_overwrites_an_existing_backup() {
        let fixture = Fixture::new();
        let backup = fixture.0.join("existing.backup");
        fs::write(fixture.path(), b"original settings").unwrap();
        fs::write(&backup, b"earlier backup").unwrap();
        assert!(preserve(&backup, b"original settings").is_err());
        assert_eq!(fs::read(backup).unwrap(), b"earlier backup");
        assert_eq!(fs::read(fixture.path()).unwrap(), b"original settings");
    }

    #[cfg(windows)]
    #[test]
    fn failed_rename_keeps_a_locked_previous_file() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        fs::write(fixture.path(), b"previous").unwrap();
        let locked = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(fixture.path())
            .unwrap();
        let staging = fixture.0.join("replacement.tmp");
        assert!(replace(&fixture.path(), b"replacement", &staging).is_err());
        assert!(!staging.exists());
        drop(locked);
        assert_eq!(fs::read(fixture.path()).unwrap(), b"previous");
    }

    #[test]
    fn failed_rename_cleans_staging_and_preserves_destination_contents() {
        let fixture = Fixture::new();
        let destination = fixture.path();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("previous"), b"previous contents").unwrap();
        let staging = fixture.0.join("replacement.tmp");
        assert!(replace(&destination, b"replacement", &staging).is_err());
        assert!(!staging.exists());
        assert_eq!(
            fs::read(destination.join("previous")).unwrap(),
            b"previous contents"
        );
    }
}
