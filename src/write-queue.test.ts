import assert from "node:assert/strict";
import { test } from "node:test";
import { WriteQueue } from "./write-queue.js";

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

test("WriteQueue writes chunks in enqueue order when writes finish at different speeds", async () => {
  const queue = new WriteQueue();
  const seen: string[] = [];
  let active = 0;
  let maxActive = 0;
  const delays = [40, 5, 20];
  const jobs = ["a", "b", "c"].map((chunk, index) =>
    queue.enqueue(
      [chunk],
      async (value) => {
        active += 1;
        maxActive = Math.max(maxActive, active);
        await delay(delays[index] ?? 0);
        seen.push(value);
        active -= 1;
      },
      () => {},
    ),
  );
  await Promise.all(jobs);
  assert.deepEqual(seen, ["a", "b", "c"]);
  assert.equal(maxActive, 1);
});

test("WriteQueue drops the rest of a failed write and keeps the queue moving", async () => {
  const queue = new WriteQueue();
  const seen: string[] = [];
  const errors: unknown[] = [];
  const failed = queue.enqueue(
    ["a", "b", "c"],
    async (value) => {
      if (value === "b") {
        throw new Error("fail");
      }
      seen.push(value);
    },
    (error) => {
      errors.push(error);
    },
  );
  const next = queue.enqueue(
    ["d"],
    async (value) => {
      seen.push(value);
    },
    () => {},
  );
  await Promise.all([failed, next]);
  assert.deepEqual(seen, ["a", "d"]);
  assert.equal(errors.length, 1);
});

test("WriteQueue invalidate discards queued chunks from the old generation", async () => {
  const queue = new WriteQueue();
  const seen: string[] = [];
  let release = (): void => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let markStarted = (): void => {};
  const started = new Promise<void>((resolve) => {
    markStarted = resolve;
  });
  const first = queue.enqueue(
    ["a", "b"],
    async (value) => {
      if (value === "a") {
        markStarted();
        await gate;
      }
      seen.push(value);
    },
    () => {},
  );
  await started;
  queue.invalidate();
  const later = queue.enqueue(
    ["c"],
    async (value) => {
      seen.push(value);
    },
    () => {},
  );
  release();
  await Promise.all([first, later]);
  assert.deepEqual(seen, ["a", "c"]);
});
