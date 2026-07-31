opensbi_image="${MANGOPI_OPENSBI_IMAGE:?MANGOPI_OPENSBI_IMAGE must be set by Nix}"
opensbi_address="${MANGOPI_OPENSBI_ADDRESS:?MANGOPI_OPENSBI_ADDRESS must be set by Nix}"
uboot_image="${MANGOPI_UBOOT_IMAGE:?MANGOPI_UBOOT_IMAGE must be set by Nix}"
uboot_address="${MANGOPI_UBOOT_ADDRESS:?MANGOPI_UBOOT_ADDRESS must be set by Nix}"
disk_image="${MANGOPI_DISK_IMAGE:?MANGOPI_DISK_IMAGE must be set by Nix}"
disk_size_file="${MANGOPI_DISK_SIZE_FILE:?MANGOPI_DISK_SIZE_FILE must be set by Nix}"
ram_disk_address="${MANGOPI_RAM_DISK_ADDRESS:?MANGOPI_RAM_DISK_ADDRESS must be set by Nix}"
tio_script="${MANGOPI_TIO_SCRIPT:?MANGOPI_TIO_SCRIPT must be set by Nix}"

usage() {
  cat <<'USAGE'
Usage: run-mangopi [SERIAL_DEVICE]

Boot the MangoPi MQ Pro through FEL

U-Boot maps the in-memory disk through blkmap.

SERIAL_DEVICE defaults to /dev/ttyUSB0.

Options:
  -h, --help    Show this help
USAGE
}

usage_error() {
  echo "error: $1" >&2
  echo >&2
  usage >&2
  exit 2
}

serial_device="${MANGOPI_SERIAL_PORT:-/dev/ttyUSB0}"

if (($# > 1)); then
  usage_error "expected at most one serial device"
fi

if (($# == 1)); then
  case "$1" in
  -h | --help)
    usage
    exit 0
    ;;
  -*)
    usage_error "unknown option: $1"
    ;;
  *)
    serial_device="$1"
    ;;
  esac
fi

if [[ ! -e ${serial_device} ]]; then
  usage_error "serial device does not exist: ${serial_device}"
fi

if [[ ! -f ${disk_image} ]]; then
  usage_error "ESP disk image does not exist: ${disk_image}"
fi

if [[ ! -f ${disk_size_file} ]]; then
  usage_error "ESP disk size metadata does not exist: ${disk_size_file}"
fi

disk_size_bytes=$(stat -c '%s' "${disk_image}")
reserved_size_bytes=$(<"${disk_size_file}")

if ((disk_size_bytes == 0)); then
  usage_error "ESP disk image is empty"
fi

if ((disk_size_bytes % 512 != 0)); then
  usage_error "ESP disk image size must be a multiple of 512 bytes"
fi

if ((disk_size_bytes != reserved_size_bytes)); then
  usage_error \
    "ESP disk image (${disk_size_bytes} bytes) does not match the DTB reservation (${reserved_size_bytes} bytes)"
fi

disk_blocks=$((disk_size_bytes / 512))
printf -v disk_blocks_hex '0x%x' "${disk_blocks}"

# The Lua script uses these values when it creates the U-Boot blkmap device.
export MANGOPI_RAM_DISK_BLOCKS="${disk_blocks_hex}"

if ! xfel version >/dev/null 2>&1; then
  echo "error: unable to access a D1 device in FEL mode" >&2
  echo "Connect the USB OTG port and check USB permissions." >&2
  exit 1
fi

echo "Initializing D1 DDR..."
xfel ddr d1

echo "Uploading U-Boot proper to ${uboot_address}..."
xfel write "${uboot_address}" "${uboot_image}"

echo "Uploading the ${disk_size_bytes}-byte ESP disk to ${ram_disk_address}..."
xfel write "${ram_disk_address}" "${disk_image}"

echo "Uploading OpenSBI FW_JUMP to ${opensbi_address}..."
xfel write "${opensbi_address}" "${opensbi_image}"

echo "Starting OpenSBI..."
xfel exec "${opensbi_address}"

echo "Attaching to U-Boot on ${serial_device}..."
exec tio \
  --baudrate 115200 \
  --databits 8 \
  --flow none \
  --stopbits 1 \
  --parity none \
  --no-reconnect \
  --script-file "${tio_script}" \
  --script-run once \
  "${serial_device}"
