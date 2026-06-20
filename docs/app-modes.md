# App modes and transitions

The app has three full-page modes managed by `useAppModeStore`. `App.vue` renders the active view via `v-if` on `appMode.mode`. Transitions are handled centrally in `switchTo()`:

- **Performance ↔ Edit**: non-destructive. Entering Edit stops any playing deck; decks stay loaded.
- **Any → Session**: ejects all decks before switching.
- **Session → Any**: calls `session.exit()` (stops playback, clears the loaded file) then resets the mixer.

The AppBar dropdown triggers `switchTo()` with a confirmation dialog when the transition would be destructive (active playback, loaded tracks, or an open session file).
