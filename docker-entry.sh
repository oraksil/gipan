#!/usr/bin/env bash

LD_LIBRARY_PATH=$APP_HOME/mame \
  ./gipan \
  --imageframe-output $IPC_IMAGE_FRAMES \
  --soundframe-output $IPC_SOUND_FRAMES \
  --cmd-input $IPC_CMD_INPUTS \
  --resolution $RESOLUTION \
  --fps $FPS \
  --keyframe-interval $KEYFRAME_INTERVAL \
  --game $GAME
