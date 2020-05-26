SILENT = @

TARGET_DBG = ./target/debug/gipan
TARGET_REL = ./target/release/gipan

MAME_HOME = ../mame

RUN_ARGS = --imageframe-output ipc://./imageframes.ipc \
    --soundframe-output ipc://./soundframes.ipc \
    --key-input ipc://./keys.ipc \
    --resolution 480x320 \
    --fps 20 \
    --keyframe-interval 250 \
    --game mslug5
#    --game bublbobl
#    --game tekken3
#    --game s1945iii
#    --game dino
#    --game hsf2
#    --game ddsomu
#    --game ffightu
#    --game kof97pls
#    --game dynamcop
#		 --game suprslam

build_dbg:
	cargo build

build_rel:
	cargo build --release

run_dbg: build_dbg
	LD_LIBRARY_PATH=$(MAME_HOME) RUST_BACKTRACE=1 $(TARGET_DBG) $(RUN_ARGS)

run_rel: build_rel
	LD_LIBRARY_PATH=$(MAME_HOME) $(TARGET_REL) $(RUN_ARGS)
