# Beatmatcher

A desktop app for practicing beat matching. Two or Four independent decks to play local audio.

![Beatmatcher screenshot](assets/screenshot.png)

## Download

Get the latest release for macOS or Windows from the [Releases](https://github.com/berrutti/beatmatcher/releases) page.

### macOS note

The app is not code-signed, so macOS will block it on first launch. To fix this, after you drag and dropped the app to `Applications`, run once in Terminal:

```bash
xattr -cr /Applications/Beatmatcher.app
```

Then the app should open without problems.

---

## Getting started

Import audio files into your collection. You can load those tracks into one of the four decks, and from there you can play, equalize and mix them.

The BPM will be auto-detected. If auto-detection fails, you can manually set te BPM in the "Edit" view.  
Having the correct BPM of each track is essential for proper mixing.

## How to play

Beatmatcher was designed to be fully operable with pointer (trackpad, mouse) and keyboard.
You can press the following buttons to control the decks.

| Deck C | Deck A | Deck B | Deck D | Function |
| ------ | ------ | ------ | ------ | -------- |
| Q / W  | E / R  | Y / U  | I / O  | Nudge    |
| A / S  | D / F  | H / J  | K / L  | CUE/Play |
| Z / X  | C / V  | N / M  | , / .  | LOOP     |

## Development

```bash
yarn install
yarn dev
```

Built with Tauri + Vue 3 + TypeScript + Vite. See [ARCHITECTURE.md](ARCHITECTURE.md) for diagrams of the audio engine, signal chain, and IPC layer.

## Acknowledgements

The BPM detection logic was inspired by this [Joe Sullivan](https://x.com/itsjoesullivan)'s blog post:  
http://joesul.li/van/beat-detection-using-web-audio/  
The waveform and meter rendering were inspired by [Mixxx](https://github.com/mixxxdj/mixxx).

## License

Beatmatcher - A desktop app for practicing beat matching.  
Copyright (C) 2026 Matias Berrutti

This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

See the [LICENSE](LICENSE) file for the full terms.
