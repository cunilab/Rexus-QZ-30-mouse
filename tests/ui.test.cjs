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
    this.classList = { toggle() {} };
  }
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
  const calls = [];
  const app = setup(async (command, args) => {
    calls.push(command);
    if (failure) throw new Error(failure);
    if (command === "get_status" && readGate) await readGate;
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
  const options = app.get("mapping-middle").children.map((option) => option.value);
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

test("unknown mapping state is not presented as active and permissions point to Save", async () => {
  const native = nativeApp({ platform: "macos", permission: "required", mapping_state: "future_state" });
  await tick();
  assert.equal(native.app.get("mapping-status").textContent, "Mappings unavailable");
  assert.equal(native.app.get("permission").hidden, false);
  native.app.click("permission");
  await tick();
  assert.match(native.app.get("message").textContent, /Save and apply again/);
});
