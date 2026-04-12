# coco-tui

Terminal UI using ratatui with Elm architecture (TEA). Replaces React/Ink rendering.

## TS Source
- `src/components/` (33 directories -- UI components)
- `src/screens/` (UI screens)
- `src/ink/` (terminal rendering, events, hooks)
- `src/outputStyles/` (output formatting)
- `src/services/notifier.ts` (notifications)
- `src/utils/theme.ts`, `src/utils/terminal.ts`
- `src/costHook.ts`, `src/dialogLaunchers.tsx`, `src/replLauncher.tsx`

## Key Types
App, AppState (TUI model), TuiEvent, UserCommand, Overlay
