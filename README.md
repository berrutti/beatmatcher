# Beatmatcher

A desktop app for practicing beat matching. Two independent decks to play local audio. It has animations for a visual representation of how accurate the beatmatching is.

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

There are two decks, and each can load a track. When loading a track, the BPM will be auto-detected. If auto-detection fails, you will be promped to introduce the track's BPM. This is important, as you wont be able to properly beatmatch if the BPM of the track is not correct.

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

Built with Tauri + Vue 3 + TypeScript + Vite.

## Acknowledgements

The BPM detection logic was inspired by this [Joe Sullivan](https://x.com/itsjoesullivan)'s blog post:  
http://joesul.li/van/beat-detection-using-web-audio/

## License

Beatmatcher - A desktop app for practicing beat matching.  
Copyright (C) 2026 Matias Berrutti

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See the [LICENSE](LICENSE) file for the full terms.
