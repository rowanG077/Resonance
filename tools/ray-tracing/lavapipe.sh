#!/usr/bin/env bash
# Run the real Bevy Solari renderer on Mesa's software Vulkan device.
set -euo pipefail
cd "$(dirname "$0")/../.."

if [[ -z "${VK_DRIVER_FILES:-}" ]]; then
    for icd in /run/opengl-driver/share/vulkan/icd.d/lvp_icd.*.json /usr/share/vulkan/icd.d/lvp_icd.*.json; do
        if [[ -f "$icd" ]]; then
            export VK_DRIVER_FILES="$icd"
            break
        fi
    done
fi
if [[ -z "${VK_DRIVER_FILES:-}" ]]; then
    echo 'Lavapipe ICD not found. Set VK_DRIVER_FILES to its lvp_icd JSON file.' >&2
    exit 1
fi

export WGPU_BACKEND=vulkan
# Mesa's acceleration-structure radix sort requires eight lanes. The default
# four-lane aarch64 subgroup crashes in rs_scatter; 512 bits also crashes.
export LP_NATIVE_VECTOR_WIDTH=256
# The shared Mesa disk cache reproduced a JIT raster segfault even at 256
# bits. Leave that cache intact and bypass it for this prototype process.
export MESA_SHADER_CACHE_DISABLE=true
export LP_NUM_THREADS="${LP_NUM_THREADS:-4}"

if [[ $# == 0 ]]; then
    set -- cargo run -p resonance-presentation --features solari --example modern_classroom -- local/raytraced-classroom.png
fi
exec "$@"
