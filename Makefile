SILENT = @

TARGET_DBG = ./target/debug/gipan
TARGET_REL = ./target/release/gipan

MAME_HOME = ../mame

RUN_ARGS = --imageframe-output ipc://./imageframes.ipc \
    --soundframe-output ipc://./soundframes.ipc \
    --key-input ipc://./keys.ipc \
    --resolution 480x360 \
    --fps 24 \
    --keyframe-interval 10 \
    --game dino

build_dbg:
	cargo build

build_rel:
	cargo build --release

run_dbg: build_dbg
	DYLD_LIBRARY_PATH=$(MAME_HOME) RUST_BACKTRACE=1 $(TARGET_DBG) $(RUN_ARGS)

run_rel: build_rel
	DYLD_LIBRARY_PATH=$(MAME_HOME) $(TARGET_REL) $(RUN_ARGS)
