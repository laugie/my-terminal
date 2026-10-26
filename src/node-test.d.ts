declare module "node:test" {
  export function test(name: string, fn: () => void | Promise<void>): void;
}

declare module "node:assert/strict" {
  function assert(value: unknown, message?: string): asserts value;
  namespace assert {
    function equal(actual: unknown, expected: unknown, message?: string): void;
    function deepEqual(actual: unknown, expected: unknown, message?: string): void;
    function ok(value: unknown, message?: string): void;
    function match(value: string, pattern: RegExp, message?: string): void;
    function notEqual(actual: unknown, expected: unknown, message?: string): void;
  }
  export default assert;
}
