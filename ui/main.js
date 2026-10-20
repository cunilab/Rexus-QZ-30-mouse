"use strict";

const inputs = [
  ["middle", "Middle click", "Press the scroll wheel", "1"],
  ["back", "Back button", "Thumb button · rear", "2"],
  ["forward", "Forward button", "Thumb button · front", "3"],
  ["wheel_right", "Side up", "Turn the side wheel up", "↑"],
  ["wheel_left", "Side down", "Turn the side wheel down", "↓"],
  ["thumb", "Thumb-rest button", "Press the thumb-rest paddle", "4"],
];
const actionGroups = [
  ["Mouse", [["default", "Default behavior"], ["disable", "Disable button"]]],
  ["Editing", [["copy", "Copy"], ["paste", "Paste"], ["cut", "Cut"], ["undo", "Undo"], ["redo", "Redo"], ["select_all", "Select all"]]],
  ["Desktop and windows", [["desktop", "Show desktop"], ["task_view", "Task View / Mission Control"], ["previous_desktop", "Previous desktop"], ["next_desktop", "Next desktop"], ["switch_apps", "Switch apps"], ["minimize", "Minimize window"], ["maximize", "Maximize / restore window"]]],
  ["Browser and tabs", [["browser_back", "Browser back"], ["browser_forward", "Browser forward"], ["previous_tab", "Previous tab"], ["next_tab", "Next tab"], ["new_tab", "New tab"], ["close_tab", "Close tab"], ["reopen_tab", "Reopen closed tab"], ["refresh", "Refresh"]]],
  ["Media", [["volume_up", "Volume up"], ["volume_down", "Volume down"], ["mute", "Mute"], ["play_pause", "Play / pause"], ["previous_track", "Previous track"], ["next_track", "Next track"]]],
  ["Zoom and capture", [["zoom_in", "Zoom in"], ["zoom_out", "Zoom out"], ["zoom_reset", "Reset zoom"], ["screenshot", "Screenshot"]]],
  ["Custom", [["custom_shortcut", "Custom keyboard shortcut"], ["launch_application", "Launch application"]]],
];
const shortcutKeys = [
  ...Array.from({ length: 26 }, (_, index) => `Key${String.fromCharCode(65 + index)}`),
  ...Array.from({ length: 10 }, (_, index) => `Digit${index}`),
  ...Array.from({ length: 12 }, (_, index) => `F${index + 1}`),
  "ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Space", "Enter", "Tab", "Escape", "Backspace", "Delete", "Home", "End", "PageUp", "PageDown", "Insert", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Comma", "Period", "Slash", "Backquote",
];
const modifiers = ["control", "shift", "alt", "meta"];
const inputLabels = { middle: "Middle button", back: "Back button", forward: "Forward button", wheel_left: "Side wheel down", wheel_right: "Side wheel up", thumb: "Thumb-rest button (4)" };
const clone = (value) => JSON.parse(JSON.stringify(value));
const actionKind = (action) => typeof action === "string" ? action : Object.keys(action)[0];
function actionSignature(action) {
  if (action?.custom_shortcut) return JSON.stringify(["custom_shortcut", action.custom_shortcut.key, [...action.custom_shortcut.modifiers].sort()]);
  if (action?.launch_application) return JSON.stringify(["launch_application", action.launch_application.path]);
  return JSON.stringify(action);
}
const byId = (id) => document.getElementById(id);
const invoke = window.__TAURI__?.core?.invoke;
const defaults = Object.fromEntries(inputs.map(([key]) => [key, "default"]));
let saved = { ...defaults };
let draft = { ...defaults };
let status = null;
let busy = false;
let polling = false;
let pendingRefresh = null;
let initialized = false;
let connectionFailed = false;
let messageTimer;
let selectedInput = "middle";
let recording = false;
const actionOptions = [];

for (const [key, label, description, number] of inputs) {
  const row = document.createElement("div");
  row.id = `row-${key}`;
  row.className = "mapping-row";
  const marker = document.createElement("span");
  marker.className = "input-number";
  marker.textContent = number;
  const labelElement = document.createElement("label");
  labelElement.htmlFor = `mapping-${key}`;
  labelElement.textContent = label;
  const detail = document.createElement("small");
  detail.textContent = description;
  labelElement.append(detail);
  const select = document.createElement("select");
  select.id = `mapping-${key}`;
  for (const [name, actions] of actionGroups) {
    const group = document.createElement("optgroup");
    group.label = name;
    for (const [value, label] of actions) {
      const option = new Option(label, value);
      if (value === "maximize") actionOptions.push(option);
      group.append(option);
    }
    select.append(group);
  }
  select.addEventListener("focus", () => { selectedInput = key; render(); });
  select.addEventListener("change", () => {
    selectedInput = key;
    draft[key] = select.value === "custom_shortcut" ? { custom_shortcut: { key: "KeyC", modifiers: [status?.platform === "macos" ? "meta" : "control"] } } : select.value === "launch_application" ? { launch_application: { path: "" } } : select.value;
    render();
  });
  byId(`marker-${key}`).addEventListener("click", () => {
    if (byId(`marker-${key}`).disabled) return;
    selectedInput = key;
    render();
    select.focus();
  });
  row.append(marker, labelElement, select);
  byId("mapping-rows").append(row);
}
for (const key of shortcutKeys) byId("shortcut-key").add(new Option(key.replace(/^Key/, "").replace(/^Digit/, ""), key));

function showPage(settings) {
  byId("controls-page").hidden = settings;
  byId("settings-page").hidden = !settings;
  byId("nav-controls").setAttribute("aria-current", settings ? "false" : "page");
  byId("nav-settings").setAttribute("aria-current", settings ? "page" : "false");
  byId("page-name").textContent = settings ? "Settings" : "QZ30 Black";
  byId(settings ? "settings-title" : "controls-title").focus();
}

byId("nav-controls").addEventListener("click", () => showPage(false));
byId("nav-settings").addEventListener("click", () => showPage(true));

function isDirty() {
  return inputs.some(([key]) => actionSignature(draft[key]) !== actionSignature(saved[key]));
}

function showMessage(text, error = false) {
  clearTimeout(messageTimer);
  byId("message").textContent = text;
  byId("message").classList.toggle("error", error);
  byId("message").setAttribute("role", error ? "alert" : "status");
  byId("message").hidden = !text;
  if (!error) messageTimer = setTimeout(() => { byId("message").hidden = true; }, 5000);
}

function acceptStatus(next, adoptMappings = false) {
  const dirty = isDirty();
  status = next;
  saved = clone({ ...defaults, ...next.mappings });
  if (!initialized || adoptMappings || !dirty) draft = clone(saved);
  initialized = true;
  connectionFailed = false;
  render();
  if (next.error) showMessage(next.error, true);
}

function render() {
  const preview = !invoke;
  const detected = Boolean(status?.devices?.length);
  const dirty = isDirty();
  const retry = status?.paused === true || status?.mapping_state === "unavailable";
  const unavailable = preview || !initialized || connectionFailed;
  byId("preview").hidden = !preview;
  byId("receiver-status").textContent = preview ? "Interface preview" : connectionFailed ? "Connection unavailable" : detected ? "Compatible receiver detected" : initialized ? "No compatible receiver detected" : "Checking receiver…";
  byId("receiver-detail").textContent = preview ? "Open the desktop app to connect." : connectionFailed ? "Retrying the desktop connection automatically." : detected ? "Receiver present · mouse power is not verified" : "Connect your QZ30 receiver to get started.";
  byId("receiver-dot").classList.toggle("connected", detected && !connectionFailed);
  byId("connection").textContent = detected ? status.devices.map((device) => device.transport || "USB").filter((value, index, all) => all.indexOf(value) === index).join(", ") : "—";
  byId("last-input").textContent = status?.last_input || "No input yet";
  byId("platform").textContent = status?.platform === "macos" ? "macOS · local utility" : status?.platform === "windows" ? "Windows · local utility" : "Windows + macOS";
  byId("draft-state").textContent = dirty ? "Unsaved changes" : preview ? "Preview" : initialized ? "Saved locally" : "Connecting";
  byId("draft-state").classList.toggle("dirty", dirty);
  for (const [key] of inputs) {
    byId(`mapping-${key}`).value = actionKind(draft[key]);
    byId(`mapping-${key}`).disabled = (key === "thumb" && !status?.thumb_available) || busy || (!preview && unavailable);
    for (const option of byId(`mapping-${key}`).options) {
      if (option.value === "disable") option.hidden = option.disabled = status?.platform !== "macos" && ["wheel_left", "wheel_right", "thumb"].includes(key);
    }
    byId(`marker-${key}`).disabled = byId(`mapping-${key}`).disabled;
    byId(`marker-${key}`).setAttribute("aria-pressed", String(key === selectedInput));
    byId(`marker-${key}`).classList.toggle("last-input", status?.last_input === inputLabels[key]);
    byId(`row-${key}`).classList.toggle("selected", key === selectedInput);
    byId(`row-${key}`).classList.toggle("last-input", status?.last_input === inputLabels[key]);
  }
  const incomplete = inputs.filter(([key]) => draft[key]?.launch_application?.path === "").map(([, label]) => label);
  byId("save").disabled = unavailable || busy || incomplete.length > 0 || (!dirty && !retry);
  byId("save").textContent = busy ? "Please wait…" : "Save and apply";
  byId("reset").disabled = busy || (!preview && unavailable) || inputs.every(([key]) => draft[key] === "default");
  const mappingState = status?.mapping_state;
  byId("mapping-status").textContent = preview ? "Interface preview" : connectionFailed ? "Connection unavailable" : !initialized ? "Checking mappings..." : mappingState === "preparing" ? "Preparing button 4" : mappingState === "active" ? "Mappings active" : mappingState === "paused" ? "Mappings paused" : mappingState === "default" ? "Default behavior" : "Mappings unavailable";
  byId("mapping-detail").textContent = preview ? "Open the desktop app to apply assignments." : connectionFailed ? "Waiting for the desktop connection." : !initialized ? "Reading saved assignments." : status?.paused ? "Save and apply resumes mappings. You can also Resume from the tray or menu bar." : mappingState === "preparing" ? "Waiting for the receiver to confirm button 4 is ready." : mappingState === "unavailable" ? "Save and apply to retry. Check the error message and macOS input permissions if required." : dirty ? "Your edits are a draft. Current behavior continues until you Save and apply." : mappingState === "active" ? "Saved assignments are running. Pause or Resume from the tray or menu bar." : "Choose an assignment, then Save and apply.";
  byId("permission").hidden = status?.platform !== "macos" || status?.permission !== "required";
  byId("permission").disabled = busy;
  const startupAvailable = typeof status?.launch_at_login === "boolean";
  byId("launch-at-login").checked = status?.launch_at_login === true;
  byId("launch-at-login").disabled = unavailable || busy || !startupAvailable;
  byId("startup-detail").textContent = preview ? "Open the desktop app to change login settings." : connectionFailed ? "Login settings unavailable while reconnecting." : !initialized ? "Checking login settings…" : !startupAvailable ? "Launch at login is unavailable on this system." : status.launch_at_login ? "QZ Flow will start quietly when you log in." : "QZ Flow will only start when you open it.";
  for (const option of actionOptions) option.label = status?.platform === "macos" ? "Toggle full screen" : "Maximize / restore window";
  const selected = draft[selectedInput];
  const editorDisabled = byId(`mapping-${selectedInput}`).disabled;
  byId("editor-title").textContent = `${inputs.find(([key]) => key === selectedInput)[1]} action`;
  byId("shortcut-editor").hidden = !selected?.custom_shortcut;
  byId("application-editor").hidden = !selected?.launch_application;
  byId("editor-hint").textContent = recording ? "Recording: press a shortcut. Escape cancels; recording ends after 10 seconds. Finish or cancel before saving." : incomplete.length ? `Choose an application for ${incomplete.join(", ")} before saving.` : "Select a mouse marker or button row to edit. Actions use OS and app shortcuts, which may need enabling in system settings. Disable is unavailable for the Windows receiver side wheel and thumb-rest button. The last detected input stays highlighted.";
  byId("shortcut-key").value = selected?.custom_shortcut?.key || "KeyC";
  byId("shortcut-key").disabled = editorDisabled;
  for (const option of byId("shortcut-key").options) {
    if (option.value === "Insert") option.hidden = option.disabled = status?.platform === "macos";
  }
  for (const modifier of modifiers) {
    byId(`modifier-${modifier}`).checked = selected?.custom_shortcut?.modifiers.includes(modifier) || false;
    byId(`modifier-${modifier}`).disabled = editorDisabled;
  }
  byId("record-shortcut").disabled = editorDisabled || unavailable;
  byId("record-shortcut").textContent = recording ? "Recording... Escape to cancel" : "Record shortcut";
  byId("application-path").value = selected?.launch_application?.path || "";
  byId("pick-application").disabled = editorDisabled || unavailable;
}

async function refresh() {
  if (!invoke || busy || polling || document.hidden) return;
  polling = true;
  try {
    pendingRefresh = invoke("get_status");
    acceptStatus(await pendingRefresh);
  } catch (error) {
    connectionFailed = true;
    render();
    showMessage(`Could not read device status: ${String(error)}`, true);
  } finally {
    polling = false;
    pendingRefresh = null;
  }
}

async function runCommand(command, args, success) {
  if (!invoke || busy) return;
  busy = true;
  render();
  try {
    if (pendingRefresh) await pendingRefresh.catch(() => {});
    const next = await invoke(command, args);
    if (next && command !== "open_input_settings") acceptStatus(next, command === "save_mappings" && !next.error);
    if (!next?.error) showMessage(success);
  } catch (error) {
    showMessage(String(error), true);
  } finally {
    busy = false;
    render();
    refresh();
  }
}

byId("save").addEventListener("click", () => {
  if (!byId("save").disabled) runCommand("save_mappings", { mappings: clone(draft) }, "Assignments saved and applied.");
});
byId("reset").addEventListener("click", () => {
  draft = { ...defaults };
  render();
  showMessage("Defaults restored in your draft. Save to apply.");
});
byId("permission").addEventListener("click", () => runCommand("open_input_settings", {}, "Input settings opened. Grant access, then Save and apply again."));
byId("launch-at-login").addEventListener("change", () => {
  if (!byId("launch-at-login").disabled) runCommand("set_launch_at_login", { enabled: byId("launch-at-login").checked }, "Login setting updated.");
});
function updateShortcut() {
  if (byId("shortcut-key").disabled || !draft[selectedInput]?.custom_shortcut) return;
  draft[selectedInput] = { custom_shortcut: { key: byId("shortcut-key").value, modifiers: modifiers.filter((modifier) => byId(`modifier-${modifier}`).checked) } };
  render();
}
byId("shortcut-key").addEventListener("change", updateShortcut);
for (const modifier of modifiers) byId(`modifier-${modifier}`).addEventListener("change", updateShortcut);

async function editFromNative(command) {
  if (!invoke || busy || byId(command === "record_shortcut" ? "record-shortcut" : "pick-application").disabled) return;
  const target = selectedInput;
  busy = true;
  recording = command === "record_shortcut";
  render();
  try {
    if (pendingRefresh) await pendingRefresh.catch(() => {});
    const result = await invoke(command);
    if (result !== null) {
      draft[target] = command === "record_shortcut" ? clone(result) : { launch_application: { path: result } };
      showMessage("Draft updated. Save and apply to activate.");
    } else showMessage("Canceled. Your draft is unchanged.");
  } catch (error) {
    showMessage(String(error), true);
  } finally {
    busy = false;
    recording = false;
    render();
    refresh();
  }
}
byId("record-shortcut").addEventListener("click", () => editFromNative("record_shortcut"));
byId("pick-application").addEventListener("click", () => editFromNative("pick_application"));
document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });
render();
refresh();
setInterval(refresh, 2000);
