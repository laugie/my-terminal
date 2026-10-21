// #19 前端失败测试：放到 src/startup.test.ts；被测模块 src/startup.ts（纯函数，无 DOM）。
import assert from "node:assert/strict";
import { test } from "node:test";
import { startupRoute, toggleLockPlan } from "./startup.js";

test("lock off: home regardless of auth state", () => {
  for (const state of ["needsSetup", "locked", "unlocked", "unavailable"] as const) {
    assert.equal(startupRoute({ lockEnabled: false, authState: state }), "home");
  }
});

test("lock on: follows #15 states", () => {
  assert.equal(startupRoute({ lockEnabled: true, authState: "needsSetup" }), "setup");
  assert.equal(startupRoute({ lockEnabled: true, authState: "locked" }), "login");
  assert.equal(startupRoute({ lockEnabled: true, authState: "unlocked" }), "home");
  assert.equal(startupRoute({ lockEnabled: true, authState: "unavailable" }), "unavailable");
});

test("turning on without a password runs setup now; with one takes effect next launch", () => {
  assert.deepEqual(toggleLockPlan({ from: false, to: true, hasPassword: false }), { step: "setup", effect: "nextLaunch" });
  assert.deepEqual(toggleLockPlan({ from: false, to: true, hasPassword: true }), { step: "save", effect: "nextLaunch" });
});

test("turning off verifies only when a password exists", () => {
  assert.deepEqual(toggleLockPlan({ from: true, to: false, hasPassword: true }), { step: "verify", effect: "now" });
  assert.deepEqual(toggleLockPlan({ from: true, to: false, hasPassword: false }), { step: "save", effect: "now" });
});
