# gipan
Oraksil Gipan

## Lib
### Generate C Bindings from Headers (Manually)
There're too many options when using `bindgen`. Need to deep dive sometime. For now, we need `-x c++` for `bindgen` to understand c++ standard headers or syntax.
```
$ cd libemu
$ bindgen libemu/include/headless.h -o libemu/src/bindings.rs -- -x c++
```

### Build
To build `libmame` as standalone, use following command. But, we don't need to build this everytime in development cycle since an app will build all dependencies that it depends by path.
```
$ cd libemu
$ cargo build --release
```

## App (ctrl)
### Prerequisite
#### Nanomsg lib
Gipan needs Nanomsg so that it delivers image frames and takes keyboard events with orakki.

For more details about Nanomsg,
https://github.com/nanomsg/nanomsg
https://bravenewgeek.com/a-look-at-nanomsg-and-scalability-protocols/

```
$ git clone https://github.com/nanomsg/nanomsg.git
$ mkdir -p nanomsg/build
$ cd nanomsg/build
$ cmake ..
$ cmake --build .
$ ctest .
$ sudo cmake --build . --target install
```

#### libvpx

```
$ brew install libvpx
$ brew install libopusenc
```

### Build
```
$ cd ctrl
$ cargo build

# or for release
$ cargo build --release
```

### Run
```
$ DYLD_LIBRARY_PATH=../mame RUST_BACKTRACE=1 cargo run -- \
    --imageframe-output ipc://./imageframes.ipc \
    --soundframe-output ipc://./soundframes.ipc \
    --key-input ipc://./keys.ipc \
    --resolution 640x480 \
    --fps 30 \
    --keyframe-interval 12 \
    --game dino
```
