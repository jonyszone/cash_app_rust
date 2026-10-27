#!/bin/sh
# Platform volumes (e.g. Fly.io) are mounted root-owned, so fix ownership of the
# data directory, then drop privileges before starting the server.
set -e
if [ "$(id -u)" = "0" ]; then
    chown app:app /data
    exec setpriv --reuid=app --regid=app --init-groups "$@"
fi
exec "$@"
