#!/usr/bin/env bash
set -euo pipefail

export OZZIE_PATH=./dev_home

exec cargo run -- "$@"
