#!/usr/bin/env bash

LD_LIBRARY_PATH=$APP_HOME/mame \
  ./gipan \
  --imageframe-output ipc://$IPC_IMAGE_FRAMES \
  --soundframe-output ipc://$IPC_SOUND_FRAMES \
  --key-input ipc://$IPC_KEY_INPUTS \
  --resolution 480x320 \
  --fps 23 \
  --keyframe-interval 80 \
  --idle-time-to-enc-sleep 10 \
  --game $GAME
