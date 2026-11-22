# IntentumDiff Patched Crates

This directory contains local crate patches used by the Rust engine and parser
workspace. Keep this set small. Every patch must have a clear reason, a removal
path, and tests that would fail if the patch stopped doing its job.

## Why Patches Exist

Patches are allowed only for release-critical reasons:

| Category | Crates | Why it exists | Removal path |
| --- | --- | --- | --- |
| Windows/AppControl build stability | `tree-sitter`, `tree-sitter-asm`, `tree-sitter-clojure`, `tree-sitter-dart`, `tree-sitter-dockerfile`, `tree-sitter-r`, `tree-sitter-squirrel`, `tree-sitter-typescript` | Some upstream crates rely on generated build scripts, missing vendored C sources, or build behavior that fails under Windows Application Control and WASI builds. The patches make native tests and Wasm parser builds deterministic. | Upstream the build-script/source changes, or switch to upstream versions that vendor the required sources and support the host/WASI split cleanly. |
| Missing or disabled grammar sources | `tree-sitter-abap`, `tree-sitter-freebasic`, `tree-sitter-pascal` | These grammars are not in a shape that can be consumed directly by the release parser workspace. | Replace with maintained upstream grammar crates or retire the language until a Rust/Wasm FullParse grammar is available. |
| Grammar correctness | `tree-sitter-cmake` (`src/scanner.c` and the prebuilt `lib/libtree_sitter_cmake.a` linked for `wasm32-wasip2`) | The external scanner's state was never initialised: `create` used `ts_malloc` and a fresh `deserialize` reset only `level`, so the first parse in a process read `token == BRACKET_ARGUMENT_OPEN` and dropped commands such as `message(${var with space})`, and a file diffed against itself reported an ADDITION (cmake-parser#4). The patch zeroes the state and resets `token` to the neutral `LINE_COMMENT` (test: `scanner_state_starts_and_restarts_neutral`). It also stops a level-mismatched close candidate (`]=]` inside `#[[`) from consuming the `]` that starts the real close, so `#]=]]` closes the bracket (test: `a_mismatched_close_inside_a_bracket_does_not_hide_the_real_close`). Rebuild the archive's `scanner.o` with wasi-sdk 28 (`clang --target=wasm32-wasip2 --sysroot=<wasi-sysroot> -std=c11 -O2 -Isrc -c src/scanner.c`) and keep `parser.o`. | Upstream the initialisation fix to tree-sitter-cmake and move to a release that has it. |
| Supply-chain and release determinism | `quote`, `serde_core`, `serde_json`, `wit-bindgen-rust-macro`, `zmij` | These are pinned local copies used to stabilize release builds and avoid dependency behavior that breaks the native wheel/Wasm build contract. | Remove once upstream versions satisfy the release constraints without local edits. |

## Rules For New Patches

- Prefer upstream crates first.
- Prefer a small target-aware `build.rs` patch over broad source changes.
- Do not patch to hide parser correctness failures.
- Do not patch to bypass security checks.
- Add a comment in the root `Cargo.toml` patch section if the reason is not obvious.
- Add or update a focused test before adding a patch.

## Review Checklist

- Can this be solved by upgrading the crate?
- Can this be upstreamed?
- Does the patch affect native tests, Wasm builds, or both?
- Is the patch still needed on Windows ARM64?
- Is the patch still needed after moving parser crates under `crates/parsers/`?

## Parser Crate Layout

Parser crates live under a dedicated parser namespace so the engine, SDK,
renderers, and parser implementations are visually separated.

Target layout:

```text
crates/
  parsers/
    python-parser/
    json-parser/
    ...
  rust-core-host/
  index-engine/
  renderers/
    terminal-renderer/
    patch-renderer/
    html-renderer/
    llm-renderer/
  sdk/
  patches/
```

Only parser crates are grouped today. Renderer/core grouping is intentionally
left as a smaller follow-up so parser path churn does not hide engine changes.
