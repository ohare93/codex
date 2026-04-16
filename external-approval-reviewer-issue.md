One related use case this could cover is approval-block observability.

If the external reviewer command receives the same approval request Codex is about to show to the user, local tooling can treat that as a stable machine-readable "Codex is blocked waiting for approval" event. That avoids scraping transcript text or internal artifacts.

For that to work cleanly, I think the response enum should include an explicit abstain value:

```json
{"decision":"defer_to_user"}
```

Semantics:
- `approved`, `approved_for_session`, `denied`, and `abort` make the approval decision.
- `defer_to_user` means the external reviewer observed the request but chose not to decide.
- Codex should then fall back to the normal user approval prompt.
- Reviewer failure behavior should be configurable. I would expect `deny` as the safe default, with an opt-in mode that falls back to the user prompt.

Example:

```toml
approvals_reviewer_failure_policy = "defer_to_user"
```

This would apply to spawn failure, timeout, nonzero exit, or malformed JSON.

That gives both workflows:
- policy engines can auto-approve or deny when confident
- notification/observability tools can just watch approval blocks and return `defer_to_user`
- local setups can decide whether reviewer failure means deny or ask the user
