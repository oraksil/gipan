#!/usr/bin/env bash

LD_LIBRARY_PATH=$APP_HOME/mame \
  ./gipan \
  --imageframe-output $IPC_IMAGE_FRAMES \
  --soundframe-output $IPC_SOUND_FRAMES \
  --key-input $IPC_KEY_INPUTS \
  --resolution 640x480 \
  --fps 25 \
  --keyframe-interval 150 \
  --idle-time-to-enc-sleep 300 \
  --game $GAME
