#!/usr/bin/env bash
set -xeuo pipefail

declare -A SECRET PUBKEY BIND

ID[1]="1111111111111111111111111111111111111111111111111111111111111111"
SECRET[1]="3d1RiRMXUV1MS2ZQzAruMHRqxa9GWdHswjp4C1PEoMKo7bLzSwoCr7awHqbSgVsG6JgmbU"
PUBKEY[1]="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTFboatKRK6R4nX1s64R5Ec3gszK7i8fxnFVjT4c2BLZh6rT1bx8xLFnejjDNwJpdvsyJoVmsdTbjrLCG2vWDKk5Fg"
BIND[1]="127.0.0.1:9000"

ID[2]="2222222222222222222222222222222222222222222222222222222222222222"
SECRET[2]="3d1RiRMXUUo8Hfyq54MYzz5Vc7CZZZL1YXTaWnKegnN1nvCQv69mFfEbMzaJWJcgoVN47x"
PUBKEY[2]="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTGwfYaU7QX6WHzudC4KeQvRkrnCziSEZ8boSUa8dPsZ6Fg4t3YeEu6kWX3oSQPmgfRkNHY69Q5cDnYZaDBoE67FA4"
BIND[2]="127.0.0.1:9001"

ID[3]="3333333333333333333333333333333333333333333333333333333333333333"
SECRET[3]="3d1RiRMXUVSxiALyJM4wMSsmV36LdXdvETuznTbyWtxQ3okQaDfVdx6498LEPMpDo6uM7k"
PUBKEY[3]="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTJBWHBYxY7ynPB1dcsdhe8HxqitwjtdeAXs1HGh3dPYhgQyV2vmmBxsfaQv9CT3T3u4grD17v2oR8pJ5CgRfEN899"
BIND[3]="127.0.0.1:9002"

ID[4]="4444444444444444444444444444444444444444444444444444444444444444"
SECRET[4]="3d1RiRMXUVQvgZVQGBA3LuHHxREXebxQofMvUV5o7qG9oy74B9NdAuzKNXF8ijUqKHgrFY"
PUBKEY[4]="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTHwcMq73gSqTqqvrvJadc3tMpZYLJLeihNrnRhJNdRJELoVM3LoJqZW2yFT9yenB1cyRjGzivuNLcAv5pVLj96NXt"
BIND[4]="127.0.0.1:9003"

usage() {
  echo "Usage: $0 <node-index> [extra node args...]"
  exit 1
}

[[ $# -ge 1 ]] || usage
N=$1
shift
[[ -v SECRET[$N] ]] || {
  echo "Unknown node: $N"
  usage
}

PEER_ARGS=()

for I in "${!SECRET[@]}"; do
  [[ "$I" == "$N" ]] && continue
  PEER_ARGS+=(--peer "${PUBKEY[$I]}@${BIND[$I]}")
done

exec cargo run -p vertex-veil-agents --features vertex-transport -- node \
  --bind "${BIND[$N]}" \
  --secret "${SECRET[$N]}" \
  --node-id "${ID[$N]}" \
  --topology fixtures/topology-4node.toml \
  --private-intents fixtures/topology-4node.private.toml \
  "${PEER_ARGS[@]}" \
  "$@"
