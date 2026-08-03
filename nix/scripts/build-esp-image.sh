#!/usr/bin/env bash
set -euo pipefail

# TODO: See if there are more standardized ways to define usage and parse
# arguments across sh files (decent amount of duplciated boilerplate)
# TODO: standardize also error handling across scripts
# TODO: review and cleanup the scripts in the project
usage() {
  cat <<'EOF'
Usage: build-esp-image \
  --bootloader FILE \
  --output DIRECTORY \
  --partition-start-mib NUMBER \
  --sector-size NUMBER \
  --disk-size NUMBER
EOF
}

usage_error() {
  echo "error: $1" >&2
  echo >&2
  usage >&2
  exit 2
}

bootloader_image=
output_directory=
partition_start_mib=
sector_size_bytes=
disk_size_bytes=

while (($# > 0)); do
  case "$1" in
  --bootloader)
    (($# >= 2)) || usage_error "--bootloader requires a file"
    bootloader_image=$2
    shift 2
    ;;
  --output)
    (($# >= 2)) || usage_error "--output requires a directory"
    output_directory=$2
    shift 2
    ;;
  --partition-start-mib)
    (($# >= 2)) || usage_error "--partition-start-mib requires a number"
    partition_start_mib=$2
    shift 2
    ;;
  --sector-size)
    (($# >= 2)) || usage_error "--sector-size requires a number"
    sector_size_bytes=$2
    shift 2
    ;;
  --disk-size)
    (($# >= 2)) || usage_error "--disk-size requires a number"
    disk_size_bytes=$2
    shift 2
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    usage_error "unknown option: $1"
    ;;
  esac
done

[[ -n ${bootloader_image} ]] ||
  usage_error "--bootloader is required"

[[ -n ${output_directory} ]] ||
  usage_error "--output is required"

[[ -n ${partition_start_mib} ]] ||
  usage_error "--partition-start-mib is required"

[[ -n ${sector_size_bytes} ]] ||
  usage_error "--sector-size is required"

[[ -n ${disk_size_bytes} ]] ||
  usage_error "--disk-size is required"

[[ -f ${bootloader_image} ]] ||
  usage_error "bootloader does not exist: ${bootloader_image}"

[[ ${partition_start_mib} =~ ^[1-9][0-9]*$ ]] ||
  usage_error "--partition-start-mib must be a positive integer"

[[ ${sector_size_bytes} =~ ^[1-9][0-9]*$ ]] ||
  usage_error "--sector-size must be a positive integer"

[[ ${disk_size_bytes} =~ ^[1-9][0-9]*$ ]] ||
  usage_error "--disk-size must be a positive integer"

# The disk is exposed by QEMU and U-Boot using 512-byte logical blocks.
if ((sector_size_bytes != 512)); then
  usage_error "only a 512-byte sector size is currently supported"
fi

readonly mib_bytes=$((1024 * 1024))
readonly partition_start_sector=$(((\
  partition_start_mib * mib_bytes) / sector_size_bytes))

# Calculate fixed partition size based on the provided disk size
readonly partition_size_bytes=$((disk_size_bytes - (partition_start_mib * mib_bytes)))
readonly partition_size_sectors=$((partition_size_bytes / sector_size_bytes))

if ((partition_size_bytes <= 0)); then
  echo "error: disk-size is too small to contain the partition offset" >&2
  exit 1
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

# Create a fixed-size partition image
truncate \
  --size="${partition_size_bytes}" \
  "${partition_image}"

mkfs.vfat \
  --invariant \
  -n ESP \
  -h "${partition_start_sector}" \
  "${partition_image}" \
  >/dev/null

# Populate the partition image
if ! mcopy \
  -i "${partition_image}" \
  -s "${esp_root}/EFI" \
  ::/ \
  >/dev/null 2>"${mcopy_error}"; then

  echo "error: ESP payloads did not fit in the allocated ${partition_size_bytes} bytes partition" >&2
  cat "${mcopy_error}" >&2
  exit 1
fi

# Create the final disk image to exact requested size
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
