# beatmatcher

A desktop app for practicing beat matching. Two independent decks, each with a pulse metronome and loop playback from audio files. The phase ring and Lissajous scope give real-time visual feedback on sync.

## Stack

Electron + Vue 3 + TypeScript + Vite (via electron-vite).

## Setup

```bash
yarn install
yarn dev
```

## How it works

Each deck has two audio sources that run together: a pulse (metronome click) and an audio loop. The loop is defined by marking a region on the waveform and declaring how many beats it contains — BPM is inferred from the duration. Playback rate adjusts automatically to match the target BPM.

The phase ring shows position within the current beat. The Lissajous scope in the center plots both decks' phases against each other — a straight diagonal line means they are in sync.

## Keyboard

| Key | Action |
|-----|--------|
| A / K | Deck A/B play-pause |
| S / L | Deck A/B cue |
| Q W / O P | Deck A/B nudge (pitch bend while held) |
| Tab / ' | Deck A/B pulse toggle |
