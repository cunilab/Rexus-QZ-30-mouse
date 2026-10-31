# Action behavior

The editor keeps changes as a draft until **Save and apply**. Existing mappings continue during editing. Custom shortcuts use distinct Control, Shift, Alt/Option, and Windows/Command modifiers plus one key. Key names follow the US physical-key layout; other layouts need testing.

| Action | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Previous / next desktop | Ctrl+Win+Left / Right | Ctrl+Left / Right | Ctrl+Alt+Left / Right |
| Task View / Mission Control | Win+Tab | Ctrl+Up | Super (GNOME Activities; opens the launcher on KDE) |
| Show desktop | Win+D | F11 (configured Mission Control shortcut) | Not supported |
| Switch apps | Alt+Tab | Command+Tab | Alt+Tab |
| Minimize | Native foreground-window minimize | Command+M | Not supported |
| Maximize / restore | Native foreground-window toggle | Toggle full screen: Control+Command+F | Not supported |
| Previous / next tab | Ctrl+Shift+Tab / Ctrl+Tab | Control+Shift+Tab / Control+Tab | Ctrl+Shift+Tab / Ctrl+Tab |
| New / close / reopen tab | Ctrl+T / Ctrl+W / Ctrl+Shift+T | Command+T / Command+W / Command+Shift+T | Ctrl+T / Ctrl+W / Ctrl+Shift+T |
| Refresh | Ctrl+R | Command+R | Ctrl+R |
| Editing | Ctrl-based shortcuts | Command-based shortcuts; Redo adds Shift | Ctrl-based shortcuts; Redo is Ctrl+Shift+Z |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 | Command+= / Command+- / Command+0 | Ctrl+= / Ctrl+- / Ctrl+0 |
| Screenshot | Win+Shift+S | Command+Shift+3 | Print Screen key |
| Media | Native Windows media keys | Native AppKit media events | Media keys from a virtual keyboard |

Linux sends shortcuts through a `uinput` virtual keyboard, so they follow the desktop's and each application's bindings. Back/Forward use Alt+Left/Right. Actions without a shortcut shared by GNOME and KDE (Show desktop, Minimize, Maximize) are rejected when saving rather than guessed; use a custom shortcut for your desktop. Linux cannot see physically held keyboard modifiers, so holding Ctrl/Alt/Shift/Super combines with the injected shortcut instead of bypassing it, and on X11 the injected modifier release can release a modifier you are physically holding. The virtual mouse mirrors only key and relative-motion events (not absolute-axis, LED or switch events). A side wheel that reports only high-resolution events triggers once per accumulated 120-unit notch; reports that do not complete a notch pass through to applications.

Browser/tab/zoom commands follow the foreground application's shortcuts. Window commands target the foreground window. Mac full screen and Windows maximize have different behavior and labels. Desktop switching needs multiple desktops/Spaces and enabled OS shortcuts. Side-wheel desktop switching is limited to one step per 250 ms so rapid rotation cannot flood system navigation. Check the machine's Mission Control settings and each target application's shortcuts before relying on them. Standard combinations are referenced in [Microsoft's shortcut guide](https://support.microsoft.com/en-au/windows/keyboard-shortcuts-in-windows-dcc61a57-8ff0-cffe-9796-cb9706c75eec); Mac media identifiers follow [Apple's published HID definitions](https://github.com/apple-oss-distributions/IOHIDFamily/blob/main/IOHIDSystem/IOKit/hidsystem/ev_keymap.h).

## Record a shortcut

Choose **Custom keyboard shortcut**, then **Record shortcut**. Keep QZ Flow focused, release held keys, press the chord, and release it. Escape, loss of focus, or ten seconds cancels recording. Captured releases are drained before the recorder closes, with a bounded cleanup timeout. Recording temporarily bypasses mapped mouse actions. Injected events are not used for recording; OS-reserved and secure shortcuts may bypass native hooks. Use the manual key/modifier controls for reserved chords. macOS requires Accessibility and Input Monitoring; recording never requests permission in the background. Linux does not support recording yet; choose the key and modifiers manually.

Recording produces a draft, and does not execute that shortcut as a test. Do not use secure OS sequences such as Ctrl+Alt+Delete or lock-screen chords as a recording test. Actual native capture and non-US layouts still need hardware verification.

## Launch an application

Choose **Launch application**, then **Choose application**. Windows accepts an existing absolute `.exe` path (started with the shell, so apps that require elevation show the Windows prompt); Mac accepts a `.app` bundle; Linux accepts an absolute path to an executable file and needs `zenity` or `kdialog` for the picker. Only the path's shape is checked when saving, so a missing app never disables your other mappings. Arguments, scripts, and shell commands are not supported. The selected path is persisted only after Save. Launching is dispatched outside the input listener, with failures shown in status. An application that is removed or moved reports an error when pressed and must be selected again.

## Disable and verification

**Default behavior** passes native input through. **Disable button** consumes OS-hook events. It is offered for Middle, Back, and Forward on Windows; raw receiver wheel/thumb input cannot be reliably suppressed. Mac and Linux support OS mouse/scroll events (Linux can consume the side wheel) but have no thumb-rest support.

Verify one assignment at a time with the original mouse utility closed: desktop switching with two desktops, a tab/editing action in a disposable document, media controls, and launch of a known application. Confirm one action per press, paired key release, Pause/Resume, disconnect/reconnect, and Quit. Native dialogs, recording, and physical Windows/Mac execution are separate from unit/build checks; they are not claimed as verified by compilation.


## Input failure recovery

On Windows with the compatible USB receiver, a custom button4 action first initializes the receiver in the background. Wait for **Mappings active**. Keep Mouse Assistant closed. Setup failures pause mappings and show an error; Save and apply retries. Pause, Default behavior for button4, and normal Quit restore the OEM's default input mode. Forced termination cannot guarantee that reset. Over Bluetooth, button 4's input path is not identified, so a non-default button4 action is rejected on every OS with "Button 4 over Bluetooth is not supported yet; use the USB receiver." macOS thumb remapping remains unverified.

If Windows accepts only part of a shortcut, QZ Flow accounts for those inserted events and checks immediate release recovery. A failure pauses mappings for the current session. It does not force delayed key-up events, since a key may now be physically held. Follow the displayed instruction to press and release Ctrl, Alt, Shift and Windows, then resume only when typing is normal. Failed cleanup blocks new injection until the pending keys are confirmed released. Forced process termination cannot guarantee cleanup. Native runtime verification is still required for beta.5; a passing fake-sender test is not a hardware pass.
