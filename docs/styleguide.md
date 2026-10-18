# Style guide

## Comments

The whole rule: **invariants go in tests, comments explain WHY, everything else is deleted.**

- **One line. Two only if the second says something the first does not.** If the explanation wants a paragraph, it is a test, a doc in `docs/`, or nothing.
- **Never state an invariant in a comment.** Any sentence shaped like "X must always Y", "never Z", "this keeps A in step with B", or "or else W happens" is a test name plus an assertion. Write the test and delete the sentence.
- **Never explain what a test asserts.** The test name is the claim and the body is the proof. Nothing above `#[test]` or `it(...)` unless it records a fact the name cannot hold, such as a hardware address or a value transcribed from a real file.
- **Never narrate the change or the history.** No "renamed from", no "moved here because", no "previously this did X", no justification for the diff. That belongs in the PR.
- Never restate the code, the types, or the signature. If the name already says it, delete the comment.
- Never label sections (`// Types`, `// Constants`, `// ── Section ──`) and never write a file-level "what this file does" header.
- The only comment worth keeping answers a WHY the reader cannot recover from the code: a specific bug the line works around, a non-obvious ordering constraint, or a number that came from measurement. State the consequence, not the mechanism.
- Rust `///` docs are held to the same limit as `//`. A doc comment is not permission for a paragraph.
- Before writing any comment, delete it and re-read the code. If nothing is lost, leave it deleted.
- A comment is deleted or kept, never reworded. When trimming one, copy the surviving sentence out verbatim and delete the rest. Never invent a rationale to keep a comment alive: if the reason for a constant is not known, delete the comment.
- No semicolons in comments, docs or commit messages. Use a full stop.
- `node scripts/no-section-comments.mjs` (part of `yarn lint`) catches divider comments only. Passing it says nothing about whether a comment should exist.

### The check, before a change is reported done

1. List every comment line the change adds or touches.
2. Put each one in exactly one bucket:
   - **why**: a consequence the code cannot show. Keep.
   - **what**: restates the name, the type or the body. Delete.
   - **invariant**: "always", "never", "so X cannot Y", "keeps A in step with B". Make sure a test holds it, then delete.
   - **test narration**: explains the test below it. Delete.
   - **history**: says what changed or why the diff exists. Delete.
   - **stale**: no longer true after this change. Delete.
3. Any comment over two lines is deleted unless every line passes on its own.
4. Comments already in the file that this change makes false are stale too. Delete them in the same change.

### Examples

Kept, because each is a consequence the code cannot show:

```rust
// Built outside the lock, swapped inside it, so the audio thread sees one strip
// or the other and never a half-built one.
```

```rust
/// Runaway is not a creative setting: at unity the tail grows without bound and the limiter
/// is all that stands between the mixer and a held full-scale tone.
```

```ts
// Time first, because it is the one a performance reaches for.
```

Deleted:

| Comment                                                                                                      | Bucket                                  |
| ------------------------------------------------------------------------------------------------------------ | --------------------------------------- |
| `/// What this strip is feeding the master send bus, from the last processed frame.`                         | what                                    |
| `/// Adds the echo of send into mix, both interleaved stereo over the same frames.`                          | what                                    |
| `// One scale for every band so they rest at one height.`                                                    | invariant, held by `eqLayout.test.ts`   |
| `/// Switching what the knob drives disengages what it drove, so a channel is never still being coloured...` | invariant                               |
| `// Turned up but not switched on, which is what a knob left where it was looks like.`                       | test narration                          |
| `// The shelves travel 0..1 from a 0.5 detent; the bells are dB about zero.`                                 | stale, and a semicolon                  |
| `// The one mixer the live engine builds ... nothing selects another at runtime.`                            | stale once the mixer swapped at runtime |

## Naming

- **A pure function is named for what it returns**, as a bare noun: `waveformImageData`, not `buildWaveformImageData`. Imperative verbs (`build`, `create`, `make`, `compute`, `generate`) are reserved for functions that act on the world. `draw*` stays, because a canvas drawer is a command.
- **`get*` means a getter callback** that samples live state when called (`getCuePoint`, `getPlayheadPosition`). A bare noun computes from its arguments.
- **A name that will not fit says the function does two things.** A function that cannot be renamed for its result because it mutates in place should return the value and let the caller assign it. Rename as you touch code, not in one sweep.
- **Settings are named as the physical quantity**, never an abstract 1 to 10 scale: jog sensitivity is rotation speed in rpm, the fader curve is exponential, linear or logarithmic.
- **EQ, channel faders and the crossfader belong to the mixer, never to a deck.** A deck is transport plus the track. In MIDI mappings, transport and jog are addressed by deck and the channel strip by strip.
- The app name is always lowercase: beatmatcher.

## Attribution

- Credit for a borrowed technique stays and is never stripped.
- Never name a competing product as a reference or a comparison. Describe the behaviour and where a number came from instead. Comments naming the hardware actually in hand stay.

## Where logic lives

- **The frontend holds no state the engine owns.** It sends commands and reflects what the backend reports. Interaction state is the exception: held keys, drag gestures, selection, view and zoom.
- **Presentation does not belong in the engine.** The engine answers what a value does. How densely to sample it for a plot, what order to lay controls out in and which widget renders it are the frontend's.
- Every struct a Tauri command returns carries `#[serde(rename_all = "camelCase")]`. Tauri does not rename return values, so a multi-word field arrives as `undefined` without it.

## CSS and controls

- No Tailwind or utility CSS. Styles are scoped per component and named in BEM.
- Sliders use `appearance: none` with explicit pseudo-elements, never `accent-color` or `slider-vertical`.
- Do not group unrelated controls into one row to save space.
- A deck hidden from the performance grid stays mounted and is hidden with `opacity: 0; visibility: hidden`. Its row is what gives the mixer its height, and a canvas under `display: none` has no size to draw into.
- In the WebKit webview, never animate `background-position` (the whole UI blanks every frame), and append a programmatic file input to the DOM before clicking it.
- A Vue directive on a component re-renders that component on every parent render. For a component rendered many times, such as a knob, pass what the directive needs as a prop and apply the directive inside.

## Changing how it sounds

- **No regressions.** Nothing ships that changes existing behaviour by accident.
- **Keep what was tuned by ear alongside its replacement.** A sound that was tuned by ear ships as a second manifest before anything replaces it, so the two can be compared and the change reversed.
- **A figure that ends up in a coefficient is quoted from the manufacturer's own manual**, with the sentence beside it. Never a retailer listing, a forum or a search summary.
- **The filter's centre bypasses by design.** One bipolar knob with a dead zone at twelve o'clock stays through any pass that moves the filter closer to hardware.
- **A number measured or tuned by ear keeps its reason beside it** in the code, as a one-line comment naming the failure it fixed.

## Product decisions

- The pitch fader is slower at the top. Never flip it.
- Every mixer param gets an automation lane. No rationing.
- BPM shows two decimals, and the audible rate derives from the displayed value, so a shown BPM is exactly settable. The pitch control changes `targetBpm` only, never `trackBpm`, the loop region or the beat grid.
- BPM is `number | null` and the UI shows `--.-`. There is no default BPM.
- Deck mute and solo in the session view are audition-only: never logged and never in the offline render. Solo wins over mute.
- A cue sheet's cue point is the first moment a track is audible, playing with its channel gain above zero. Headphone previews do not count.
- The performer broadcast sends every live deck's own grid. There is no master deck, averaged BPM or octave folding, because decks start and stop arbitrarily and phase cannot be averaged. The payload carries `epochMs` so the consumer extrapolates phase.
- The fader curve applies to every channel at once and logs one event. All three curves hold both ends of the throw exactly, so the cue sheet's audibility test does not depend on the choice.
- Filter value and filter on/off are independent. A DJ sets the knob with the filter off, then punches it in.

## Waveform drawing

- Points aggregate by mean-in-window, not nearest neighbour, so the shape does not jump when the view resizes.
- Display height is `sqrt(amplitude)`. Amplitude is never boosted in Rust, which would clamp mastered music to 1.0 and flatten the variation the curve needs.
- Level-of-detail thresholds use physical canvas pixels, never CSS pixels.
- Colour is hue from the band balance times brightness from amplitude, each band divided by its own track RMS. Normalising a band by its peak turned everything red, and full saturation with no brightness term read as confetti.
- Timeline items paint in list order and the ruler paints last, so rows that scroll cannot paint over it.

## Tests

- **Every fuzz suite asserts that it committed something.** Two harness traps produced silently empty suites: a component that batches to `requestAnimationFrame` commits nothing until a frame is driven by hand, and happy-dom's `WheelEvent` drops the `MouseEvent` init fields, so `clientX` has to be reattached.
