#!/usr/bin/env bash
# #15 仓库检查（clone 根目录）：命令都被登录门禁保护；无默认口令；依赖到位。
set -u; fail=0
grep -q '^argon2' src-tauri/Cargo.toml || { echo "FAIL: 缺 argon2 依赖"; fail=1; }
grep -q '^keyring' src-tauri/Cargo.toml || { echo "FAIL: 缺 keyring 依赖"; fail=1; }
for f in src-tauri/src/hosts/mod.rs src-tauri/src/session/mod.rs; do
  grep -q 'require_unlocked' "$f" || { echo "FAIL: $f 的命令未调用 require_unlocked"; fail=1; }
done
if git grep -nEi '(default_?pass(word)?|password\s*[:=]\s*"[^"]+"|"(admin|123456|changeme|letmein)")' -- ':!*.lock' ':!package-lock.json' ; then
  echo "FAIL: 疑似默认口令"; fail=1; fi
[ $fail = 0 ] && echo "PASS: repo checks"; exit $fail
