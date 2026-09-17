#!/usr/bin/env bash
# Run a toolkit spike on a chosen GPU: ./run.sh <gpui|floem> <intel|nvidia> [rows]
# GPU selection restricts the Vulkan loader to one ICD (NixOS paths).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
toolkit="${1:?toolkit}"; gpu="${2:-intel}"; rows="${3:-100000}"
case "$gpu" in
  intel)  icd=intel_icd.x86_64.json ;;
  nvidia) icd=nvidia_icd.json ;;
  *) echo "gpu must be intel or nvidia" >&2; exit 2 ;;
esac
export VK_DRIVER_FILES="/run/opengl-driver/share/vulkan/icd.d/$icd"
export VK_ICD_FILENAMES="$VK_DRIVER_FILES"
export WGPU_BACKEND=vulkan
export SPIKE_GPU="$gpu"
mkdir -p "$here/results"
(cd "$here" && cargo build --release -q -p "spike-$toolkit")
exec "$here/target/release/spike-$toolkit" \
  --rows "$rows" --images "$here/../bench-data/images10k" --duration 5 \
  --out "$here/results/$toolkit-$gpu.json"
