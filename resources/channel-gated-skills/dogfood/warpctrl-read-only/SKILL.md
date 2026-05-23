---
name: warpctrl-read-only
description: Use implemented read-only warpctrl metadata commands safely to inspect running local Warp app instances, choose explicit targets, and read appearance/settings metadata without reading underlying terminal or user data.
---

# warpctrl Read-Only Metadata Recipes

Use this skill when a task asks you to inspect or reason about a running local Warp app through the provisional `warpctrl` CLI without changing app state.

## Ground rules

- Use only commands whose selected app action metadata reports `implementation_status: implemented`.
- Do not call mutating commands from this skill. `warpctrl tab create` exists as a first-slice app-state mutation smoke test, but it is not read-only.
- Do not treat parser support as proof that the selected app build has a live handler. If a command returns `unsupported_action`, report that the handler is not implemented in the running app and stop that recipe.
- Keep metadata reads separate from underlying-data reads. Metadata read permission does not authorize reading terminal output, input buffers, command history, file contents, Drive object contents, or AI conversation content.
- Prefer `--output-format json` for Agent workflows so errors and returned IDs can be parsed reliably.

## Select a target safely

1. Discover compatible instances:
   ```bash
   warpctrl --output-format json instance list
   ```
2. Choose an `instance_id` from the result.
3. Pass `--instance <instance_id>` on every follow-up command in scripts or Agent workflows.
4. Use implicit active-instance targeting only for short interactive checks when exactly one compatible instance is present.
5. Avoid `--pid` for durable automation. It is a convenience filter for local debugging, not the canonical selector.

Handle these structured errors explicitly: `no_instance`, `ambiguous_instance`, `local_control_disabled`, `unauthorized_local_client`, `insufficient_permissions`, `execution_context_not_allowed`, `unsupported_action`, and `stale_target`.

## Metadata read recipes

Metadata reads inspect app structure or local configuration without exposing terminal contents.

### Health and protocol metadata

```bash
warpctrl --output-format json app ping --instance <instance_id>
warpctrl --output-format json app version --instance <instance_id>
```

### Active target chain and app summary

```bash
warpctrl --output-format json app active --instance <instance_id>
warpctrl --output-format json app inspect --instance <instance_id>
```

### Action catalog

Use action metadata to confirm `implementation_status`, `permission_category`, and `requires_authenticated_user` before relying on a command.

```bash
warpctrl --output-format json action list --instance <instance_id>
warpctrl --output-format json action get --instance <instance_id> tab.list
```

### Layout metadata

```bash
warpctrl --output-format json window list --instance <instance_id>
warpctrl --output-format json tab list --instance <instance_id>
warpctrl --output-format json pane list --instance <instance_id>
warpctrl --output-format json session list --instance <instance_id>
```

### Appearance and allowlisted settings metadata

```bash
warpctrl --output-format json theme list --instance <instance_id>
warpctrl --output-format json appearance get --instance <instance_id>
warpctrl --output-format json setting list --instance <instance_id>
warpctrl --output-format json setting get --instance <instance_id> appearance.themes.theme
warpctrl --output-format json setting get --instance <instance_id> appearance.text.font_size
```

`setting get` intentionally exposes only allowlisted local configuration metadata. If it returns `not_allowlisted`, do not try to read the same setting through a broader data command.

## Commands this skill must not use

Do not use or document these as implemented read-only metadata recipes:

- terminal or user-content reads such as `block`, `input`, `history`, file contents, Drive object contents, AI conversation contents, pane output, scrollback, or transcripts;
- window, tab, or pane mutations such as create, focus, close, split, activate, move, rename, maximize, resize, or navigate;
- theme, appearance, or setting writes such as set, toggle, font-size, or zoom changes;
- app surface toggles or opens such as settings, command palette, Warp Drive, resource center, AI assistant, code review, or vertical tabs;
- terminal input mutations such as insert, replace, clear, mode switching, command execution, accepted-command submission, workflow execution, or agent prompt submission.

If the user explicitly asks for a mutation or underlying-data read, leave this skill and verify the command's implemented action metadata and permission category before proceeding.
