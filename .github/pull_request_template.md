## Summary

-

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `git diff --check`

## Keel Review Notes

- Keel task or run id:
- Risk warnings:
- User-facing behavior changed:

## Safety Checklist

- [ ] Does not auto merge.
- [ ] Does not auto push outside explicit push flows.
- [ ] Does not commit local-only `.keel/`, `AGENTS.md`, `docs/`, `output/`, or `target/`.
