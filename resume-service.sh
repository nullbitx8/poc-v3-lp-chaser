#!/bin/bash
set -euxo pipefail

# print usage
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <token-name>"
    exit 1
fi

# take first arg as name of token
TOKEN_NAME=$1

# directory where token info lives
RUN_DIR=/home/node/workspace/running-v3-chaser/$TOKEN_NAME

# stop the service
sudo systemctl start $TOKEN_NAME

# disable the service to no longer start on boot
sudo systemctl enable $TOKEN_NAME
