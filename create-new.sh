#!/bin/bash
set -euxo pipefail

# print usage
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <token-name>"
    exit 1
fi

# take first arg as name of token
TOKEN_NAME=$1

# create directory for running the script
RUN_DIR=/home/node/workspace/running-v3-chaser/$TOKEN_NAME
mkdir -p $RUN_DIR

# copy the config template to the running directory
cp sample-config.json $RUN_DIR/$TOKEN_NAME.json

# edit the config file
vi $RUN_DIR/$TOKEN_NAME.json

# create service file from the contents below
cat <<EOF > $RUN_DIR/$TOKEN_NAME.service
[Unit]
Description=V3 Chaser $TOKEN_NAME

[Service]
User=node
WorkingDirectory=/usr/local/bin
ExecStart=poc-v3-lp-chaser $RUN_DIR/$TOKEN_NAME.json
# optional items below
Restart=always
RestartSec=3
StandardOutput=append:$RUN_DIR/$TOKEN_NAME.log
StandardError=append:$RUN_DIR/$TOKEN_NAME.log

[Install]
WantedBy=multi-user.target
EOF

# copy the service file to the systemd directory
sudo cp $RUN_DIR/$TOKEN_NAME.service /etc/systemd/system/

# reload the systemd daemon
sudo systemctl daemon-reload

# start the service
sudo systemctl start $TOKEN_NAME

# enable the service to start on boot
sudo systemctl enable $TOKEN_NAME

# check the status of the service
sudo systemctl status $TOKEN_NAME

# check the logs of the service
sudo journalctl -u $TOKEN_NAME -f
