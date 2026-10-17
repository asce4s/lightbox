# AGENTS.md — Operating rules for the coding agent

You are building **Lightbox**, a Picasa-style photo manager in Rust with GPUI Kit. You work autonomously: you write the code, the tests, and the CI checks, and you prove every feature works before moving on.

Read in this order at the start of **every** session:
1. `AGENTS.md` (this file) — how to work.
2. `TASKS.md` — where you are. Find the first unchecked task. Read the Progress Log, Decisions, and Blockers.
3. The `SPEC.md` sections referenced by that task. For M13 (photo viewer) tasks, `SPEC-VIEWER.md` is the spec; it supersedes SPEC.md §9.4.

---

## 1. The loop

For each task in `TASKS.md`, in order:

1. **Plan.** Re-read the referenced SPEC sections. List the acceptance tests (AC) by name.
2. **Tests first.** Write the AC tests (and any fixtures/fakes they need). Run them and confirm they fail for the right reason.
3. **Implement** the smallest code that makes them pass, following SPEC architecture and dependency rules.
4. **Verify.** Run `cargo xtask ci`. Fix until green. Do not skip steps of `xtask ci`.
5. **Record.** In `TASKS.md`: tick the box, add a Progress Log line (`YYYY-MM-DD · task id · what was done · tests added`), add any Decisions.
6. **Commit.** `git add -A && git commit -m "<type>(<scope>): <summary> [<task-id>]"` using conventional commits (`feat`, `fix`, `test`, `refactor`, `chore`, `docs`, `perf`, `ci`).
7. Move to the next task. Do not start a new milestone while the previous one has unchecked P0 tasks.

If a session is about to end mid-task, write a `WIP:` line in the Progress Log describing exactly what is left, and commit with `chore: wip [<task-id>]`.

---

## 2. Commands

| Purpose | Command |
|---|---|
| Everything CI runs | `cargo xtask ci` |
| Fast inner loop for one crate | `cargo test -p <crate> --locked` |
| One test | `cargo test -p <crate> --locked -- <test_name> --exact --nocapture` |
| Regenerate fixtures | `cargo xtask fixtures` |
| Update a golden (rare; see §4) | `cargo xtask goldens --update <name>` |
| Perf budgets | `cargo xtask perf` (release) |
| Run the app | `cargo run -p lightbox` |
| CLI | `cargo run -p lb-cli -- <args>` |
| Docs of locked deps | `cargo doc -p gpui-kit --no-deps --open` or read `~/.cargo/registry/src/*/gpui-kit-*/` |

Always pass `--locked` once `Cargo.lock` exists. Add dependencies with `cargo add` (then pin exact versions in `[workspace.dependencies]`).

---

## 3. Hard rules

1. **Never weaken a test to make it pass.** Do not delete, `#[ignore]`, loosen tolerances, or rewrite expected values of an existing test unless the SPEC changed or the test itself was wrong — and then record why in Decisions.
2. **Never modify original fixture images or user files** outside explicit file-operation features. The originals-unchanged invariant (SPEC §12.6) must hold.
3. **No network in tests.** Use the fakes in `lb-testkit`. Only `#[ignore]` nightly tests may touch the network or real models, and only when their env var is set.
4. **Respect crate boundaries** (SPEC §4.2). `lb-ui` talks only to `lb-app`. No SQL outside `lb-catalog`.
5. **Depend on `gpui-kit` only** for UI. Never add `gpui`, `gpui-pre-*`, or `gpui-component` directly. Use `gpui_kit::actions!`.
6. **No `unwrap`/`expect`** in library code (clippy-denied). Use typed errors.
7. **Deterministic tests.** No sleeps longer than 100 ms; use `wait_until`/`wait_for` with timeouts. Seed every PRNG. Use `JobQueue::inline()` and `FakeClock` where possible.
8. **Every interactive UI element** gets an id from `lb_ui::ids` and `.test_support()` (before `.track_focus`).
9. **Don't invent scope.** If something isn't in SPEC, don't build it. If SPEC is ambiguous, choose the simplest reading consistent with Picasa behavior, implement it, and record the choice in Decisions.
10. **Licenses.** Only permissive licenses in the default build (see `cargo deny` config in SPEC §13). Bundled fonts must be OFL. ML models are downloaded, not committed.

---

## 4. Goldens

Golden images (SPEC §12.4) encode the edit math. A failing golden usually means a bug in your code, not in the golden. Only regenerate when:
- the SPEC formula for that op changed, or
- you have identified and fixed a bug in the golden-generation path itself.

When you regenerate, inspect `target/test-artifacts/<test>/diff.png` (open it if you can view images; otherwise report max diff and PSNR per region), explain the change in Decisions, and regenerate **only** the affected goldens.

---

## 5. Working with GPUI / GPUI Kit (fast-moving APIs)

- SPEC snippets show intent; the locked crate is the truth. When a symbol is missing or has a different signature, look it up in the crate source under `~/.cargo/registry/src/`, the crate's examples/stories, or its tests (`gpui-kit` has a `TESTING.md` and `src/test.rs`; `gpui-component` has a `story` gallery).
- Record each API adaptation in Decisions as `GPUI: <spec said> → <actual>`.
- Keep UI logic in `lb-app::vm` view-models so it can be tested without GPUI. GPUI views should mostly render VM state and forward events.
- Headless UI tests do not render pixels. Assert on VM/service state, element visibility, bounds, focus, and events.
- Do not upgrade `gpui-kit` mid-milestone. If an upgrade is needed, make it its own task with its own commit and re-run the whole suite.

---

## 6. When you are stuck

- **Missing system dependency** (e.g. a Linux `-dev` package, ffmpeg): install it if you can; otherwise add it to `TASKS.md → Blockers` with the exact error, mark the dependent tasks blocked, and continue with the next independent task.
- **Flaky test**: fix the cause (timing, ordering, shared state). Never retry-loop a flaky test into passing.
- **Same failure three fix attempts in a row**: stop, write down hypotheses in the Progress Log, reduce the problem to a minimal failing test, then continue.
- **Spec contradiction**: follow the more specific section; record the conflict in Decisions.

---

## 7. Style

- Rust 2024 edition, `rustfmt` defaults, `clippy::pedantic` warnings allowed but not required; `-D warnings` for the default set.
- Modules small and named for the domain (`scan/walk.rs`, `edit/ops/tuning.rs`).
- Public API in library crates has `///` docs with at least one sentence; complex math cites the SPEC section.
- Tests live next to code (`mod tests`) for units, and in `tests/` for integration. Test names follow the AC names exactly.
- Logging with `tracing`; no `println!` outside the CLI and xtask.
