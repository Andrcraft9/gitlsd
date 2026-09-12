## Review findings
  1. (DONE) (Refactoring, High) Screen-specific state is flattened into App.
     src/app.rs:36 contains log, preview, help, and show fields simultaneously, allowing
     meaningless combinations such as show focus while on the log screen. Introduce cohesive
     LogState, HelpState, and ShowState structures, ideally owned by an enum representing the
     active screen.

  2. (Refactoring, High) Most application state is publicly mutable.
     The public fields at src/app.rs:36 let UI and debug code depend directly on implementation
     details and bypass invariants. Make fields private and expose intent methods plus read-only
     view accessors.

  3. (Refactoring, High) Action routing has become difficult to follow.
     App::dispatch (src/app.rs:172) handles preview focus, two show focus modes, help, global
     actions, and log navigation using nested matches and deliberate fall-through. Split this
     into screen-specific dispatch functions returning whether the action was consumed.

  4. (Refactoring, High) Scrolling is duplicated and mixed with rendering concerns.
     Vertical and horizontal offsets are stored independently for every pane, while
     max_horizontal_offset (src/app.rs:384) performs display-width calculations inside the
     application layer. A small Viewport type could own offsets and scrolling operations, while
     UI code supplies geometry-derived limits.

  5. (Refactoring, High) Layout calculations have two sources of truth.
     active_content_width (src/ui.rs:109) reconstructs the same pane layout later used by
     rendering at src/ui.rs:152 and src/ui.rs:236. Return named rectangles from one layout
     function and use them for both rendering and scroll-width calculation.

  6. (Optimization, ?) Patch navigation repeatedly reparses the entire diff.
     file_at_patch_offset (src/git.rs:287) calls patch_offset for every file whenever the diff
     moves. Parse patch headers once when loading show data and retain ordered PatchSection
     { file_index, start_line } entries. Selection synchronization then becomes a simple lookup.

  7. (?) Git-specific patch interpretation leaks into App.
     src/app.rs:11 imports patch parsing helpers from the Git adapter. load_show should return
     navigation-ready patch sections so Git output parsing remains wholly inside git.rs.

  8. (?) show_explorer_offset does not represent the real viewport.
     The value is updated in src/app.rs:441 and reported by debug output, but the actual list
     viewport belongs to Ratatui’s ListState at src/ui.rs:288. Remove the field or explicitly
     own the real list viewport; otherwise its name and debug value are misleading.

  9. (Refactoring, Mid) Preview and show searches are nearly identical.
     search_preview (src/app.rs:611) and search_show_diff (src/app.rs:731) duplicate traversal,
     wrapping, matching, and status handling. Extract a line-search helper returning a result
     such as Found, Boundary, or NoMatch; keep paginated log search separate.

  10. Git command configuration is weakly typed and execution is duplicated.
     GitHistory::new (src/git.rs:57) accepts four positional Vec<String> values, while run (src/
     git.rs:73) and run_show (src/git.rs:96) repeat process setup. Introduce a named GitCommands
     structure and one executor that attaches operation context to errors.

  11. Unsupported source capabilities appear as valid empty content.
     Default implementations in HistorySource (src/git.rs:37) return empty preview and show
     data. Now that both are core features, require implementations to provide them or return an
     explicit unsupported-operation error.

  12. Sanitized terminal text is represented as an ordinary String.
     Repeated calls to safe_text(..., false) across app, UI, and debug indicate that safety and
     styling guarantees are implicit. A small GitText/SafeText type with styled and plain views
     would centralize the trust boundary and reduce repeated sanitization.

  13. (Function, High) Don't show "Focus the show diff to search". Always search through diff.

  14. (Function, High) There is no returning back from search state. Once searched, you always see
      highlighted search results. ESC does't reset search.
