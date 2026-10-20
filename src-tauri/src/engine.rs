use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    #[default]
    Default,
    Copy,
    Paste,
    Undo,
    Redo,
    SelectAll,
    Desktop,
    BrowserBack,
    BrowserForward,
    TaskView,
    VolumeUp,
    VolumeDown,
    Disable,
    Cut,
    PreviousDesktop,
    NextDesktop,
    SwitchApps,
    Minimize,
    Maximize,
    PreviousTab,
    NextTab,
    NewTab,
    CloseTab,
    ReopenTab,
    Refresh,
    Mute,
    PlayPause,
    PreviousTrack,
    NextTrack,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Screenshot,
    CustomShortcut {
        key: String,
        modifiers: Vec<String>,
    },
    LaunchApplication {
        path: String,
    },
}

impl Action {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::CustomShortcut { key, modifiers } => {
                let letter = key
                    .strip_prefix("Key")
                    .is_some_and(|key| key.len() == 1 && key.as_bytes()[0].is_ascii_uppercase());
                let digit = key
                    .strip_prefix("Digit")
                    .is_some_and(|key| key.len() == 1 && key.as_bytes()[0].is_ascii_digit());
                let function = key.strip_prefix('F').is_some_and(|key| {
                    key.parse::<u8>()
                        .is_ok_and(|number| (1..=12).contains(&number) && key == number.to_string())
                });
                let named = [
                    "ArrowLeft",
                    "ArrowRight",
                    "ArrowUp",
                    "ArrowDown",
                    "Space",
                    "Enter",
                    "Escape",
                    "Tab",
                    "Backspace",
                    "Delete",
                    "Home",
                    "End",
                    "PageUp",
                    "PageDown",
                    "Insert",
                    "Minus",
                    "Equal",
                    "BracketLeft",
                    "BracketRight",
                    "Backslash",
                    "Semicolon",
                    "Quote",
                    "Comma",
                    "Period",
                    "Slash",
                    "Backquote",
                ]
                .contains(&key.as_str());
                if !(letter || digit || function || named) {
                    return Err("Choose a supported shortcut key.".into());
                }
                if modifiers.len() > 4
                    || modifiers.iter().enumerate().any(|(index, modifier)| {
                        !["control", "shift", "alt", "meta"].contains(&modifier.as_str())
                            || modifiers[..index].contains(modifier)
                    })
                {
                    return Err("Shortcut modifiers must be distinct Control, Shift, Alt/Option, or Windows/Command keys.".into());
                }
            }
            Self::LaunchApplication { path }
                if path.len() > 4096
                    || path.contains('\0')
                    || !std::path::Path::new(path).is_absolute() =>
            {
                return Err("Choose an application using an absolute path.".into());
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mappings {
    pub back: Action,
    pub forward: Action,
    pub middle: Action,
    pub wheel_left: Action,
    pub wheel_right: Action,
    #[serde(default)]
    pub thumb: Action,
}

impl Mappings {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub fn actions(&self) -> [&Action; 6] {
        [
            &self.back,
            &self.forward,
            &self.middle,
            &self.wheel_left,
            &self.wheel_right,
            &self.thumb,
        ]
    }

    pub fn validate(&self) -> Result<(), String> {
        self.actions().into_iter().try_for_each(Action::validate)
    }

    fn action(&self, input: Input) -> Action {
        match input {
            Input::Back => self.back.clone(),
            Input::Forward => self.forward.clone(),
            Input::Middle => self.middle.clone(),
            Input::WheelLeft => self.wheel_left.clone(),
            Input::WheelRight => self.wheel_right.clone(),
            Input::Thumb => self.thumb.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Back,
    Forward,
    Middle,
    WheelLeft,
    WheelRight,
    #[cfg_attr(not(windows), allow(dead_code))] // Receiver thumb input is currently Windows-only.
    Thumb,
}

impl Input {
    pub fn label(self) -> &'static str {
        match self {
            Self::Back => "Back button",
            Self::Forward => "Forward button",
            Self::Middle => "Middle button",
            Self::WheelLeft => "Side wheel down",
            Self::WheelRight => "Side wheel up",
            Self::Thumb => "Thumb-rest button (4)",
        }
    }

    pub fn button_bit(self) -> u8 {
        match self {
            Self::Back => 1,
            Self::Forward => 2,
            Self::Middle => 4,
            Self::Thumb => 8,
            _ => 0,
        }
    }
}

#[derive(Default)]
pub struct Engine {
    pub enabled: bool,
    pub paused: bool,
    pub mappings: Mappings,
    pub last_input: Option<String>,
    pub error: Option<String>,
    suppressed: u8,
    navigation_at: Option<std::time::Instant>,
}

impl Engine {
    pub fn action(&mut self, input: Input, down: bool) -> Action {
        self.last_input = Some(input.label().into());
        if self.enabled && !self.paused && down && self.suppressed & input.button_bit() == 0 {
            self.mappings.action(input)
        } else {
            Action::Default
        }
    }

    pub fn navigation_throttled(
        &mut self,
        input: Input,
        action: &Action,
        now: std::time::Instant,
    ) -> bool {
        if !matches!(input, Input::WheelLeft | Input::WheelRight)
            || !matches!(action, Action::PreviousDesktop | Action::NextDesktop)
        {
            return false;
        }
        if self.navigation_at.is_some_and(|last| {
            now.saturating_duration_since(last) < std::time::Duration::from_millis(250)
        }) {
            return true;
        }
        self.navigation_at = Some(now);
        false
    }

    pub fn suppress_press(&mut self, input: Input) {
        self.suppressed |= input.button_bit();
    }

    pub fn is_suppressed(&self, input: Input) -> bool {
        self.suppressed & input.button_bit() != 0
    }

    pub fn reset_pressed(&mut self) {
        self.suppressed = 0;
    }

    pub fn suppress_release(&mut self, input: Input) -> bool {
        let bit = input.button_bit();
        let suppress = self.suppressed & bit != 0;
        self.suppressed &= !bit;
        suppress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_wheel_navigation_is_bounded_without_throttling_other_actions() {
        let mut engine = Engine::default();
        let now = std::time::Instant::now();
        assert!(!engine.navigation_throttled(Input::WheelLeft, &Action::PreviousDesktop, now));
        assert!(engine.navigation_throttled(
            Input::WheelRight,
            &Action::NextDesktop,
            now + std::time::Duration::from_millis(249)
        ));
        assert!(!engine.navigation_throttled(
            Input::WheelRight,
            &Action::NextDesktop,
            now + std::time::Duration::from_millis(250)
        ));
        assert!(!engine.navigation_throttled(Input::WheelLeft, &Action::VolumeDown, now));
        assert!(!engine.navigation_throttled(Input::Back, &Action::PreviousDesktop, now));
    }

    #[test]
    fn disabled_and_default_inputs_pass_through() {
        let mut engine = Engine::default();
        engine.mappings.back = Action::Copy;
        assert_eq!(engine.action(Input::Back, true), Action::Default);
        engine.enabled = true;
        assert_eq!(engine.action(Input::Forward, true), Action::Default);
        assert_eq!(engine.action(Input::Back, true), Action::Copy);
    }

    #[test]
    fn consumed_press_has_consumed_release_even_after_disable() {
        let mut engine = Engine {
            enabled: true,
            ..Engine::default()
        };
        engine.mappings.back = Action::Copy;
        engine.suppress_press(Input::Back);
        assert_eq!(engine.action(Input::Back, true), Action::Default);
        engine.enabled = false;
        assert!(engine.suppress_release(Input::Back));
        assert!(!engine.suppress_release(Input::Back));
        assert!(!engine.suppress_release(Input::Forward));
    }

    #[test]
    fn failed_injection_does_not_consume_original_release() {
        let mut engine = Engine {
            enabled: true,
            ..Engine::default()
        };
        engine.mappings.middle = Action::Paste;
        assert_eq!(engine.action(Input::Middle, true), Action::Paste);
        assert!(!engine.suppress_release(Input::Middle));
    }

    #[test]
    fn mapping_boundary_rejects_unknown_actions_and_fields() {
        let mut value = serde_json::to_value(Mappings::default()).unwrap();
        value["back"] = serde_json::json!("run_shell");
        assert!(serde_json::from_value::<Mappings>(value).is_err());
        let mut value = serde_json::to_value(Mappings::default()).unwrap();
        value["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Mappings>(value).is_err());
        let value = serde_json::to_string(&Mappings::default()).unwrap();
        assert!(serde_json::from_str::<Mappings>(&value)
            .unwrap()
            .is_default());
    }

    #[test]
    fn previous_settings_load_with_thumb_at_default() {
        let mappings: Mappings = serde_json::from_str(r#"{"back":"copy","forward":"default","middle":"default","wheel_left":"default","wheel_right":"default"}"#).unwrap();
        assert_eq!(mappings.back, Action::Copy);
        assert_eq!(mappings.thumb, Action::Default);
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;
    #[test]
    fn custom_actions_reject_unknown_fields_keys_and_duplicate_modifiers() {
        for text in [
            r#"{"custom_shortcut":{"key":"KeyC","modifiers":[],"extra":true}}"#,
            r#"{"launch_application":{"path":"abc","args":["--unsafe"]}}"#,
        ] {
            assert!(serde_json::from_str::<Action>(text).is_err());
        }
        for key in ["Keya", "KeyAB", "F0", "F13", "F01", "Unknown"] {
            assert!(Action::CustomShortcut {
                key: key.into(),
                modifiers: vec![]
            }
            .validate()
            .is_err());
        }
        for modifiers in [vec!["control", "control"], vec!["unknown"]] {
            assert!(Action::CustomShortcut {
                key: "KeyC".into(),
                modifiers: modifiers.into_iter().map(String::from).collect()
            }
            .validate()
            .is_err());
        }
        let action = Action::CustomShortcut {
            key: "KeyC".into(),
            modifiers: vec!["control".into()],
        };
        action.validate().unwrap();
        assert_eq!(
            serde_json::from_str::<Action>(&serde_json::to_string(&action).unwrap()).unwrap(),
            action
        );
    }
}
