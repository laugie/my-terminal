#!/usr/bin/env bash
# #16 仓库卫生检查（在 clone 根目录运行）：example 存在、topology.json 被忽略、example 只含保留地址。
set -u; fail=0
[ -f topology.example.json ] || [ -f src-tauri/topology.example.json ] || { echo "FAIL: 缺 topology.example.json"; fail=1; }
git check-ignore -q topology.json || { echo "FAIL: topology.json 未在 .gitignore"; fail=1; }
git ls-files | grep -E '(^|/)topology\.json$' && { echo "FAIL: topology.json 被提交"; fail=1; }
for f in $(git ls-files | grep topology.example.json); do
  python3 - "$f" <<'PY' || fail=1
import json,sys,re,ipaddress
d=json.load(open(sys.argv[1])); bad=[]
ok_nets=[ipaddress.ip_network(n) for n in ("192.0.2.0/24","198.51.100.0/24","203.0.113.0/24","2001:db8::/32")]
for r in d["regions"]:
  for v in r["devices"]:
    for k in ("host","tailscaleAddress"):
      h=v.get(k)
      if h is None: continue
      try: ip=ipaddress.ip_address(h); good=any(ip in n for n in ok_nets)
      except ValueError: good=h=="example.com" or h.endswith(".example.com")
      if not good: bad.append(h)
print("FAIL: 非保留地址", bad) if bad else None; sys.exit(1 if bad else 0)
PY
done
# password 子串。斜杠规范化后，仓库相对路径必须和下面某一项完全相等，不是目录前缀。
# 其他禁用词没有白名单。
# src-tauri/src/auth/hash.rs: argon2 的 password_hash API（PasswordHash、hash_password、verify_password）就叫这个词。
# src-tauri/src/auth/store.rs: keyring 的 get_password / set_password 就叫这个词。
# src/login.ts: HTML 口令框的 type 必须是 password。
# src/startup.ts、src/startup.test.ts: 登录锁开关逻辑，只涉及是否已设口令的布尔状态。
password_allowed() {
  case "$1" in
    src-tauri/src/auth/hash.rs|src-tauri/src/auth/store.rs|src/login.ts|src/startup.ts|src/startup.test.ts) return 0 ;;
    *) return 1 ;;
  esac
}
while IFS= read -r -d '' file; do
  rel="${file#./}"
  rel="${rel//\\//}"
  if grep -q -i 'password' "$file" && ! password_allowed "$rel"; then
    echo "FAIL: password appeared in $rel"
    fail=1
  fi
done < <(find src-tauri/src src -type f -print0)
# #41 应用版本号统一为 1.0.N，与安装包文件名 V1.0.N 对应。
python3 - <<'PY' || fail=1
import json,re,sys
versions={
  "package.json": json.load(open("package.json"))["version"],
  "package-lock.json": json.load(open("package-lock.json"))["version"],
  "package-lock.json packages[\"\"]": json.load(open("package-lock.json"))["packages"][""]["version"],
  "src-tauri/tauri.conf.json": json.load(open("src-tauri/tauri.conf.json"))["version"],
  "src-tauri/Cargo.toml": re.search(r'(?m)^version = "([^"]+)"', open("src-tauri/Cargo.toml").read()).group(1),
  "src-tauri/Cargo.lock": re.search(r'name = "my-terminal"\nversion = "([^"]+)"', open("src-tauri/Cargo.lock").read()).group(1),
}
bad=[f"{k}={v}" for k,v in versions.items() if not re.fullmatch(r"1\.0\.\d+", v)]
if bad: print("FAIL: 版本号不是 1.0.N", bad)
if len(set(versions.values()))!=1: print("FAIL: 版本号不一致", versions); sys.exit(1)
sys.exit(1 if bad else 0)
PY
[ $fail = 0 ] && echo "PASS: repo checks"; exit $fail
