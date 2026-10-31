# Open-source readiness plan

Status: draft for review. No code changes are part of this document.
Scope: everything needed for my-terminal to work well for most developers, not only for its original author's setup.

## 0. Where we start

- The repository is **already public** on GitHub. It has 0 forks, 0 stars, no tags, no releases, no licence file, no branch protection, and private vulnerability reporting is off.
- Anything in history, issues and PR text is already exposed. The privacy work below is about limiting further exposure and keeping new data out. It is not about a "go public" switch.
- The code is in better shape than the "personal tool" framing suggests. Hosts already come from a local file (`topology.json`, git-ignored) and there are no real addresses in code. Most of the personal parts are defaults, wording, naming, language and process.
- With no licence, nobody can legally reuse or redistribute the code. Adding a licence is the cheapest high-value step.

Priority levels used below:

| Level | Meaning |
| --- | --- |
| **P0** | Do before announcing the project or publishing any installer to strangers. |
| **P1** | Do before the first public release (proposed `v1.1.0`). |
| **P2** | Do after the first release, as the community forms. |

---

## 1. P0: privacy and history audit (done), and its clean-up

### 1.1 What was scanned

| Source | Coverage |
| --- | --- |
| Git objects | All 115 commits reachable from every branch, remote branch and GitHub PR head (`refs/pull/*/head`), 576 blobs, 184 distinct paths, including deleted files |
| Orphan branch `claude/my-terminal-29-screenshots` | 10 PNGs. 3 were inspected visually; all show `topology.example.json` data with mocked IPC, as PR #30 states |
| GitHub text | Bodies and comments of all 48 issues and PRs |
| Patterns | IPv4 outside RFC 5737 / loopback / the `100.64.0.x` test range, `*.ts.net` names, e-mail addresses, private-key headers, common token formats (GitHub, Tailscale `tskey-`, AWS, Slack, OpenAI-style), home-directory paths, `password/secret/token = value`, and a keyword list of hosting providers and personal names |

Not done yet: a run of a dedicated scanner (gitleaks or trufflehog was not installed). That is step 1.3.1 below and is the gate before announcing.

### 1.2 Findings

No credentials, private keys, tokens, real IP addresses or real Tailscale names were found in any blob, current or historical. All addresses are RFC 5737 (`192.0.2.x`), `example.com`, `example(-tailnet).ts.net`, or `100.64.0.x` in clearly synthetic fixtures (node keys are `0000…01`-style). Binary matches in icons were byte noise.

What was found:

| # | Finding | Where | Severity |
| --- | --- | --- | --- |
| F1 | One commit author line uses a personal e-mail address instead of the GitHub noreply address | `8a7f081` (#7), on `main` | Low (not a secret, but harvestable) |
| F2 | Agent author identities whose domains are machine or provider hostnames (`claude@…`, `grokbot@cloud`, `cursoragent@cursor.com`). New commits from the current agent setup will keep adding these, including the commit that adds this document | 60 commits | Low |
| F3 | Issue and PR text describes the author's real infrastructure: device names, hosting-provider names, which device relays to which, and which hosts cannot do `direct-tcpip` | Mainly #31 and #32; NAS routing also in #20, #36, #38 | Low to medium (network layout, no addresses) |
| F4 | A deleted test used a real server name as a tab title | `c364015`, fixed in `d783411` | Low |
| F5 | Stale branches: 15 task branches from merged or closed PRs, plus the orphan screenshot branch | `origin/claude-dmit/*`, `origin/grok-bot/*`, `origin/claude/*` | Hygiene |

### 1.3 Clean-up

1. **Authoritative scan.** Run `gitleaks detect --log-opts="--all"` over a fresh mirror clone that includes `refs/pull/*`. Add gitleaks to CI (`gitleaks/gitleaks-action`) and as an optional pre-commit hook. Extend `scripts/repo_checks.sh` so any IPv4 outside the documentation ranges fails CI, the same rule it already applies to `topology.example.json`.
2. **Issue and PR text (F3, F4).** Edit #31 and #32 (and the NAS mentions in #20, #36, #38) to use placeholder names. GitHub keeps edit history visible on issue bodies, so delete the earlier revisions from the "edited" menu after editing. Lock the edited conversations if needed.
3. **History rewrite (F1, F2): recommended answer is "don't rewrite".**
   - What a rewrite costs: every SHA changes, so the 48 PR pages lose their commit links. GitHub keeps `refs/pull/N/head` pointing at the old commits, and only GitHub Support can purge those and cached views. Anyone who already cloned keeps the old history.
   - What it buys: removing one e-mail address that is not a secret and some hostname-like agent identities.
   - If laugie still wants it, now is the cheapest time (0 forks): `git filter-repo --mailmap` on a mirror, force-push all branches, then open a GitHub Support ticket to drop PR refs and caches. The decision is laugie's.
   - Either way: add a `.mailmap` so `git log` and `git shortlog` show one canonical identity, turn on GitHub's "Block command line pushes that expose my email", and move all agent identities to noreply-style addresses that do not name machines or providers.
4. **Branches (F5).** Delete merged task branches and the orphan screenshot branch after new screenshots exist (section 3.2). Turn on "automatically delete head branches".
5. **Keep it clean going forward.** Templates and the bug-report form (section 3.4) tell reporters to redact hosts, users and addresses. The app's diagnostics output (section 4.6) redacts them by default.

---

## 2. P0: personal defaults become user configuration

### 2.1 Inventory

"Where" is the current code location. Each row ends in a setting with a default chosen for a developer who has never seen the app.

| Item | Where today | Becomes | Default for new users |
| --- | --- | --- | --- |
| **Remote shell "beautify"**: after connecting, the app edits `~/.bashrc` / `~/.zshrc` on the server, writes `~/.my-terminal/`, and may `git clone` zsh-syntax-highlighting from GitHub | On by default: `src-tauri/src/config.rs:149-156,636`, `src/appearance.ts:353`; script `src-tauri/src/remote_shell/beautify.sh` | Opt-in feature, per host. A clear dialog says which files change, where backups go, and what is downloaded. A global switch plus a per-host "allow" list in config | **Off.** Never touch a remote machine without explicit consent. This is the largest trust issue for strangers |
| **Home screen = hand-written `topology.json`** | Missing file shows an error that points at the repo's example (`src-tauri/src/topology/mod.rs:86-91`) | Hosts file with an in-app editor and a first-run flow (2.2). Regions become optional groups | Empty state with import and add actions, not an error |
| **Manual servers** | Kept in memory only and lost on restart (`src-tauri/src/hosts/store.rs:29`) | Saved in the same hosts file as topology devices | Persisted |
| **`~/.ssh/config`** | Used by `ssh`, but its hosts are not listed | Read-only import source: each `Host` alias becomes a device that connects by alias, so `ssh_config` stays the source of truth | Offered at first run |
| **Tailscale** | Always queried at start. CLI looked up in fixed macOS paths (`src-tauri/src/hosts/exec.rs:14-20`) and on PATH | Optional integration: auto-detected, with a toggle and an optional `tailscalePath` override | On if the CLI is found, silent if not |
| **Relay routes** (`connections`, `tailscale nc %h %p`) | Generic ProxyCommand feature, but every example and test assumes Tailscale and a NAS | Keep the feature. The editor offers presets (`nc %h %p`, `tailscale nc %h %p`, custom), and examples use neutral names | No relays |
| **Presence probing**: TCP connect to the SSH port of direct hosts that Tailscale status does not cover, every 150 s | `src/presence.ts:8`, `src-tauri/src/presence.rs` | Setting: off / on, with an interval. Documented in the privacy notes | On for hosts in the list, interval 150 s, disclosed at first run |
| **Tab limit 8** | `src/tabs.ts:52` | Setting | 8, maximum 32 |
| **UI language**: about 350 Chinese text fragments in non-test `src/*.ts`, Chinese error strings in Rust (for example `src-tauri/src/config.rs:25-29`, `topology/mod.rs:91`), Chinese-only shell script output | Throughout | Message catalogue: `en` and `zh-CN` to start. Rust returns error codes plus arguments, and the front end renders them | Follow the OS locale, fall back to English |
| **Device-type name guessing** | Mix of Chinese and English keywords | Keep, with keyword lists per locale | Unchanged |
| **Built-in font list, themes** | Hard-coded lists | Keep the lists. Allow a custom font family name and, in P2, theme import | Unchanged |
| **Config location** | `config_dir()/my-terminal` (`src-tauri/src/config.rs:20`) | Add an environment override (for portable installs and tests) | Unchanged |

### 2.2 First-run experience

1. **Welcome**, in the OS language. One screen with three detections and their results: system `ssh` (version), `~/.ssh/config` (number of hosts found), Tailscale CLI (found / logged in / not installed).
2. **Add hosts**, any combination: "Import from ~/.ssh/config" (pick hosts from a checklist), "Add from Tailscale" (only if detected), "Add a host" (name, host, user, port, key, jump host), or "Start with an example" (loads the placeholder topology, clearly marked).
3. **Privacy and behaviour**, one screen with three toggles: online status probing (on), check for updates (on), "beautify remote shells" (off, with a "what does this do?" link).
4. **Done**: land on the host list or topology view. Everything can be changed later in Settings. Existing users with a `topology.json` skip this flow.

### 2.3 Config format

- Keep `topology.json` as the stored format so existing users are unaffected. Add a `version` field and publish a JSON Schema (`$schema`) so editors can autocomplete and validate it.
- The in-app editor writes the same file. Hand-edits keep working, and the file is reloaded when it changes on disk.
- `config.json` keeps app settings, as it does today, with documented keys.

### 2.4 Naming (one-way door, decide in P0)

The name is built into user-visible and persistent places:

- product and window name `my-terminal`, npm package name, login title (`src/login.ts:14`)
- bundle identifier `com.laugie.myterminal` (`src-tauri/tauri.conf.json:5`) and `authors = ["laugie"]` (`src-tauri/Cargo.toml:5`)
- config directory name (`src-tauri/src/config.rs:20`) and keychain service (`src-tauri/src/auth/store.rs:8`)
- on remote hosts: `~/.my-terminal/`, the rc-file marker `# >>> my-terminal shell colors >>>`, and `MY_TERMINAL_*` variables (`beautify.sh`)

"my-terminal" is generic, hard to search for, and likely to clash. Recommendation: pick a distinctive name and a neutral identifier (for example a project domain or `io.github.<org>.<name>`) **before** the first signed release. After that, the identifier is tied to code-signing identity, the updater, macOS permissions and keychain entries. A rename then needs migration code: move the config directory, copy the keychain entry, and recognise the old rc marker and remote directory. Check the new name for trademarks and existing apps or packages (crates.io, npm, Homebrew, winget).

### 2.5 Personal process in the repo

- `README.md` describes a self-use tool and an "AI engineering team", and its status line is stale (v0.1 vs 1.0.7). `docs/PLAN.md` documents a private agent workflow (`agent-ready` label, Triage/Builder/Reviewer, owner-only merges). Replace both with public equivalents (sections 3.2, 3.3 and a public roadmap) and keep the agent workflow out of the repo.
- `scripts/repo_checks.sh` blocks the word `password` outside a path allowlist and pins versions to `1.0.N`. The first rule will confuse contributors: keep it only with a clear error message that explains it, or replace it with the gitleaks check. Replace the second rule with plain SemVer.
- `CHANGELOG.md` is in Chinese and puts every release under `[Unreleased]`. Move to English, one section per tagged release.
- Code comments and test messages are partly Chinese. Use English for new code and convert existing comments when a file is touched.

---

## 3. P0/P1: licence, documentation and contribution setup

### 3.1 Licence (P0)

- Recommendation: **dual `MIT OR Apache-2.0`**, the same as Tauri and most of the Rust ecosystem. Apache-2.0 adds an explicit patent grant, and MIT keeps reuse simple. Add `LICENSE-MIT`, `LICENSE-APACHE`, SPDX `license` fields in `Cargo.toml` and `package.json`, and remove `"private": true` only if publishing to npm (otherwise keep it).
- Copyright holder: laugie. All commits so far are by laugie or by agents laugie operated (Claude, Grok Bot, Cursor Agent). State this in a short `NOTICE` or in the README.
- Third-party notices: `docs/THIRD_PARTY_LICENSES.md` covers JetBrains Mono (OFL-1.1) only. Generate a full list for the shipped binaries with `cargo-about` (Rust) and `license-checker` or similar (npm), and ship it inside the app (an "About → Licences" view). Enforce allowed licences in CI with `cargo-deny`.
- App icon: record its origin (made in #27/#28 by an agent) and confirm it is original and not similar to another product's mark.
- Trademarks: say "works with Tailscale", not affiliated. Avoid naming competitors in the tagline.

### 3.2 README with screenshots (P1)

- English `README.md`, with `README.zh-CN.md` kept in sync. Structure: one-line pitch and screenshot, features, install (per platform, with signed-build notes), quick start (first-run flow), configuration reference link, security and privacy notes, building from source, contributing, licence.
- Move the long feature sections in today's README (login lock, zoom, SFTP, relay templates, and so on) to `docs/` pages, linked from the README.
- Screenshots: generate them reproducibly the way PR #30 did (example data, mocked IPC), using a script committed to the repo. Store optimised PNG/WebP under `docs/images/`. Include topology 3D and 2D, a terminal with a theme, SFTP, settings, and the first-run screen, in light and dark where relevant. Add a short GIF of click-to-connect. Add a CI check that screenshots contain only example data (they are generated from `topology.example.json`).
- Badges: CI status, latest release, licence, platforms.

### 3.3 Contribution documents (P1)

- `CONTRIBUTING.md`: prerequisites (Node 22, Rust stable, platform build dependencies including WebKitGTK on Linux), `npm ci`, `npm run tauri dev`, the test commands (`npm run test:frontend`, `cargo test`, `scripts/repo_checks.sh`, the optional sshd integration test), code style (`cargo fmt`, `clippy`, `tsc`), Conventional Commits in English, how to add a UI string to both locales, and a policy on AI-assisted contributions (allowed, the human submitter is responsible, disclose in the PR).
- `CODE_OF_CONDUCT.md`: Contributor Covenant 2.1 with a contact address that is not a personal inbox.
- `SECURITY.md`: supported versions, how to report (enable GitHub private vulnerability reporting), response targets.
- `docs/architecture.md`: front end / Rust back end split, IPC commands and capabilities, how sessions spawn the system `ssh`, the config files.
- `CODEOWNERS`, a public `ROADMAP.md` (replacing `docs/PLAN.md`), and `GOVERNANCE` notes once there is more than one maintainer.

### 3.4 Issue and PR templates (P1)

- `.github/ISSUE_TEMPLATE/bug_report.yml` (form): app version, OS and version, `ssh -V`, Tailscale version if used, connection type (direct / ProxyJump / relay), steps, expected vs actual, diagnostics output. A required checkbox: "I removed hostnames, usernames and IP addresses."
- `feature_request.yml`, and `config.yml` that disables blank issues and points questions to GitHub Discussions.
- `.github/pull_request_template.md`: summary, linked issue, tests run, screenshots for UI changes, both locales updated, CHANGELOG entry.
- Labels: keep `good first issue`, `help wanted`, `bug`, `enhancement`; add `platform:macos|windows|linux`, `area:*`, `i18n`; retire `agent-ready`.

---

## 4. P1: releases, installers and code signing

### 4.1 Release pipeline

- Keep `.github/workflows/build.yml` for PRs (add Linux and `cargo clippy`). Add `release.yml` that runs on tags `v*.*.*` and uses `tauri-apps/tauri-action` to build:
  - macOS: Apple Silicon and Intel (or one universal build), `.dmg` and `.app.tar.gz`
  - Windows: x64 (arm64 later), NSIS `.exe` (MSI optional)
  - Linux (new): `.AppImage`, `.deb`, `.rpm`
- The workflow creates a **draft** GitHub Release with release notes from `CHANGELOG.md`, `SHA256SUMS`, and build provenance attestations (`actions/attest-build-provenance`). A maintainer reviews and publishes it.
- Put signing secrets in a GitHub **environment** named `release` with a required reviewer, used only by tag builds. Protect `v*` tags with a ruleset. Pin third-party actions to commit SHAs.
- Versioning: SemVer, a single version source checked by CI (as today, without the `1.0.N` pin). The first public release is proposed as `v1.1.0`.

### 4.2 macOS signing and notarisation

- Needs the Apple Developer Program (US$99 per year) and a **Developer ID Application** certificate. Notarisation is required for that certificate type.
- CI secrets read by the Tauri build: `APPLE_CERTIFICATE` (base64 `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and for notarisation either the App Store Connect API key set `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH` (preferred) or `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`. Source: <https://v2.tauri.app/distribute/sign/macos/>.
- Test the signed build on a clean Mac: keychain access for the login lock, spawning `/usr/bin/ssh`, `pageZoom` on macOS 11, and no Gatekeeper warning after download.
- Until signing is set up, document the right-click → Open path and mark builds as unsigned.

### 4.3 Windows signing

- Standard OV certificates are now issued on hardware or cloud HSMs, so a plain `.pfx` in CI is no longer possible. Options, in order of preference:
  1. **SignPath Foundation**: free signing for qualifying open-source projects, integrated with GitHub Actions. Requires an OSI licence (3.1) and a project that meets their criteria.
  2. **Azure Artifact Signing** (formerly Trusted Signing): Tauri documents it through `bundle.windows.signCommand` with `artifact-signing-cli` and `AZURE_CLIENT_ID` / `AZURE_CLIENT_SECRET` / `AZURE_TENANT_ID`. It needs a pay-as-you-go Azure subscription and identity validation (individual validation exists). **Check price and country eligibility** on the Artifact Signing pricing and quickstart pages before choosing.
  3. An OV certificate on a cloud HSM (for example Certum's open-source certificate, Sectigo, DigiCert), also through `signCommand`.
- Tauri config keys: `bundle.windows.certificateThumbprint`, `digestAlgorithm`, `timestampUrl`, `signCommand`. Source: <https://v2.tauri.app/distribute/sign/windows/>.
- SmartScreen reputation builds per certificate over downloads. Even signed builds may warn at first, so the README should say so.

### 4.4 Auto-update

- Add `tauri-plugin-updater`. Generate the update key with `tauri signer generate`, put the public key in `plugins.updater.pubkey`, point `plugins.updater.endpoints` at `latest.json` on GitHub Releases, set `bundle.createUpdaterArtifacts: true`, and give CI `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Source: <https://v2.tauri.app/plugin/updater/>.
- The update key is separate from the code-signing certificates. If it is lost, existing installs cannot auto-update, so keep an offline backup.
- The update check can be turned off in Settings. Section 5.2 lists every network action the app takes.

### 4.5 Package managers (P2)

Homebrew cask (own tap first, then `homebrew/cask` once the project meets its notability rules), winget (`winget-pkgs` manifest, automated with `komac` or `wingetcreate` in the release workflow), Scoop bucket, and AUR or Flathub through community maintainers.

### 4.6 Supporting items for releases

- Supported platforms: macOS 11+ (WKWebView `pageZoom`), Windows 10 1809+ (built-in OpenSSH, WebView2), Linux with WebKitGTK 4.1. Linux also needs the `keyring` secret-service backend and Wayland clipboard support checked.
- "Copy diagnostics" in Settings: app and OS versions, `ssh -V`, Tailscale status summary, config validation result, and the last errors, with hosts, users and IPs redacted by default.

---

## 5. P1/P2: what else an open-source terminal app needs

### 5.1 Security (P1)

- `docs/security.md`: a short threat model. The app runs the system `ssh` without a shell; relay commands are restricted templates that run on the jump host; temporary ssh configs and their permissions; OSC 52 is write-only; the login lock deters casual access and is not encryption; what the beautify script does when enabled.
- CI: `cargo audit` or `cargo-deny` (advisories and licences), `npm audit --omit=dev`, CodeQL for JavaScript/TypeScript, Dependabot for npm, Cargo and GitHub Actions, OpenSSF Scorecard.
- Repository settings: branch ruleset on `main` (PR required, CI green, no force push), signed tags for releases, private vulnerability reporting on.

### 5.2 Privacy statement (P1)

State in the README: no telemetry and no accounts. List every network action the app takes: SSH connections the user starts, presence TCP probes (toggle), `tailscale status` (local CLI), update check (toggle), and the remote `git clone` (only when beautify is enabled).

### 5.3 Quality (P1)

- CI on Linux in addition to macOS and Windows. Keep the sshd and SFTP integration tests and run them on Linux as well.
- Config migration tests between schema versions, including the rename migration if 2.4 is done.
- Accessibility review (keyboard paths exist; add screen-reader labels and a contrast check across themes).

### 5.4 Features developers will expect (P2, roadmap candidates, not blockers)

Local shell tabs, port forwarding (`-L`, `-R`, `-D`), scrollback search, split panes, a better first-connection host-key prompt, theme import, per-host appearance, and config sync (already in the old plan). Collect these as issues and let users vote.

### 5.5 Community (P2)

GitHub Discussions (Q&A, ideas, show-and-tell), a release cadence note, and a `good first issue` backlog seeded from this plan (for example individual string-extraction tasks per view).

---

## 6. Proposed order of work

| Milestone | Contents | Exit criteria |
| --- | --- | --- |
| **M0 Hygiene** (P0) | Licence; gitleaks run and CI step; edit issue text; history decision; `.mailmap` and noreply identities; delete stale branches; enable private vulnerability reporting and branch ruleset; choose the name and identifier | Clean gitleaks report over all refs; `LICENSE-*` on `main`; ruleset active |
| **M1 Safe defaults** (P0) | Beautify off and opt-in; persisted manual hosts; empty-state and first-run flow; `~/.ssh/config` import; presence toggle; Tailscale optional | A fresh install on a machine with no Tailscale and no `topology.json` can add a host and connect without reading docs, and nothing is changed on any remote host |
| **M2 English** (P0/P1) | i18n framework, `en` + `zh-CN`, English README/CONTRIBUTING/CHANGELOG, rename migration if decided | No hard-coded UI strings outside locale files (CI check) |
| **M3 Releases** (P1) | Release workflow, macOS signing and notarisation, Windows signing, Linux packages, updater, checksums and attestations | `v1.1.0` draft release installs without OS warnings on macOS, signed on Windows, updates to a test `v1.1.1` |
| **M4 Community** (P1/P2) | Templates, CoC, SECURITY, screenshots, Discussions, package managers, roadmap | Announcement-ready README; first external contribution merged |

After this plan is approved, each milestone becomes a GitHub milestone with one issue per row, and the decisions marked for laugie (history rewrite, name and identifier, licence, Windows signing route) are recorded in their issues.
