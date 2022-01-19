# Gipan

`Gipan` means a (green) main board in the game console box. It integrates MAME emulator as a game engine, encodes video/audio frames MAME renders and provides IPC or TCP channels for Orakki to fetch the encoded data. (It's the same for player control data) It's built with Rust.


# Prerequisite

## Nanomsg 
Gipan needs Nanomsg so that it delivers image frames and takes keyboard events with orakki.

For more details about Nanomsg,
https://github.com/nanomsg/nanomsg
https://bravenewgeek.com/a-look-at-nanomsg-and-scalability-protocols/

```bash
$ git clone https://github.com/nanomsg/nanomsg.git
$ mkdir -p nanomsg/build
$ cd nanomsg/build
$ cmake ..
$ cmake --build .
$ ctest .
$ sudo cmake --build . --target install
```

## Codecs

```bash
# For Linux
$ apt-get install -y \
    libvpx-dev \
    libopus-dev

# For MacOS
$ brew install libvpx
$ brew install libopusenc
```

## Game Roms

You should get your own MAME game roms due to some license issues. Place the roms under `./roms`.

# Build & Run

## Build
```bash
$ cargo build

# For release
$ cargo build --release

# If using Makefile
$ make build_dbg
$ make build_rel
```

## Run
```bash
$ DYLD_LIBRARY_PATH=../mame RUST_BACKTRACE=1 cargo run -- \
    --imageframe-output ipc://./images.ipc \
    --soundframe-output ipc://./sounds.ipc \
    --cmd-input ipc://./cmds.ipc \
    --resolution 480x320 \
    --fps 23 \
    --keyframe-interval 48 \
    --game dino

# Or with Makefile
$ make run_dbg
$ make run_rel
```
