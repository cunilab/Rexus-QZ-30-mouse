const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");

const source = readFileSync(join(__dirname, "../ui/main.js"), "utf8");
const html = readFileSync(join(__dirname, "../ui/index.html"), "utf8");
const tick = () => new Promise((resolve) => setImmediate(resolve));

// Only the DOM surface used by the UI; missing IDs fail instead of creating mocks.
class Element {
  constructor() {
    this.events = {};
    this.children = [];
    this.disabled = false;
    const classes = new Set();
    this.classList = { toggle(name, enabled) { if (enabled) classes.add(name); else classes.delete(name); }, contains(name) { return classes.has(name); } };
  }
  get options() { return this.children.flatMap((child) => child.children ? child.options : [child]); }
  focus() { this.focused = true; }
  append(...children) { this.children.push(...children); }
  add(option) { this.children.push(option); }
  addEventListener(name, callback) { this.events[name] = callback; }
  setAttribute(name, value) { this[name] = value; }
}

function setup(invoke) {
  const elements = new Map([...html.matchAll(/id="([^"]+)"/g)].map((match) => [match[1], new Element()]));
  let poll;
  const document = {
    hidden: false,
    getElementById(id) {
      assert(elements.has(id), `Missing DOM id: ${id}`);
      return elements.get(id);
    },
    createElement() {
      const element = new Element();
      Object.defineProperty(element, "id", { set(id) { elements.set(id, element); } });
      return element;
    },
    addEventListener() {},
  };
  vm.runInNewContext(source, {
    document,
    window: invoke ? { __TAURI__: { core: { invoke } } } : {},
    Option: function (label, value) { this.label = label; this.value = value; },
    setInterval(callback) { poll = callback; },
    setTimeout() { return 1; },
    clearTimeout() {},
  });
  return {
    get: (id) => document.getElementById(id),
    poll: () => poll(),
    document,
    change(key, value) {
      const select = document.getElementById(`mapping-${key}`);
      select.value = value;
      select.events.change();
    },
    click: (id) => document.getElementById(id).events.click(),
    edit(id, value) {
      const element = document.getElementById(id);
      if (typeof value === "boolean") element.checked = value; else element.value = value;
      element.events.change();
    },
    startup(enabled) {
      const checkbox = document.getElementById("launch-at-login");
      checkbox.checked = enabled;
      checkbox.events.change();
    },
  };
}

function nativeApp(initial = {}) {
  let status = {
    platform: "windows", devices: [{ transport: "USB" }], enabled: false,
    permission: "granted", hook_running: false, last_input: null, error: null,
    thumb_available: true,
    launch_at_login: false,
    paused: false, mapping_state: "default",
    mappings: Object.fromEntries(["back", "forward", "middle", "wheel_left", "wheel_right"].map((key) => [key, "default"])),
    ...initial,
  };
  let failure = null;
  let saveFailure = null;
  let readGate = null;
  let editorGate = null;
  const editorResults = { record_shortcut: null, pick_application: null };
  const calls = [];
  const app = setup(async (command, args) => {
    calls.push(command);
    if (failure) throw new Error(failure);
    if (command === "get_status" && readGate) await readGate;
    if (Object.hasOwn(editorResults, command)) {
      if (editorGate) await editorGate;
      return structuredClone(editorResults[command]);
    }
    if (command === "save_mappings") {
      if (saveFailure) throw new Error(saveFailure);
      const active = Object.values(args.mappings).some((action) => action !== "default");
      status = { ...status, mappings: { ...args.mappings }, paused: false, enabled: active, hook_running: active, mapping_state: active ? "active" : "default", error: null };
    }
    if (command === "set_launch_at_login") status = { ...status, launch_at_login: args.enabled };
    return structuredClone(status);
  });
  return {
    app, calls,
    get status() { return status; },
    fail(message) { failure = message; },
    failSave(message) { saveFailure = message; },
    editorResult(command, result) { editorResults[command] = result; },
    holdEditor() {
      let release;
      editorGate = new Promise((resolve) => { release = resolve; });
      return () => { editorGate = null; release(); };
    },
    holdReads() {
      let release;
      readGate = new Promise((resolve) => { release = resolve; });
      return () => { readGate = null; release(); };
    },
  };
}

test("browser preview cannot save or fabricate a receiver and has no enable switch", () => {
  const app = setup();
  assert.equal(app.get("preview").hidden, false);
  assert.equal(app.get("receiver-status").textContent, "Interface preview");
  app.change("back", "copy");
  assert.equal(app.get("save").disabled, true);
  app.click("save");
  assert(!html.includes('id="enable"'));
  assert(!source.includes('"set_enabled"'));
});

test("original numbering and actions follow native thumb availability", async () => {
  const native = nativeApp();
  const { app } = native;
  await tick();
  const rows = app.get("mapping-rows").children;
  assert.deepEqual(rows.map((row) => row.children[0].textContent), ["1", "2", "3", "↑", "↓", "4"]);
  assert.equal(app.get("mapping-thumb").disabled, false);
  const options = app.get("mapping-middle").options.map((option) => option.value);
  for (const action of ["task_view", "volume_up", "volume_down"]) assert(options.includes(action));
  app.change("wheel_right", "volume_up");
  app.click("save");
  await tick();
  assert.equal(app.get("mapping-wheel_right").value, "volume_up");
  native.status.thumb_available = false;
  native.status.platform = "macos";
  await app.poll();
  assert.equal(app.get("mapping-thumb").disabled, true);
});

test("saving applies immediately and active mappings stay editable as drafts", async () => {
  const native = nativeApp();
  const { app, calls } = native;
  await tick();
  assert.equal(app.get("receiver-status").textContent, "Compatible receiver detected");
  assert.equal(app.get("mapping-status").textContent, "Default behavior");
  app.change("back", "copy");
  await app.poll();
  assert.equal(app.get("mapping-back").value, "copy");
  assert.equal(app.get("draft-state").textContent, "Unsaved changes");
  assert.equal(native.status.enabled, false);
  app.click("save");
  await tick();
  assert.equal(native.status.enabled, true);
  assert.equal(app.get("mapping-status").textContent, "Mappings active");
  assert.equal(app.get("message").textContent, "Assignments saved and applied.");
  assert.equal(app.get("mapping-back").disabled, false);
  assert.equal(app.get("save").disabled, true);
  app.change("back", "paste");
  assert.equal(native.status.mappings.back, "copy");
  assert.equal(app.get("save").disabled, false);
  assert.match(app.get("mapping-detail").textContent, /draft/);
  await app.poll();
  assert.equal(app.get("mapping-back").value, "paste");
  assert(!calls.includes("set_enabled"));
});

test("save waits for an outstanding read and polling never overlaps", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("middle", "paste");
  const release = native.holdReads();
  const pending = native.app.poll();
  const reads = native.calls.filter((command) => command === "get_status").length;
  await native.app.poll();
  assert.equal(native.calls.filter((command) => command === "get_status").length, reads);
  native.app.click("save");
  assert(!native.calls.includes("save_mappings"));
  release();
  await pending;
  await tick();
  assert.equal(native.status.mappings.middle, "paste");
  assert.equal(native.app.get("draft-state").textContent, "Saved locally");
  native.app.document.hidden = true;
  const count = native.calls.length;
  await native.app.poll();
  assert.equal(native.calls.length, count);
});

test("native failures remain visible, preserve edits, and recover controls", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "undo");
  native.fail("test transport failure");
  native.app.click("save");
  await tick();
  assert.equal(native.app.get("message").hidden, false);
  assert.match(native.app.get("message").textContent, /test transport failure/);
  assert.equal(native.status.mappings.back, "default");
  assert.equal(native.app.get("mapping-back").value, "undo");
  native.fail(null);
  await native.app.poll();
  assert.equal(native.app.get("save").disabled, false);
  native.app.click("save");
  await tick();
  assert.equal(native.status.mappings.back, "undo");
  assert.equal(native.app.get("mapping-status").textContent, "Mappings active");
});

test("launch at login starts off, uses OS status, and preserves mapping drafts", async () => {
  const native = nativeApp();
  await tick();
  assert.equal(native.app.get("launch-at-login").checked, false);
  assert.equal(native.app.get("launch-at-login").disabled, false);
  native.app.change("back", "copy");
  native.app.startup(true);
  assert.equal(native.app.get("launch-at-login").disabled, true);
  await tick();
  assert(native.calls.includes("set_launch_at_login"));
  assert.equal(native.status.launch_at_login, true);
  assert.equal(native.app.get("launch-at-login").checked, true);
  assert.equal(native.app.get("mapping-back").value, "copy");
  assert.equal(native.app.get("draft-state").textContent, "Unsaved changes");
  native.app.startup(false);
  await tick();
  assert.equal(native.status.launch_at_login, false);
  assert.equal(native.app.get("launch-at-login").checked, false);
});

test("failed login change restores the saved setting and shows an error", async () => {
  const native = nativeApp();
  await tick();
  native.fail("OS denied login registration");
  native.app.startup(true);
  await tick();
  assert.equal(native.status.launch_at_login, false);
  assert.equal(native.app.get("launch-at-login").checked, false);
  assert.equal(native.app.get("message").hidden, false);
  assert.match(native.app.get("message").textContent, /OS denied login registration/);
  native.fail(null);
  await native.app.poll();
  assert.equal(native.app.get("launch-at-login").disabled, false);
});

test("login control is disabled in preview, before status, and when unavailable", async () => {
  const preview = setup();
  assert.equal(preview.get("launch-at-login").disabled, true);
  const native = nativeApp();
  assert.equal(native.app.get("launch-at-login").disabled, true);
  await tick();
  native.status.launch_at_login = null;
  await native.app.poll();
  assert.equal(native.app.get("launch-at-login").disabled, true);
  const count = native.calls.length;
  native.app.startup(true);
  assert.equal(native.calls.length, count);
  assert.match(native.app.get("startup-detail").textContent, /unavailable/);
});

test("login control stays disabled while another command is pending", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "copy");
  const release = native.holdReads();
  const pending = native.app.poll();
  native.app.click("save");
  assert.equal(native.app.get("launch-at-login").disabled, true);
  native.app.startup(true);
  assert(!native.calls.includes("set_launch_at_login"));
  release();
  await pending;
  await tick();
  assert.equal(native.app.get("launch-at-login").disabled, false);
});

test("failed saves preserve the previous runtime, saved assignments, and draft", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "copy");
  native.app.click("save");
  await tick();
  native.app.change("back", "paste");
  native.failSave("Activation failed; previous assignments restored");
  native.app.click("save");
  await tick();
  assert.equal(native.status.mappings.back, "copy");
  assert.equal(native.status.enabled, true);
  assert.equal(native.app.get("mapping-status").textContent, "Mappings active");
  assert.equal(native.app.get("mapping-back").value, "paste");
  assert.equal(native.app.get("draft-state").textContent, "Unsaved changes");
  assert.equal(native.app.get("save").disabled, false);
  assert.match(native.app.get("message").textContent, /Activation failed/);
  native.failSave(null);
  native.app.click("save");
  await tick();
  assert.equal(native.status.mappings.back, "paste");
});

test("saving unchanged paused mappings resumes and unavailable mappings can retry", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "copy");
  native.app.click("save");
  await tick();
  Object.assign(native.status, { paused: true, enabled: false, hook_running: false, mapping_state: "paused" });
  await native.app.poll();
  assert.equal(native.app.get("draft-state").textContent, "Saved locally");
  assert.match(native.app.get("mapping-detail").textContent, /resumes/);
  assert.equal(native.app.get("save").disabled, false);
  native.app.click("save");
  await tick();
  assert.equal(native.status.paused, false);
  assert.equal(native.status.mapping_state, "active");
  Object.assign(native.status, { enabled: false, hook_running: false, mapping_state: "unavailable", error: "Close the original utility" });
  await native.app.poll();
  assert.equal(native.app.get("save").disabled, false);
  assert.equal(native.app.get("mapping-status").textContent, "Mappings unavailable");
  assert.match(native.app.get("message").textContent, /original utility/);
  native.app.click("save");
  await tick();
  assert.equal(native.status.mapping_state, "active");
});

test("all-default saves work without a receiver and restore default behavior", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "copy");
  native.app.click("save");
  await tick();
  native.status.devices = [];
  await native.app.poll();
  native.app.click("reset");
  assert.equal(native.app.get("save").disabled, false);
  native.app.click("save");
  await tick();
  assert.equal(native.status.enabled, false);
  assert.equal(native.status.mapping_state, "default");
  assert.equal(native.app.get("mapping-status").textContent, "Default behavior");
  assert.equal(native.app.get("draft-state").textContent, "Saved locally");
});

test("thumb setup waits for receiver readiness before showing mappings active", async () => {
  const native = nativeApp({ mapping_state: "preparing", hook_running: true });
  await tick();
  assert.equal(native.app.get("mapping-status").textContent, "Preparing button 4");
  assert.match(native.app.get("mapping-detail").textContent, /receiver to confirm/);
  Object.assign(native.status, { mapping_state: "active", enabled: true });
  await native.app.poll();
  assert.equal(native.app.get("mapping-status").textContent, "Mappings active");
});

test("unknown mapping state is not presented as active and permissions point to Save", async () => {
  const native = nativeApp({ platform: "macos", permission: "required", mapping_state: "future_state" });
  await tick();
  assert.equal(native.app.get("mapping-status").textContent, "Mappings unavailable");
  assert.equal(native.app.get("permission").hidden, false);
  native.app.click("permission");
  await tick();
  assert.match(native.app.get("message").textContent, /Save and apply again/);
});


test("Settings navigation preserves mapping drafts and changes no native settings", async () => {
  const native = nativeApp();
  const { app, calls } = native;
  await tick();
  app.change("back", "copy");
  const count = calls.length;
  app.click("nav-settings");
  assert.equal(app.get("controls-page").hidden, true);
  assert.equal(app.get("settings-page").hidden, false);
  assert.equal(app.get("nav-settings")["aria-current"], "page");
  assert.equal(app.get("nav-controls")["aria-current"], "false");
  assert.equal(app.get("page-name").textContent, "Settings");
  assert.equal(app.get("settings-title").focused, true);
  assert.equal(calls.length, count);
  app.startup(true);
  await tick();
  assert.equal(native.status.launch_at_login, true);
  app.click("nav-controls");
  assert.equal(app.get("controls-page").hidden, false);
  assert.equal(app.get("settings-page").hidden, true);
  assert.equal(app.get("page-name").textContent, "QZ30 Black");
  assert.equal(app.get("controls-title").focused, true);
  assert.equal(app.get("mapping-back").value, "copy");
  assert.equal(app.get("draft-state").textContent, "Unsaved changes");
});

test("action menus group supported commands and use accurate platform labels", async () => {
  const native = nativeApp();
  await tick();
  const menu = native.app.get("mapping-middle");
  assert.deepEqual(menu.children.map((group) => group.label), ["Mouse", "Editing", "Desktop and windows", "Browser and tabs", "Media", "Zoom and capture", "Custom"]);
  const values = menu.options.map((option) => option.value);
  for (const value of ["disable", "cut", "previous_desktop", "next_desktop", "switch_apps", "minimize", "maximize", "previous_tab", "next_tab", "new_tab", "close_tab", "reopen_tab", "refresh", "mute", "play_pause", "previous_track", "next_track", "zoom_in", "zoom_out", "zoom_reset", "screenshot", "custom_shortcut", "launch_application"]) assert(values.includes(value), value);
  assert.equal(menu.options.find((option) => option.value === "disable").disabled, false);
  assert.equal(native.app.get("mapping-wheel_right").options.find((option) => option.value === "disable").hidden, true);
  assert.equal(menu.options.find((option) => option.value === "maximize").label, "Maximize / restore window");
  native.status.platform = "macos";
  native.status.thumb_available = false;
  await native.app.poll();
  assert.equal(menu.options.find((option) => option.value === "maximize").label, "Toggle full screen");
  assert.equal(native.app.get("mapping-wheel_right").options.find((option) => option.value === "disable").hidden, false);
  assert.equal(native.app.get("shortcut-key").options.find((option) => option.value === "Insert").disabled, true);
});

test("mouse markers select accessible editors and show the last detected input", async () => {
  const native = nativeApp();
  await tick();
  native.app.click("marker-back");
  assert.equal(native.app.get("marker-back")["aria-pressed"], "true");
  assert.equal(native.app.get("marker-middle")["aria-pressed"], "false");
  assert.equal(native.app.get("row-back").classList.contains("selected"), true);
  assert.equal(native.app.get("mapping-back").focused, true);
  assert.equal(native.app.get("editor-title").textContent, "Back button action");
  native.status.last_input = "Side wheel up";
  await native.app.poll();
  assert.equal(native.app.get("marker-wheel_right").classList.contains("last-input"), true);
  assert.equal(native.app.get("row-wheel_right").classList.contains("last-input"), true);
  native.status.last_input = "Thumb-rest button (4)";
  await native.app.poll();
  assert.equal(native.app.get("marker-wheel_right").classList.contains("last-input"), false);
  assert.equal(native.app.get("marker-thumb").classList.contains("last-input"), true);
  native.status.thumb_available = false;
  await native.app.poll();
  native.app.click("marker-thumb");
  assert.equal(native.app.get("marker-back")["aria-pressed"], "true");
});

test("custom shortcut drafts are deeply isolated and compared structurally", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("back", "custom_shortcut");
  assert.equal(native.app.get("shortcut-editor").hidden, false);
  native.app.edit("shortcut-key", "KeyK");
  native.app.edit("modifier-shift", true);
  native.app.click("save");
  await tick();
  assert.deepEqual(JSON.parse(JSON.stringify(native.status.mappings.back)), { custom_shortcut: { key: "KeyK", modifiers: ["control", "shift"] } });
  assert.equal(native.app.get("save").disabled, true);
  native.status.mappings.back = { custom_shortcut: { modifiers: ["shift", "control"], key: "KeyK" } };
  await native.app.poll();
  assert.equal(native.app.get("draft-state").textContent, "Saved locally");
  assert.equal(native.app.get("save").disabled, true);
  native.app.edit("shortcut-key", "F2");
  assert.equal(native.status.mappings.back.custom_shortcut.key, "KeyK");
  native.failSave("Could not activate shortcut");
  native.app.click("save");
  await tick();
  assert.equal(native.app.get("shortcut-key").value, "F2");
  assert.equal(native.app.get("draft-state").textContent, "Unsaved changes");
  assert.equal(native.status.mappings.back.custom_shortcut.key, "KeyK");
});

test("recorder blocks saving until completion and returns only a draft", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("middle", "custom_shortcut");
  native.editorResult("record_shortcut", { custom_shortcut: { key: "KeyP", modifiers: ["control", "alt"] } });
  const release = native.holdEditor();
  native.app.click("record-shortcut");
  assert.equal(native.app.get("save").disabled, true);
  assert.equal(native.app.get("shortcut-key").disabled, true);
  assert.equal(native.app.get("marker-back").disabled, true);
  assert.match(native.app.get("editor-hint").textContent, /Escape cancels/);
  native.app.click("save");
  assert(!native.calls.includes("save_mappings"));
  release();
  await tick();
  assert.equal(native.app.get("shortcut-key").value, "KeyP");
  assert.equal(native.app.get("modifier-alt").checked, true);
  assert.equal(native.status.mappings.middle, "default");
  assert.equal(native.app.get("save").disabled, false);
  native.app.click("save");
  await tick();
  assert.equal(native.status.mappings.middle.custom_shortcut.key, "KeyP");
});

test("recorder cancel or failure preserves an existing shortcut draft", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("middle", "custom_shortcut");
  native.app.edit("shortcut-key", "F3");
  native.app.click("record-shortcut");
  await tick();
  assert.equal(native.app.get("shortcut-key").value, "F3");
  assert.match(native.app.get("message").textContent, /Canceled/);
  native.fail("Recorder unavailable");
  native.app.click("record-shortcut");
  await tick();
  assert.equal(native.app.get("shortcut-key").value, "F3");
  assert.match(native.app.get("message").textContent, /Recorder unavailable/);
});

test("application picker requires a choice and preserves saved paths on cancel", async () => {
  const native = nativeApp();
  await tick();
  native.app.change("forward", "launch_application");
  assert.equal(native.app.get("application-editor").hidden, false);
  assert.equal(native.app.get("save").disabled, true);
  native.editorResult("pick_application", "C:\\Program Files\\Example\\example.exe");
  native.app.click("pick-application");
  await tick();
  assert.equal(native.app.get("application-path").value, "C:\\Program Files\\Example\\example.exe");
  assert.equal(native.app.get("save").disabled, false);
  native.app.click("save");
  await tick();
  assert.equal(native.status.mappings.forward.launch_application.path, "C:\\Program Files\\Example\\example.exe");
  assert.equal(native.app.get("save").disabled, true);
  native.editorResult("pick_application", null);
  native.app.click("pick-application");
  await tick();
  assert.equal(native.app.get("application-path").value, "C:\\Program Files\\Example\\example.exe");
  assert.equal(native.app.get("save").disabled, true);
});

test("preview custom editors keep native recorder and picker unavailable", () => {
  const app = setup();
  app.change("middle", "custom_shortcut");
  assert.equal(app.get("record-shortcut").disabled, true);
  app.change("middle", "launch_application");
  assert.equal(app.get("pick-application").disabled, true);
  assert.equal(app.get("save").disabled, true);
});
