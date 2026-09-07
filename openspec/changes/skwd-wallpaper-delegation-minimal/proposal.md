# Proposal: skwd Wallpaper Delegation (Minimal)

## Intent

HVE applies theme wallpapers via Noctalia IPC, but the visible painter is `skwd-walld` (live, socket `$XDG_RUNTIME_DIR/skwd-wall-v2/wall.sock`). The Sep 6 force-wallpaper-off policy (`noctalia.rs:153-192, 378-392, 438-461`) suppresses that path, so changes land underneath skwd-walld's layer and stay invisible. Remove the unneeded rule and best-effort forward the path to skwd-walld.

## Scope

### In Scope
- Remove force-wallpaper-off: copy `settings.json` verbatim; `post_apply` wallpaper IPC unconditional (pre-Sep-6 behavior)
- After successful `wallpaper-set`, best-effort forward same path to skwd-walld only when its socket exists; silent no-op otherwise, never fails apply
- Socket discovery spike: confirm `wall.sock` message format (undocumented protocol)

### Out of Scope
- Deep/bidirectional sync; external manual changes ignored
- Noctalia 600s automation handling; UI indicator; visual styles; monitor/HDR

## Capabilities

### New Capabilities
- `wallpaper-delegation`: best-effort forward of the applied wallpaper path to skwd-walld when present (no existing spec covers wallpaper)

### Modified Capabilities
- None

## Approach

1. Delete force-off (`force_noctalia_wallpaper_off` → remove; plain copy; drop `post_apply` skip gate).
2. Add small delegate helper in the Noctalia provider: probe socket, send path, swallow errors to warn-log.
3. Fallback preserved: daemon absent → Noctalia path alone, behavior unchanged.
4. Strict TDD (`cargo test`): passthrough copy test, IPC-attempted-when-module-off test, absent-socket no-op test, failure-swallowed test.

Alternatives: gated policy (keeps dead code for an invented bug — rejected); removal only (wallpaper still hidden under skwd-walld — insufficient); deep sync (overbuild — rejected). Removal + delegation is the smallest visible, reversible fix.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/providers/noctalia.rs` | Modified | remove force-off + gate; call delegate (sealed file — user approved minimal touch for this change only) |
| delegate helper + tests | New | socket probe + best-effort forward |
| `ui/*`, engine, other providers | Untouched | explicit non-goal |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| `wall.sock` protocol undocumented | Med | time-boxed spike; land removal-only slice first if blocked |
| blurred-layer regression (Sep-6 fear) | Low | user declares bug invented; visual verify; one-commit revert |
| automation override / z-order double-paint | Low | out of scope, documented |

## Rollback Plan

Revert the single change commit; no migrations or persisted state; behavior returns to current invisible state.

## Dependencies

- `skwd-walld` live with `wall.sock` (confirmed); exact IPC message format TBD in design spike.

## Success Criteria

- [ ] Theme apply changes visible wallpaper when skwd-walld runs
- [ ] Apply unchanged when skwd-walld absent; delegation failure never fails apply
- [ ] `cargo test` green

## Proposal question round (pre-answered assumptions)

User already decided: cooperative coexistence without stepping; solo fallback when daemon absent; no indicator; external changes ignored; Sep-6 rule removed, not gated. Correct any assumption before spec phase.
