# open.mp Linux Launcher

open.mp launcher for Linux written in Rust with Ratatui.

Browse the open.mp server list in the terminal and join servers through Wine. The SA-MP and
open.mp client files are downloaded from open.mp automatically.

![omp-tui](assets/preview.png)

This was built to serve my personal need for having a browser on Arch. Beta tested by 2-3 people
who helped me battle harden it. If you find any issues, feel free to open an issue or a PR.

## Requirements

- Wine and winetricks
- GTA San Andreas 1.0 US installed in a Wine prefix

## Install

Download the binary from the [releases](https://github.com/DignitySAMP/open.mp-linux-launcher/releases) and run
`omp-tui --install-desktop`, or build it:

```
rustup target add i686-pc-windows-msvc   # for the embedded Windows helper, needs lld
cargo build --release
./target/release/omp-tui --install-desktop
```

## Usage

Run `omp-tui`. The first boot finds Wine and the game, downloads the client files and opens the
server list. `Enter` joins, `F` favorites, `/` searches, `,` opens settings, `?` lists all keys.

```
omp-tui omp://1.2.3.4:7777
omp-tui -h 1.2.3.4 -p 7777 -n Nick -g "/path/to/GTA San Andreas"
```

## Notes

Not affiliated with the open.mp or SA-MP projects. AI was used for the injector and package
management.
