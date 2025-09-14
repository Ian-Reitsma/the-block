#!/usr/bin/env bash
set -euo pipefail
IMAGE=${1:?"usage: $0 <new-image-tag>"}
COMPOSE=${COMPOSE_FILE:-deploy/docker-compose.yml}
SERVICE=${SERVICE:-node1}
# verify signed metadata
cosign verify --key cosign.pub "$IMAGE"
# start new container alongside existing one
NEW_SERVICE=${SERVICE}-new
docker compose -f "$COMPOSE" up -d --no-deps --scale $SERVICE=1 --no-build $NEW_SERVICE || \
  docker compose -f "$COMPOSE" run -d --name "$NEW_SERVICE" $SERVICE "$IMAGE"
# wait for health
for i in {1..30}; do
  if docker inspect --format='{{.State.Health.Status}}' "$NEW_SERVICE" 2>/dev/null | grep -q healthy; then
    break
  fi
  sleep 2
done
# swap
docker compose -f "$COMPOSE" stop $SERVICE
docker compose -f "$COMPOSE" rm -f $SERVICE
docker rename "$NEW_SERVICE" "$SERVICE"
echo "upgrade complete"
