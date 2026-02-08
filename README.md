# gardisplay

Display/monitor manager for the gardesk desktop suite.

Manages monitor layouts, resolutions, refresh rates, scaling, rotation, and display effects (gamma, night mode, color profiles) with a draggable UI built on gartk.

## Components

- **gardisplay** - GUI application with visual monitor layout editor
- **gardisplayd** - Daemon for config persistence and effect scheduling
- **gardisplayctl** - CLI for scripting and automation
- **gardisplay-ipc** - Shared IPC message types

## Building

```bash
cargo build --release
```

## Configuration

```
~/.config/gardisplay/config.toml
```

## License

MIT
