# Spar

**Your agent sets the challenge. You write the code.**

A local-first coding practice TUI in Rust. Short reps, deliberate repetition, a built-in editor, and your existing Codex CLI login. Python and TypeScript are the initial practice languages.

Spar is a working name. Package and trademark availability have not been checked; Cargo publishing is disabled.

## Try it

```sh
cargo run -- demo
# or preview Python
cargo run -- demo --language python
```

The preview opens a bundled rep in a temporary profile, without authentication or a model call. It does not claim that the exercise has been execution-validated. Editing works immediately; running checks requires the prepared container runtime. Preview history is discarded on exit.

For regular practice:

```sh
cargo build --release --locked
./target/release/spar init
./target/release/spar setup   # starts Apple container and prepares both runners
./target/release/spar doctor
./target/release/spar
```

On Apple silicon with macOS 26 or later, install the runtime with `brew install container`. `spar setup` starts its service, installs the recommended Linux kernel, downloads the Python/Node images, and checks both runners. Install Codex CLI and run `codex login` with a ChatGPT account for generation. `spar doctor` checks the CLI capabilities, authentication mode, and images. There is no API-key billing mode in this version.

Spar itself is a single executable: the UI, SQLite library, prompts, schemas, runner templates, and example reps are compiled into it. Rust is only required to build from source. **Codex CLI and Apple container remain external runtime requirements** for generation and execution respectively on macOS. Host Python, Node, npm, and exercise-specific installs are unnecessary.

## Your practice

`spar init` asks for language, experience, interests, and session duration. Profiles keep independent histories. You can also supply every field as a flag:

```sh
spar init --name rusty-ts --language typescript \
  --experience 'Experienced, but rusty' \
  --focus Backend,'Data processing',Testing --minutes 5
spar profiles
spar profiles rusty-ts
spar history
spar revisit                 # fresh attempt at the last completed rep, no AI call
# spar revisit ATTEMPT_ID    # IDs are shown by spar history
```

On launch, Spar resumes an unfinished attempt, takes a validated cached rep, or prepares one. Build reps ask for an implementation. Debug reps require a fix and a regression test that catches the original bug. Test reps keep the implementation read-only and evaluate your tests against the reference and every deliberately buggy implementation.

The practice workspace keeps the brief, implementation, tests, and results visible together. On wide terminals the brief sits beside the editors; below 100 columns it moves above them. The active pane has a highlighted border and the Tests editor expands when focused. Python and TypeScript syntax is colored in both editors, including multiline strings and comments. Run results and debug output appear below the editors without moving your focus or hiding your code; failures appear first with their related requirement. On wide terminals, the Output pane sits beside the check summary. Alt+O focuses and expands it; on smaller terminals it switches the feedback area to Output. Output remains visible after editing and is marked as belonging to the previous run until you run again. Scroll indicators show how much of the brief or feedback is visible. Minimum size: 68 × 22.

Examples live in the brief as **Input / Output / Explanation**, separate from the editable files. New attempts and revisits start with an empty Tests editor. Build reps allow optional learner tests; Debug reps require a regression test; Test reps require tests that catch the supplied bugs. When resuming an older attempt, an unchanged copy of the original example assertions is moved out of the Tests editor; edited tests are preserved. Older cached packages show their original example assertions in the brief until replaced by a newly generated rep.

All reps use a small synchronous `solve(value)` interface with JSON-compatible arguments and results. Write executable assertions in the Tests panel: Python receives `solve`; TypeScript receives `solve` and `assert` from `node:assert/strict`. No framework or import boilerplate is needed. TypeScript runs with Node 24 native type stripping; syntax requiring TypeScript compilation, such as enums, is outside this adapter's scope.

| Key | Action |
| --- | --- |
| Shift+Tab / Alt+1…4 | Focus Brief, Code, Tests, or Results without hiding other panes |
| Click / drag / mouse wheel | Place the cursor / select text / scroll the pane under the pointer |
| Alt+O | Focus and expand debug output; press again to return to results |
| Alt+↑/↓ or Alt+PageUp/PageDown | Scroll the brief while keeping your editor cursor |
| Tab / Enter | Indent (Python: 4 spaces; TypeScript: 2) / newline with automatic indentation |
| Shift+arrows / Ctrl+A | Select text / select all in the current editor |
| Ctrl+Z / Ctrl+Y | Undo / redo, independently in each editor |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste through the system clipboard |
| Ctrl+F / Ctrl+G | Find text in the current editor / next match (wraps around) |
| F1 | Focus the brief and scroll to its beginning |
| F2 | Reveal the next hint |
| F3 | Open the current editable file in `$VISUAL`, `$EDITOR`, or `vi` |
| F4 | Confirm and reveal reference solution and tests |
| F5 | Run isolated checks |
| F6 | Next rep after completion; retry if preparation failed |
| F7 / Alt+5 | Show local practice history in the results pane |
| F8 | Replace: too large, unclear, or broken |
| F9 | Prepare one more validated rep |
| F10 | Open the complete keyboard guide |
| Esc | Close reference/history details and save, or cancel a background operation |
| Ctrl+S / Ctrl+Q | Save / save and quit |

Both editors are visible and keep independent cursors and undo histories. Long lines wrap visually without changing the source. Test reps focus the Tests editor automatically and show a read-only implementation. Hints appear at the top of the brief without moving editor focus. The editor supports undo/redo, mouse and keyboard selection, line numbers, bracketed paste, literal search, and a highlighted current line. Each file shows its save state and cursor position. Edits autosave every five seconds; an unsaved indicator stays visible until the save succeeds. Files are bounded to 32 KB, including typing and paste. Ctrl+Q saves and quits; Ctrl+C copies the selection. System clipboard support requires a desktop clipboard; terminal paste remains available in remote sessions. Syntax definitions are embedded, so highlighting works offline with no language server or additional installation. Tests use a frozen snapshot while running. Active practice time pauses during background work and after 60 seconds without input; time in an external editor is excluded. It is an estimate, not a countdown.

The runner captures `print`, `console.log`, `console.error`, stderr, and exception details per check. Normal captured output shares an 8,192-character budget per execution; excess is marked as truncated. Terminal escape sequences are removed before rendering. Output from extra reference/mutant grading runs is not attached to the learner’s result. Syntax/import failures are shown with the runtime diagnostic instead of a generic failure message. Logs remain in memory for the current session and are not sent to the AI provider.

A passing rep records **Independent / Used hints / Viewed solution** and a short explanation. Rejected exercises record the reason locally and carry no skill-failure signal. Requirements remain fixed for the duration of an attempt.

## Offline queue and allowance

```sh
spar prepare --count 3            # uses Codex allowance; queue holds at most five
spar prepare --offline --count 3  # validate bundled Build, Debug, Test examples
```

Preparation makes at most two generation attempts per requested rep. Authentication, allowance, unsupported-model, and rejected-schema errors stop immediately with a specific message. Generation requests use a complete output schema, while the local package reader separately supports older cached formats. The offline examples are three five-minute variations of the same cache scenario, in each language; they demonstrate the loop rather than a broad curriculum. Already cached reps and test runs do not call a model. A missing provider or exhausted allowance leaves the existing queue and drafts intact. An empty queue with unavailable generation shows an actionable error and retry control.

The scheduler chooses the skill and mode before requesting generation. It revisits assisted skills, reduces recent repetition, and schedules test writing after a gap. The model receives proficiency, interests, duration, the selected objective, and recent exercise-family names. It does not invent a numerical mastery score.

## Data and execution

Data lives in the platform's local application-data directory (`~/Library/Application Support/dev.spar.spar` on macOS). `spar doctor` prints the exact location. Override it with `--data-dir PATH` or `SPAR_HOME`. SQLite stores profiles, versioned packages, generation provenance, attempts, assistance and outcomes. External-editor files are placed under `workspaces/<attempt-id>`; references and unrevealed hints stay in SQLite.

Generation uses `codex login status` and schema-constrained `codex exec`, with saved CLI authentication, `--ignore-user-config`, a read-only sandbox, an ephemeral run, and an app-owned temporary working directory. Spar never reads or copies OAuth tokens. Its child environment allowlist excludes API-key and provider-endpoint overrides. Raw Codex output is kept out of the UI. Generation is remote processing and consumes normal Codex allowance. No learner code or unrelated repository content is intentionally sent; optional AI review is not implemented.

Format v2 packages include structured examples whose expected results are checked against the reference; existing v1 packages remain readable. Every generated package is bounded and checked for required fields, supported language, requirement coverage and source size. The model cannot choose file paths, install commands or execution commands. Admission runs the reference, starter and deliberately incorrect implementations; negative evidence must be an assertion failure, not a syntax or runtime error.

Execution uses app-owned templates in **Apple container by default on macOS**, with each invocation in its own lightweight Linux VM and only a disposable exercise snapshot mounted read-only. The exercise has no network interface beyond loopback, no host home or credentials, an unprivileged user, all Linux capabilities dropped, and no privilege escalation. A BusyBox launcher sets `no_new_privs` before loading exercise code. The root filesystem is read-only; `/tmp` is a 16 MB temporary filesystem. Each VM gets 512 MB memory and one CPU, with a 32-process per-user limit and a 64-file-descriptor limit. Checks have a 30-second wall-time limit including VM startup. Cancellation stops the child process group and explicitly removes the container.

`SPAR_RUNNER=apple` selects Apple container explicitly. `SPAR_RUNNER=docker` selects the retained Docker adapter, which is also the default on Linux and uses 128 MB container memory and a 32-process cgroup limit. Selection never silently falls back to another engine after an error. `spar doctor` identifies the selected engine and checks the CLI capabilities and cached images. Running a rep checks for its cached image first. Apple container can fetch missing internal VM assets; setup warms these assets so ordinary practice uses the cache. Model-generated code has no network access regardless of any host-side image downloads.

These checks catch common broken exercises; a generated reference passing generated tests does not prove correctness. Isolation depends on the selected runtime and host configuration. This is a local practice tool, not a secure grading service against an adversarial learner. Owner-readable solutions are intentionally kept out of normal editing, not encrypted.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
# Prepare Apple container on macOS (Docker on Linux):
cargo run -- setup
cargo test --locked -- --ignored
```

Normal tests cover package bounds, scheduling, profile separation, resume, assistance, defect handling, process cancellation, isolation arguments, admission policy and TUI rendering. The ignored container tests run all six bundled packages, including reference, starter, regression, and mutation checks, and verify actual OS isolation and cancellation. Regular CI runs those tests with Docker on Linux. The manual Apple container workflow targets a self-hosted Apple silicon Mac with virtualization support; standard hosted macOS runners do not provide the required environment. A distribution workflow builds native macOS ARM64 and static Linux x86-64 executables as downloadable workflow artifacts; it does not publish releases.

The editor uses EdTUI for rendering and cursor navigation, with cached Syntect highlighting and bundled Python/TypeScript grammars from two-face. Editing is modeless; no Vim commands are required.

The modules are `model` (package contract), `scheduler`, `store`, `provider`, `runner`, `service`, and `tui`. `Provider` and `Runner` are the extension seams. Language-specific file conventions, images and harnesses stay in the language adapter. No repository scanning, autocomplete, accounts, sync, telemetry, leaderboards, or marketplace.

Implementation references: [Ratatui](https://docs.rs/ratatui/0.30.2/ratatui/), [EdTUI](https://docs.rs/edtui/0.11.7/edtui/), [Syntect](https://docs.rs/syntect/5.3.0/syntect/), [two-face](https://docs.rs/two-face/0.5.2/two_face/), [Codex non-interactive execution](https://developers.openai.com/codex/noninteractive), [CLI flags](https://developers.openai.com/codex/cli/reference), [Codex authentication](https://developers.openai.com/codex/auth), [Apple container](https://github.com/apple/container).
