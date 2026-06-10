//! Deterministic copy for `warpctrl tour` stops.
//!
//! No IPC is performed here. Every function returns pre-written, on-brand text
//! so neither the agent nor the interactive runner has to generate copy on the
//! fly.
use color_print::cformat;

pub(crate) fn welcome() -> String {
    cformat!(
        r#"
<bold><cyan>  ██╗    ██╗ █████╗ ██████╗ ██████╗
  ██║    ██║██╔══██╗██╔══██╗██╔══██╗
  ██║ █╗ ██║███████║██████╔╝██████╔╝
  ██║███╗██║██╔══██║██╔══██╗██╔═══╝
  ╚███╔███╔╝██║  ██║██║  ██║██║
   ╚══╝╚══╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝</cyan></bold>

<bold>  The Agentic Development Environment</bold>
  ─────────────────────────────────────────────

  Hey there! 👋  I'm your Warp tour guide, and I'm stoked
  you're here. This isn't your average terminal — Warp is
  built from scratch in Rust to be fast, beautiful, and
  genuinely helpful.

  <bold>Here's what we'll explore together:</bold>

    <green>◆</green> <bold>Core stops</bold>   Themes · Keybindings · Panes & panels ·
                   Global search · Vertical tabs

    <green>◇</green> <bold>Optional</bold>    Terminal fundamentals · Coding workflow ·
                   Agent Mode · Knowledge & navigation

  Everything stays in this split — I won't hijack your tabs.
  You can bail out at any time and I'll tidy up after myself.

  <dim>Let's go!</dim>
"#
    )
}

pub(crate) fn themes() -> String {
    cformat!(
        r#"
<bold><cyan>🎨  Themes</cyan></bold>
────────────────────────────────────────────────────────

  Warp ships with <bold>100+ themes</bold> and lets you set separate ones
  for light and dark system modes — they switch automatically.

  <bold>What you'll do here:</bold>
  • Browse the Theme Picker and live-preview a few themes
  • Notice that nothing is applied until you actually click it
  • See the light/dark toggle at the top of the picker

  <bold>Good to know:</bold>
  • Your current theme is saved before the tour opens the picker
  • I'll restore it exactly when we're done — promise
  • Warp also supports custom themes from the community

  <dim>Heads up: the picker opens in your tour split to the right.</dim>
"#
    )
}

pub(crate) fn keybindings() -> String {
    cformat!(
        r#"
<bold><cyan>⌨️  Keybindings</cyan></bold>
────────────────────────────────────────────────────────

  Warp's defaults are sensible, but nothing is sacred.
  The Keybindings panel gives you <bold>two search directions</bold>:

    → Search by <bold>action name</bold>  (e.g. "open command palette")
    → Search by <bold>shortcut</bold>    (e.g. "what does Cmd+K do?")

  <bold>What you'll do here:</bold>
  • Open the Keybindings panel in the tour split
  • Find a command you use every day
  • See its current binding (and optionally remap it)

  <bold>Good to know:</bold>
  • Changes take effect immediately — no restart required
  • You can export/import your keybindings as a YAML file
  • Warp supports all major keybinding models (Emacs, Vim, etc.)

  <dim>Try searching for something you reach for all the time.</dim>
"#
    )
}

pub(crate) fn panes() -> String {
    cformat!(
        r#"
<bold><cyan>🪟  Panes & Panels</cyan></bold>
────────────────────────────────────────────────────────

  Warp has <bold>two kinds of "split" UI</bold> — don't mix them up:

    <bold>Panes</bold>   Split a tab horizontally or vertically.
            Each pane runs an independent terminal session.
            Great for: watching logs while running a build.

    <bold>Panels</bold>  Slide-out tool surfaces along the sides.
            They float on top without consuming a pane slot.
            Great for: Project Explorer, Code Review,
                       Conversation List, Warp Drive.

  <bold>What you'll do here:</bold>
  • See several panels open in sequence in the tour split
  • Spot the anchor pane (that's us — the tour conversation)
  • Identify which is a pane and which is a panel

  <bold>Good to know:</bold>
  • Panels remember their state across sessions
  • You can resize any panel by dragging its edge
  • Panels on the left and right can be open simultaneously

  <dim>Your workspace is more than a terminal. Take it all in.</dim>
"#
    )
}

pub(crate) fn global_search() -> String {
    cformat!(
        r#"
<bold><cyan>🔍  Global Search</cyan></bold>
────────────────────────────────────────────────────────

  Command-R finds what <bold>you typed</bold> in the past.
  Global Search finds what's <bold>in your code right now</bold>.

  It's a full-text, repository-wide search that lives inside
  Warp — no editor switch, no grep-fu required.

  <bold>What you'll do here:</bold>
  • Open Global Search in the tour split
  • Type a symbol or string you know lives in your codebase
  • Click a result to jump straight to the file and line

  <bold>Good to know:</bold>
  • Results update as you type — no Enter needed
  • Supports regex for power users
  • Works across all open repositories in your workspace

  <dim>Think of it as Cmd+Shift+F that actually lives in your terminal.</dim>
"#
    )
}

pub(crate) fn vertical_tabs() -> String {
    cformat!(
        r#"
<bold><cyan>📑  Vertical Tabs</cyan></bold>
────────────────────────────────────────────────────────

  Tabs default to the top — that's fine for a few.
  Flip to <bold>Vertical Tabs</bold> and your whole session map appears:

    Tab name → its panes → each pane's active session

  It's a live tree of everything running in this window,
  and clicking any node jumps you straight to it.

  <bold>What you'll do here:</bold>
  • Open the Vertical Tabs panel in the tour split
  • Find the anchor tab and this tour's split pane in the tree
  • Notice how the hierarchy tab → pane → session is laid out

  <bold>Good to know:</bold>
  • Vertical Tabs is a panel — toggling it doesn't switch tabs
  • You can reorder tabs by dragging them in the panel
  • Session names come from the running process or can be renamed

  <dim>Perfect when you're juggling 10 tabs and need a map.</dim>
"#
    )
}

pub(crate) fn terminal() -> String {
    cformat!(
        r#"
<bold><cyan>🖥️  Terminal Fundamentals</cyan></bold>
────────────────────────────────────────────────────────

  Warp's terminal isn't just a PTY with a pretty frame.
  It has ideas about how a terminal <bold>should</bold> feel:

    <bold>Blocks</bold>        Every command gets its own container —
                  input, output, exit status, all together.
                  Copy a block, share it, re-run it, search it.

    <bold>Autosuggestions</bold> Ghost text appears as you type from your history.
                  Press <bold>→</bold> to accept, keep typing to ignore.

    <bold>Completions</bold>   Rich, context-aware completions for CLIs, paths,
                  git branches, environment variables, and more.

    <bold>Command Search</bold> Press <bold>Ctrl+R</bold> to search your entire history
                  interactively with fuzzy matching.

  <bold>What you'll do here:</bold>
  • Run a harmless command yourself (try <bold>ls</bold> or <bold>date</bold>)
  • Identify its block in the terminal
  • Try an autosuggestion or completion
  • Find that command in Command Search

  <dim>The terminal you've always wanted was just waiting to be built in Rust.</dim>
"#
    )
}

pub(crate) fn coding() -> String {
    cformat!(
        r#"
<bold><cyan>💻  Coding Workflow</cyan></bold>
────────────────────────────────────────────────────────

  Warp is a terminal <bold>and</bold> a lightweight coding environment.
  You don't have to open VS Code for the small stuff.

    <bold>File Editor</bold>    Open any file in Warp — syntax-highlighted,
                  tabbed, with line numbers. Click the file icon
                  next to any path, or use <bold>warpctrl file open</bold>.

    <bold>Code Review</bold>    Live, always-on diff panel. Shows uncommitted
                  changes for the current repo. Diff against HEAD
                  or an unstaged state — your choice.

    <bold>Project Explorer</bold>  Full directory tree. Navigate, open files,
                  see git status at a glance.

    <bold>Global Search</bold>   Repo-wide text search. Already covered!

  <bold>What you'll do here:</bold>
  • Open a file of your choice in the tour split
  • Check out Code Review for your current branch's diff

  <dim>Write, review, and commit — all without leaving Warp.</dim>
"#
    )
}

pub(crate) fn agents() -> String {
    cformat!(
        r#"
<bold><cyan>🤖  Agent Mode</cyan></bold>
────────────────────────────────────────────────────────

  Agent Mode turns Warp into a coding co-pilot that can
  <bold>actually do things</bold> — not just suggest them.

    <bold>What agents can do</bold>
    • Read and write files in your codebase
    • Run shell commands and interpret their output
    • Create branches, open PRs, run tests
    • Use tools via MCP (databases, Figma, Linear, …)

    <bold>Agent Management</bold>
    See all running agents, inspect their permissions,
    pause or cancel any run. You're always in control.

    <bold>Conversation List</bold>
    Every agent session is saved here — pick up a thread
    from yesterday, share it with a teammate, or fork it.

    <bold>Permissions</bold>
    Under Settings → Permissions, you tune exactly what
    agents can do without asking first. Fine-grained,
    per-action control.

  <bold>What you'll do here:</bold>
  • Open Agent Management and Conversation List in the tour split
  • Peek at the Permissions settings panel

  <dim>Want to try it? Ask the tour to stage a question for an agent.</dim>
"#
    )
}

pub(crate) fn knowledge() -> String {
    cformat!(
        r#"
<bold><cyan>📚  Knowledge & Navigation</cyan></bold>
────────────────────────────────────────────────────────

  Warp Drive is your team's shared brain — commands,
  docs, and context that live alongside your terminal.

    <bold>Workflows</bold>    Parameterised command snippets. Share a
                  multi-step process as a single slash command.
                  e.g. <bold>/deploy staging</bold> → the whole deploy pipeline.

    <bold>Notebooks</bold>   Living documents with runnable command blocks.
                  Write a runbook once; run it anywhere.

    <bold>Rules</bold>       Instructions that agents always follow in a
                  given repo or globally. Your team's coding style,
                  project conventions, preferred tools — all wired in.

    <bold>MCP</bold>         Model Context Protocol. Agents can connect to
                  external tools — Linear, Figma, databases, APIs —
                  and use them as naturally as shell commands.

    <bold>Command Palette</bold>  <bold>Cmd+P</bold> (or your binding) to jump anywhere —
                  Warp Drive items, settings, actions — instantly.

  <bold>What you'll do here:</bold>
  • Browse Warp Drive and peek at the Command Palette
  • See if any MCP servers are configured

  <dim>The best teams share context. Warp Drive is how you do it.</dim>
"#
    )
}

pub(crate) fn cleanup() -> String {
    cformat!(
        r#"
<bold><cyan>✅  Tour Complete!</cyan></bold>
────────────────────────────────────────────────────────

  You made it! Here's what I'm about to clean up:

    <bold>Theme</bold>      Restoring your original light/dark theme state
    <bold>Tour pane</bold>   Closing the right-hand split I created
    <bold>Extra tabs</bold>  Closing any temporary tabs I opened (if any)

  <bold>What to expect:</bold>
  • Warp may show its normal close confirmation for each pane/tab
    — that's expected; just confirm and it'll close cleanly.
  • If a close is cancelled (e.g. there's a running process),
    I'll report what's still open so you can handle it.

  <bold>Where to go from here:</bold>
  • <bold>warpctrl help</bold>       — explore the full CLI surface
  • <bold>Settings → Scripting</bold> — tweak Warp Control permissions
  • <bold>warpctrl tour run</bold>    — take the tour again any time
  • <bold>warp.dev/docs</bold>        — full documentation

  Thanks for touring with me. Go build something great. 🚀
"#
    )
}
