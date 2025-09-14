#!/usr/bin/env bash
set -euo pipefail
TAG=${1:?"usage: $0 <tag>"}
docker pull "theblock/node:$TAG"
docker tag "theblock/node:$TAG" theblock/node:latest
docker compose -f deploy/docker-compose.yml up -d node1 node2
echo "rolled back to $TAG"
