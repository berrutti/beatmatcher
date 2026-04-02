# Beatmatcher

A desktop app for practicing beat matching. Two independent decks, each with a pulse (similar to a metronome) and or a loop from a local audio. It has some animations for a visual representation of how accurate the beatmatching is.

**[Try it in the browser](https://berrutti.github.io/beatmatcher/)**

---

## Download

Get the latest release for macOS or Windows from the [Releases](https://github.com/berrutti/beatmatcher/releases) page.

### macOS note

The app is not code-signed, so macOS will block it on first launch. To fix this, after you drag and dropped the app to `Applications`, run once in Terminal:

```bash
xattr -cr /Applications/BeatMatcher.app
```

Then the app should open without problems.

---

## How it works

Each deck has two audio sources: a pulse and an audio loop. Mark a region on the waveform and declare how many beats it contains. BPM is inferred from the duration. Playback rate adjusts automatically to match the target BPM.

Animations:
The phase ring shows position within the current beat. The Lissajous scope plots both decks' phases against each other — a straight diagonal means they are in sync.

## Keyboard

| Key | Action |
|-----|--------|
| A / K | Deck A/B play-pause |
| S / L | Deck A/B cue |
| Q W / O P | Deck A/B nudge (pitch bend while held) |
| Tab / ' | Deck A/B pulse toggle |

---

## Development

```bash
yarn install
yarn dev
```

Built with Electron + Vue 3 + TypeScript + Vite.
