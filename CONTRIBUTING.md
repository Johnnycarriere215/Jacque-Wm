# Contributing to JacqueWM

Thanks for taking a look! This project lives by a few hard rules —
they exist so the safety promise never accidentally regresses.

---

## 1. Safety rules — non-negotiable

**Any** PR that violates these will be closed without discussion:

* Do **not** replace `explorer.exe` / disable it / inject into it.
* Do **not** install drivers, services, scheduled tasks, or anything
  that survives parent-process exit.
* Do **not** write outside `HKEY_CURRENT_USER` (no `HKLM`, no
  `HKCR`).
* Do **not** require admin elevation for normal operation.
* Do **not** patch or hook undocumented kernel APIs.
* Do **not** add a DLL-injection path into arbitrary processes.

If a feature somehow *requires* one of those, the feature does not
ship. Find an alternative.

---

## 2. Code style

* Rust stable, 1.75+.
* `cargo fmt --all` before committing.
* `cargo clippy --all-targets --all-features -- -D warnings` must
  pass.
* `#![deny(unsafe_op_in_unsafe_fn)]` at the lib root. Every `unsafe`
  block has a justifying comment.
* Modules follow the existing convention: `mod.rs` re-exports
  + sub-files for individual concepts.
* Public items get meaningful doc comments.

---

## 3. Architecture rules

* Prompts are append-only. We never refactor Prompt 1 modules to
  accommodate a later feature; we extend.
* Every subsystem is independent. A panic in one must not crash
  another. Wrap new subsystems in `core::isolation::safe_init`.
* The platform layer is the only place Win32 / COM is allowed.
* Hot-reloadable settings changes must NEVER silently succeed on a
  sub-section marked unsafe (e.g. hotkey remap).

---

## 4. Pull request workflow

1. Branch off `main`.
2. Keep PRs focused — one feature or one fix.
3. Add or update tests.
4. Update `CHANGELOG.md` under the *Unreleased* section.
5. Run `cargo fmt && cargo build --release && cargo test`.
6. Verify `cargo clippy` passes.
7. Open the PR with:
   * Problem statement
   * Solution sketch
   * Test plan
   * Screenshots / logs if user-facing

We squash-merge and the timestamp becomes the canonical release time.

---

## 5. Module conventions

* Every `core/*` module has a top-of-file //!  comment explaining its
  contract + invariants.
* Every `platform/windows/*` module has a similar comment but for
  the *concrete* behaviour.
* New public functions prefer free functions over extending big
  structures. Trait objects are the contract — extend the trait,
  not the concrete impl.

---

## 6. Commit messages

`<scope>: <change>` lowercase, imperative voice.

* `panel: add divider drag tile`
* `tiling: clamp ratio to 0.05..=0.95`
* `installer: never require admin`

---

## 7. Performance budget

* Idle CPU ≈ 0 % — your PR must not introduce busy loops.
* Idle RAM (additional) ≤ 30 MB.
* Total RAM ≤ 100 MB.
* Hotkey reaction ≤ 50 ms.

If your change pushes any of these beyond budget, the PR will be
flagged and the burden is on the contributor to mitigate (not us).

---

## 8. License

By contributing you agree that your contributions are licensed under
the MIT license.
