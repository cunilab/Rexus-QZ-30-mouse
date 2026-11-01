use super::INJECTED_TAG;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    virtual_key: u16,
    scan: u16,
    flags: u32,
}

impl Key {
    fn release(self) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: self.virtual_key,
                    wScan: self.scan,
                    dwFlags: self.flags | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: INJECTED_TAG,
                },
            },
        }
    }
}

#[derive(Default)]
pub(super) struct Keyboard {
    pending: Vec<Key>,
}

impl Keyboard {
    pub(super) const fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    fn account(&mut self, inputs: &[INPUT], inserted: usize) {
        // SendInput inserts events serially; only its accepted prefix changes ownership.
        for input in inputs.iter().take(inserted) {
            if input.r#type != INPUT_KEYBOARD {
                continue;
            }
            let input = unsafe { input.Anonymous.ki };
            let key = Key {
                virtual_key: input.wVk,
                scan: input.wScan,
                flags: input.dwFlags & !KEYEVENTF_KEYUP,
            };
            if input.dwFlags & KEYEVENTF_KEYUP != 0 {
                if let Some(index) = self.pending.iter().rposition(|pending| *pending == key) {
                    self.pending.remove(index);
                }
            } else {
                self.pending.push(key);
            }
        }
    }

    pub(super) fn recover(
        &mut self,
        is_up: &mut impl FnMut(u16, u16, u32) -> Option<bool>,
    ) -> Result<(), String> {
        // After dispatch, a physical press may own the same key. Never send delayed key-ups.
        if self
            .pending
            .iter()
            .all(|key| is_up(key.virtual_key, key.scan, key.flags) == Some(true))
        {
            self.pending.clear();
            Ok(())
        } else {
            Err("QZ Flow cannot confirm that its previous shortcut keys are released. Remapping is paused. Manually press and release the shortcut keys (including Ctrl, Alt, Shift and Windows), then resume remapping.".into())
        }
    }

    fn cleanup_immediate(
        &mut self,
        send: &mut impl FnMut(&[INPUT]) -> usize,
    ) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let releases: Vec<_> = self.pending.iter().rev().map(|key| key.release()).collect();
        let inserted = send(&releases);
        self.account(&releases, inserted);
        if self.pending.is_empty() && inserted == releases.len() {
            Ok(())
        } else {
            Err("Windows could not release all QZ Flow shortcut keys. Remapping is paused. Press and release Ctrl, Alt, Shift and Windows, check for an elevated app blocking input, then resume remapping.".into())
        }
    }

    pub(super) fn send(
        &mut self,
        inputs: &[INPUT],
        send: &mut impl FnMut(&[INPUT]) -> usize,
        is_up: &mut impl FnMut(u16, u16, u32) -> Option<bool>,
    ) -> Result<(), String> {
        self.recover(is_up)?;
        let inserted = send(inputs);
        self.account(inputs, inserted);
        if inserted == inputs.len() {
            return Ok(());
        }
        self.cleanup_immediate(send)?;
        Err("Windows rejected part of a shortcut; its inserted keys were released. Remapping is paused. Release all keys and check for an elevated app blocking input, then resume remapping.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{key_input, shortcut_inputs};
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        KEYEVENTF_EXTENDEDKEY, KEYEVENTF_SCANCODE, VK_CONTROL, VK_LEFT, VK_LWIN,
    };

    fn chord(keys: &[u16], scan: Option<u16>) -> Vec<INPUT> {
        let mut inputs = shortcut_inputs(keys, scan, false);
        inputs.extend(shortcut_inputs(keys, scan, true));
        inputs
    }

    fn virtual_keys(inputs: &[INPUT]) -> Vec<u16> {
        inputs
            .iter()
            .map(|input| unsafe { input.Anonymous.ki.wVk })
            .collect()
    }

    #[test]
    fn complete_shortcut_leaves_no_owned_keys_or_extra_cleanup() {
        let mut keyboard = Keyboard::default();
        let inputs = chord(&[VK_CONTROL, VK_LWIN, VK_LEFT], None);
        let mut calls = 0;
        keyboard
            .send(
                &inputs,
                &mut |batch| {
                    calls += 1;
                    batch.len()
                },
                &mut |_, _, _| Some(true),
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert!(keyboard.pending.is_empty());
        keyboard
            .recover(&mut |_, _, _| panic!("Nothing needs checking"))
            .unwrap();
    }

    #[test]
    fn every_partial_prefix_releases_only_its_unmatched_inserted_downs() {
        let inputs = chord(&[VK_CONTROL, VK_LWIN, VK_LEFT], None);
        let expected = [
            vec![],
            vec![VK_CONTROL],
            vec![VK_LWIN, VK_CONTROL],
            vec![VK_LEFT, VK_LWIN, VK_CONTROL],
            vec![VK_LWIN, VK_CONTROL],
            vec![VK_CONTROL],
        ];
        for (prefix, expected_release) in expected.into_iter().enumerate() {
            let mut keyboard = Keyboard::default();
            let mut calls = 0;
            let error = keyboard
                .send(
                    &inputs,
                    &mut |batch| {
                        calls += 1;
                        if calls == 1 {
                            return prefix;
                        }
                        assert_eq!(virtual_keys(batch), expected_release);
                        assert!(batch.iter().all(|input| unsafe {
                            input.Anonymous.ki.dwFlags & KEYEVENTF_KEYUP != 0
                        }));
                        batch.len()
                    },
                    &mut |_, _, _| Some(true),
                )
                .unwrap_err();
            assert!(error.contains("Remapping is paused"));
            assert_eq!(calls, if prefix == 0 { 1 } else { 2 });
            assert!(keyboard.pending.is_empty());
        }
    }

    #[test]
    fn failed_cleanup_retains_only_pending_keys_and_blocks_new_injection_until_recovered() {
        let inputs = chord(&[VK_CONTROL, VK_LWIN, VK_LEFT], None);
        let mut keyboard = Keyboard::default();
        let mut calls = 0;
        keyboard
            .send(
                &inputs,
                &mut |batch| {
                    calls += 1;
                    if calls == 1 {
                        return 2;
                    }
                    assert_eq!(virtual_keys(batch), [VK_LWIN, VK_CONTROL]);
                    1
                },
                &mut |_, _, _| Some(true),
            )
            .unwrap_err();
        assert_eq!(
            keyboard
                .pending
                .iter()
                .map(|key| key.virtual_key)
                .collect::<Vec<_>>(),
            [VK_CONTROL]
        );
        for state in [Some(false), None] {
            keyboard
                .send(
                    &inputs,
                    &mut |_| panic!("Delayed recovery must never inject releases or a new chord"),
                    &mut |key, _, _| {
                        assert_eq!(key, VK_CONTROL);
                        state
                    },
                )
                .unwrap_err();
            keyboard.recover(&mut |_, _, _| state).unwrap_err();
        }
        assert_eq!(keyboard.pending.len(), 1);
        calls = 0;
        keyboard
            .send(
                &inputs,
                &mut |batch| {
                    calls += 1;
                    assert_eq!(batch.len(), inputs.len());
                    batch.len()
                },
                &mut |_, _, _| Some(true),
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert!(keyboard.pending.is_empty());
    }

    #[test]
    fn delayed_manual_recovery_requires_every_pending_key_up_and_preserves_scan_identity() {
        let mut keyboard = Keyboard::default();
        let inputs = shortcut_inputs(&[VK_CONTROL, VK_LEFT], Some(0xe04b), false);
        keyboard.account(&inputs, inputs.len());
        keyboard
            .recover(&mut |key, scan, flags| {
                if key == VK_CONTROL {
                    Some(true)
                } else {
                    assert_eq!(key, 0);
                    assert_eq!(scan, 0x4b);
                    assert_eq!(flags, KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY);
                    Some(false)
                }
            })
            .unwrap_err();
        assert_eq!(keyboard.pending.len(), 2);
        keyboard.recover(&mut |_, _, _| Some(true)).unwrap();
        assert!(keyboard.pending.is_empty());
    }

    #[test]
    fn cleanup_preserves_custom_scan_encoding_and_never_releases_unowned_keys() {
        let mut keyboard = Keyboard::default();
        let inputs = chord(&[VK_CONTROL, VK_LEFT], Some(0xe04b));
        let mut calls = 0;
        keyboard
            .send(
                &inputs,
                &mut |batch| {
                    calls += 1;
                    if calls == 1 {
                        return 2;
                    }
                    unsafe {
                        assert_eq!(batch[0].Anonymous.ki.wVk, 0);
                        assert_eq!(batch[0].Anonymous.ki.wScan, 0x4b);
                        assert_eq!(
                            batch[0].Anonymous.ki.dwFlags,
                            KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
                        );
                        assert_eq!(batch[1].Anonymous.ki.wVk, VK_CONTROL);
                    }
                    batch.len()
                },
                &mut |_, _, _| Some(true),
            )
            .unwrap_err();
        assert!(keyboard.pending.is_empty());
        keyboard.account(&[key_input(VK_LWIN, true)], 1);
        keyboard
            .recover(&mut |_, _, _| panic!("Unowned key-up must not create ownership"))
            .unwrap();
    }
}
