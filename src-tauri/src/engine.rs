use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

    fn action(&self, input: Input) -> Action {
        match input {
            Input::Back => self.back,
            Input::Forward => self.forward,
            Input::Middle => self.middle,
            Input::WheelLeft => self.wheel_left,
            Input::WheelRight => self.wheel_right,
            Input::Thumb => self.thumb,
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

    fn button_bit(self) -> u8 {
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
