import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_WRITE_BYTES, splitWriteChunks } from "./write-chunks.js";

const encoder = new TextEncoder();

function byteLength(text: string): number {
  return encoder.encode(text).byteLength;
}

function assertChunksRoundTrip(text: string): string[] {
  const chunks = splitWriteChunks(text);
  assert.equal(chunks.join(""), text);
  for (const chunk of chunks) {
    const size = byteLength(chunk);
    assert.ok(size > 0);
    assert.ok(size <= MAX_WRITE_BYTES);
  }
  return chunks;
}

test("splitWriteChunks returns no chunks for an empty string", () => {
  const chunks = splitWriteChunks("");
  assert.deepEqual(chunks, []);
  assert.equal(chunks.join(""), "");
});

test("splitWriteChunks keeps an exact 64 KiB payload in one chunk", () => {
  const text = "a".repeat(MAX_WRITE_BYTES);
  const chunks = assertChunksRoundTrip(text);
  assert.equal(chunks.length, 1);
  const only = chunks[0];
  assert.ok(only !== undefined);
  assert.equal(byteLength(only), MAX_WRITE_BYTES);
});

function assertCharacterStaysWhole(char: string, charBytes: number): void {
  assert.equal(byteLength(char), charBytes);
  const prefix = "a".repeat(MAX_WRITE_BYTES - (charBytes - 1));
  const text = `${prefix}${char}${"z".repeat(8)}`;
  const chunks = assertChunksRoundTrip(text);
  const hits = chunks.filter((chunk) => chunk.includes(char));
  assert.equal(hits.length, 1);
}

test("splitWriteChunks does not split a 2-byte character on the boundary", () => {
  assertCharacterStaysWhole("é", 2);
});

test("splitWriteChunks does not split a 3-byte character on the boundary", () => {
  assertCharacterStaysWhole("中", 3);
});

test("splitWriteChunks does not split a 4-byte character on the boundary", () => {
  assertCharacterStaysWhole("😀", 4);
});
