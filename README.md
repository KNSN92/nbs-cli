
# NBS Cli

A command-line interface for nbs files, that allows you to see the information of the nbs file, and playback the nbs file.

The purpose of this project is to provide some features around nbs files,
But it using also to test the [nbs-rust crate](https://github.com/KNSN92/nbs-rust), which is a Rust implementation of the nbs file format parser and player.

## Features & Roadmap

- [x] Show information of nbs file
- [x] Playback nbs file with custom instrument
  - [ ] Adaptive custom instrument loading
- [ ] Midi to nbs conversion
- [ ] To audio files(wav/mp3) conversion
- [ ] Bundle a nbs file and custom instrument into a single zip file, and play it without extracting

## Usage

### Show information

```bash
nbs info <file.nbs>
```

### Playback

#### Basic playback

```bash
nbs play <file.nbs>
```

#### With Custom Instrument

```bash
nbs play <file.nbs> --custom-instrument <sounds directory>
```

#### Adaptive Custom Instrument Locating
locate custom instruments recursively in the folder that includes the nbs file

```bash
nbs play <file.nbs> --adaptive
```


#### Endless Looping

```bash
nbs play <file.nbs> --loop
```
