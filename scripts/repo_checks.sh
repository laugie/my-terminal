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
[ $fail = 0 ] && echo "PASS: repo checks"; exit $fail
