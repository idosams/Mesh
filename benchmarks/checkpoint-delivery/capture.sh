#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo"

cargo build --release -p mesh-daemon --example checkpoint-delivery
sample=1
while [ "$sample" -le 5 ]; do
  target/release/examples/checkpoint-delivery "$sample"
  sample=$((sample + 1))
done
