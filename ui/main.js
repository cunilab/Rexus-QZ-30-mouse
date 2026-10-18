"use strict";

const inputs = [
  ["middle", "Middle click", "Press the scroll wheel", "1"],
  ["back", "Back button", "Thumb button · rear", "2"],
  ["forward", "Forward button", "Thumb button · front", "3"],
  ["wheel_right", "Side up", "Turn the side wheel up", "↑"],
  ["wheel_left", "Side down", "Turn the side wheel down", "↓"],
  ["thumb", "Thumb-rest button", "Press the thumb-rest paddle", "4"],
];
const actions = [
  ["default", "Default behavior"], ["copy", "Copy"], ["paste", "Paste"],
  ["undo", "Undo"], ["redo", "Redo"], ["select_all", "Select all"],
  ["desktop", "Show desktop (OS shortcut)"],
  ["task_view", "Task View / Mission Control"],
  ["volume_up", "Volume up"], ["volume_down", "Volume down"],
  ["browser_back", "Browser back"], ["browser_forward", "Browser forward"],
];
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

for (const [key, label, description, number] of inputs) {
  const row = document.createElement("div");
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
  for (const [value, name] of actions) select.add(new Option(name, value));
  select.addEventListener("change", () => {
    draft[key] = select.value;
    render();
  });
  row.append(marker, labelElement, select);
  byId("mapping-rows").append(row);
}

function isDirty() {
  return inputs.some(([key]) => draft[key] !== saved[key]);
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
  saved = { ...defaults, ...next.mappings };
  if (!initialized || adoptMappings || !dirty) draft = { ...saved };
  initialized = true;
  connectionFailed = false;
  render();
  if (next.error) showMessage(next.error, true);
}

function render() {
  const preview = !invoke;
  const enabled = Boolean(status?.enabled);
  const detected = Boolean(status?.devices?.length);
  const dirty = isDirty();
  const hasMapping = inputs.some(([key]) => saved[key] !== "default");
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
    byId(`mapping-${key}`).value = draft[key];
    byId(`mapping-${key}`).disabled = (key === "thumb" && !status?.thumb_available) || busy || enabled || (!preview && unavailable);
  }
  byId("save").disabled = unavailable || busy || enabled || !dirty;
  byId("save").textContent = busy ? "Please wait…" : "Save assignments ↗";
  byId("reset").disabled = busy || enabled || (!preview && unavailable) || inputs.every(([key]) => draft[key] === "default");
  byId("enable").disabled = busy || (unavailable && !enabled) || (!enabled && (!detected || !hasMapping || dirty));
  byId("enable").setAttribute("aria-checked", String(enabled));
  byId("enable-detail").textContent = enabled ? (status.hook_running ? "Mappings are active. Switch off to edit." : "Mappings enabled; input listener is not running.") : preview ? "Desktop connection required to enable." : connectionFailed ? "Waiting for the desktop connection." : dirty ? "Save your changes before enabling." : !detected ? "Connect a compatible receiver first." : !hasMapping ? "Choose and save at least one shortcut." : status?.permission === "required" ? "Enable to request macOS input permissions." : "Ready when you are. Enable to start.";
  byId("permission").hidden = status?.platform !== "macos" || status?.permission !== "required";
  byId("permission").disabled = busy;
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
  if (!byId("save").disabled) runCommand("save_mappings", { mappings: { ...draft } }, "Assignments saved on this computer.");
});
byId("reset").addEventListener("click", () => {
  draft = { ...defaults };
  render();
  showMessage("Defaults restored in your draft. Save to apply.");
});
byId("enable").addEventListener("click", () => {
  if (!byId("enable").disabled) runCommand("set_enabled", { enabled: !status.enabled }, status.enabled ? "Mappings disabled." : "Mappings enabled.");
});
byId("permission").addEventListener("click", () => runCommand("open_input_settings", {}, "Input settings opened. Grant access, then enable mappings again."));
document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });
render();
refresh();
setInterval(refresh, 2000);
