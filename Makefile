# SILENT = @

TARGET_DBG = ./target/debug/gipan
TARGET_REL = ./target/release/gipan

MAME_HOME = ../mame
GAME = dino

RUN_ARGS = \
	--imageframe-output ipc://./images.ipc \
	--soundframe-output ipc://./sounds.ipc \
	--cmd-input ipc://./cmds.ipc \
	--resolution 480x320 \
	--fps 23 \
	--keyframe-interval 48 \
	--game $(GAME)

build_dbg:
	cargo build

build_rel:
	cargo build --release

run_dbg: build_dbg
	LD_LIBRARY_PATH=$(MAME_HOME) RUST_BACKTRACE=1 $(TARGET_DBG) $(RUN_ARGS)

run_rel: build_rel
	LD_LIBRARY_PATH=$(MAME_HOME) $(TARGET_REL) $(RUN_ARGS)

