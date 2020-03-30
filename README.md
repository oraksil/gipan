# gipan
Oraksil Gipan

## Lib
#### Generate C Bindings from Headers
There're too many options when using `bindgen`. Need to deep dive sometime. For now, we need `-x c++` for `bindgen` to understand c++ standard headers or syntax.
```
$ bindgen include/headless.h -o src/bindings.rs -- -x c++
```

#### Build
To build `libmame` as standalone, use following command. But, we don't need to build this everytime in development cycle since an app will build all dependencies that it depends by path.
```
$ cargo build --release
```

## App
#### Build
```
$ cargo build

# or for release
$ cargo build --release
```

#### Run
```
$ DYLD_LIBRARY_PATH=../../mame cargo run -- --resolution 480x320 --game dino
```
