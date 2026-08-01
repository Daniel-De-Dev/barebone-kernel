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
  --sector-size NUMBER
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

[[ -f ${bootloader_image} ]] ||
  usage_error "bootloader does not exist: ${bootloader_image}"

[[ ${partition_start_mib} =~ ^[1-9][0-9]*$ ]] ||
  usage_error "--partition-start-mib must be a positive integer"

[[ ${sector_size_bytes} =~ ^[1-9][0-9]*$ ]] ||
  usage_error "--sector-size must be a positive integer"

# The disk is exposed by QEMU and U-Boot using 512-byte logical blocks.
if ((sector_size_bytes != 512)); then
  usage_error "only a 512-byte sector size is currently supported"
fi

readonly mib_bytes=$((1024 * 1024))
readonly sectors_per_mib=$((mib_bytes / sector_size_bytes))
readonly partition_start_sector=$(((\
  partition_start_mib * mib_bytes) / sector_size_bytes))
readonly max_partition_size_mib=1024

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

payload_size_bytes=$(
  du --summarize --bytes "${esp_root}" |
    cut --fields=1
)

partition_size_mib=$(((\
  payload_size_bytes + mib_bytes - 1) / mib_bytes))

if ((partition_size_mib < 1)); then
  partition_size_mib=1
fi

while true; do
  rm -f "${partition_image}"

  truncate \
    --size="${partition_size_mib}M" \
    "${partition_image}"

  mkfs.vfat \
    --invariant \
    -n ESP \
    -h "${partition_start_sector}" \
    "${partition_image}" \
    >/dev/null

  # WARN: mcopy does not differ between failures which can result uncessary iteration
  if mcopy \
    -i "${partition_image}" \
    -s "${esp_root}/EFI" \
    ::/ \
    >/dev/null 2>"${mcopy_error}"; then
    break
  fi

  if ((partition_size_mib >= max_partition_size_mib)); then
    echo \
      "error: ESP payloads did not fit in a ${max_partition_size_mib} MiB partition" \
      >&2

    cat "${mcopy_error}" >&2
    exit 1
  fi

  partition_size_mib=$((partition_size_mib + 1))
done

disk_size_mib=$((\
  partition_start_mib + partition_size_mib))

partition_size_sectors=$((\
  partition_size_mib * sectors_per_mib))

truncate \
  --size="${disk_size_mib}M" \
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
