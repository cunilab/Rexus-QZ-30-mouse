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
  };
}

function nativeApp() {
  let status = {
    platform: "windows", devices: [{ transport: "USB" }], enabled: false,
    permission: "granted", hook_running: false, last_input: null, error: null,
    thumb_available: true,
    mappings: Object.fromEntries(["back", "forward", "middle", "wheel_left", "wheel_right"].map((key) => [key, "default"])),
  };
  let failure = null;
  let readGate = null;
  const calls = [];
  const app = setup(async (command, args) => {
    calls.push(command);
    if (failure) throw new Error(failure);
    if (command === "get_status" && readGate) await readGate;
    if (command === "save_mappings") status = { ...status, mappings: { ...args.mappings } };
    if (command === "set_enabled") status = { ...status, enabled: args.enabled, hook_running: args.enabled };
    return structuredClone(status);
  });
  return {
    app, calls,
    get status() { return status; },
    fail(message) { failure = message; },
    holdReads() {
      let release;
      readGate = new Promise((resolve) => { release = resolve; });
      return () => { readGate = null; release(); };
    },
  };
}

test("browser preview cannot save, enable, or fabricate a receiver", () => {
  const app = setup();
  assert.equal(app.get("preview").hidden, false);
  assert.equal(app.get("receiver-status").textContent, "Interface preview");
  app.change("back", "copy");
  assert.equal(app.get("save").disabled, true);
  assert.equal(app.get("enable").disabled, true);
  app.click("save");
  app.click("enable");
  assert.equal(app.get("enable")["aria-checked"], "false");
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

test("polling preserves drafts and assignments must be saved before enable", async () => {
  const { app, calls } = nativeApp();
  await tick();
  assert.equal(app.get("receiver-status").textContent, "Compatible receiver detected");
  assert.equal(app.get("enable").disabled, true);
  app.change("back", "copy");
  await app.poll();
  assert.equal(app.get("mapping-back").value, "copy");
  assert.equal(app.get("draft-state").textContent, "Unsaved changes");
  app.click("enable");
  assert(!calls.includes("set_enabled"));
  app.click("save");
  await tick();
  assert.equal(app.get("enable").disabled, false);
  app.click("enable");
  await tick();
  assert.equal(app.get("enable")["aria-checked"], "true");
  assert.equal(app.get("mapping-back").disabled, true);
  assert.equal(app.get("save").disabled, true);
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
  assert.equal(native.app.get("enable").disabled, false);
});
