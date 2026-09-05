- source_spec: `/home/andr/code/git-tools/gitlsd/_bmad-output/implementation-artifacts/spec-gitlsd-mvp.md`
  summary: Determine whether pagination should snapshot repository refs when history changes during a running session.
  evidence: The review could not establish the expected behavior if refs move between offset-based page loads; reproducing a concurrent ref update and deciding whether sessions promise snapshot consistency would settle it.
