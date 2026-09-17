---
title: 'Simplify preview search iterator'
type: 'refactor'
created: '2026-09-05'
status: 'done'
route: 'oneshot'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Preview search uses a boxed dynamic iterator solely to unify two branch-specific iterator types, obscuring simple navigation behavior.

**Approach:** Keep forward and backward wrapping search behavior unchanged while performing the shared match lookup within each branch, without dynamic dispatch or heap allocation.

</frozen-after-approval>

## Implementation Notes

- Replaced the boxed iterator with branch-local iterator searches sharing one match predicate.
- Added unit coverage for forward and backward preview-search wrapping, a single match, and no match.

## Review Triage Log

- low: Added single-match and no-match coverage; ANSI sanitization is unchanged and covered by `git::tests::only_sgr_survives_and_plain_text_is_stable`.
