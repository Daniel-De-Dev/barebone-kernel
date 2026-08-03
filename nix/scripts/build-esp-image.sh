#!/usr/bin/env bash
set -euo pipefail

bootloader_image="${BOOTLOADER_IMAGE:?BOOTLOADER_IMAGE must be set by Nix}"
output_directory="${out:?out must be set by Nix}"
partition_start_mib="${PARTITION_START_MIB:?PARTITION_START_MIB must be set by Nix}"
sector_size_bytes="${SECTOR_SIZE_BYTES:?SECTOR_SIZE_BYTES must be set by Nix}"
disk_size_bytes="${DISK_SIZE_BYTES:?DISK_SIZE_BYTES must be set by Nix}"

require_file "${bootloader_image}"
require_positive_integer "PARTITION_START_MIB" "${partition_start_mib}"
require_positive_integer "SECTOR_SIZE_BYTES" "${sector_size_bytes}"
require_positive_integer "DISK_SIZE_BYTES" "${disk_size_bytes}"

if ((sector_size_bytes != 512)); then
  usage_error "only a 512-byte sector size is currently supported"
fi

readonly mib_bytes=$((1024 * 1024))
readonly partition_start_sector=$(((partition_start_mib * mib_bytes) / sector_size_bytes))
readonly partition_size_bytes=$((disk_size_bytes - (partition_start_mib * mib_bytes)))
readonly partition_size_sectors=$((partition_size_bytes / sector_size_bytes))

if ((partition_size_bytes <= 0)); then
  usage_error "disk-size is too small to contain the partition offset"
fi

work_directory=$(mktemp -d "${TMPDIR:-/tmp}/esp-image.XXXXXX")

cleanup() {
  rm -rf -- "${work_directory}"
}
trap cleanup EXIT

disk_image="${output_directory}/disk.img"
partition_image="${work_directory}/esp-partition.img"
esp_root="${work_directory}/esp-root"
mcopy_error="${work_directory}/mcopy-error.log"

mkdir -p "${output_directory}"
install -Dm0644 \
  "${bootloader_image}" \
  "${esp_root}/EFI/BOOT/BOOTRISCV64.EFI"

truncate \
  --size="${partition_size_bytes}" \
  "${partition_image}"

mkfs.vfat \
  --invariant \
  -n ESP \
  -h "${partition_start_sector}" \
  "${partition_image}" \
  >/dev/null

if ! mcopy \
  -i "${partition_image}" \
  -s "${esp_root}/EFI" \
  ::/ \
  >/dev/null 2>"${mcopy_error}"; then
  echo "error: ESP payloads did not fit in the allocated ${partition_size_bytes} bytes partition" >&2
  cat "${mcopy_error}" >&2
  exit 1
fi

truncate \
  --size="${disk_size_bytes}" \
  "${disk_image}"

sfdisk "${disk_image}" <<EOF
label: dos
unit: sectors
start=${partition_start_sector}, size=${partition_size_sectors}, type=ef
EOF

dd \
  if="${partition_image}" \
  of="${disk_image}" \
  bs="${sector_size_bytes}" \
  seek="${partition_start_sector}" \
  conv=notrunc \
  status=none

stat -c '%s' \
  "${disk_image}" \
  >"${output_directory}/disk-size-bytes"
