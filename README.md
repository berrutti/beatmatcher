# Beatmatcher

A desktop app for practicing beat matching. Two independent decks that loop local audio. It has some animations for a visual representation of how accurate the beatmatching is.

**[Try it in the browser](https://berrutti.github.io/beatmatcher/)**

---

## Download

Get the latest release for macOS or Windows from the [Releases](https://github.com/berrutti/beatmatcher/releases) page.

### macOS note

The app is not code-signed, so macOS will block it on first launch. To fix this, after you drag and dropped the app to `Applications`, run once in Terminal:

```bash
xattr -cr /Applications/Beatmatcher.app
```

Then the app should open without problems.

---

## How it works

There are two decks, and each can load a track. When loading a track, the BPM will be auto-detected. If auto-detection fails, you will be promped to introduce the track's BPM. This is important and the program wont work unless the proper BPM of the track is determined.
You can also mark a region on the waveform and declare how many beats it contains. In that case, BPM is inferred from the duration. Playback rate adjusts automatically to match the target BPM.

Animations:
The phase ring shows position within the current beat. The Lissajous scope plots both decks' phases against each other. A straight diagonal means they are in sync.

## Keyboard

| Key       | Action                                 |
| --------- | -------------------------------------- |
| A / K     | Deck A/B play-pause                    |
| S / L     | Deck A/B cue                           |
| Q W / O P | Deck A/B nudge (pitch bend while held) |

---

## Development

```bash
yarn install
yarn dev
```

Built with Electron + Vue 3 + TypeScript + Vite.

## Acknowledgements

The BPM detection logic was inspired by this [Joe Sullivan](https://x.com/itsjoesullivan)'s blog post:  
http://joesul.li/van/beat-detection-using-web-audio/
