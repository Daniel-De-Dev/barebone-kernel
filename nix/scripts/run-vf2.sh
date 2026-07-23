spl_image="${VF2_SPL_IMAGE:?VF2_SPL_IMAGE must be set by Nix}"
uboot_image="${VF2_UBOOT_IMAGE:?VF2_UBOOT_IMAGE must be set by Nix}"
bootloader_image="${VF2_BOOTLOADER_IMAGE:?VF2_BOOTLOADER_IMAGE must be set by Nix}"
tio_script="${VF2_TIO_SCRIPT:?VF2_TIO_SCRIPT must be set by Nix}"

usage() {
  cat <<'EOF'
Usage: run-vf2 [SERIAL_DEVICE]

Boot the UEFI bootloader on a VisionFive 2 over UART.

Defualts to /dev/ttyUSB0

The board must be configured for UART boot. Start this command with the
USB-to-UART adapter connected, then power on or reset the board.

Examples:
  run-vf2 /dev/ttyUSB0

Options:
  -h, --help    Show this help
EOF
}

usage_error() {
  echo "error: $1" >&2
  echo >&2
  usage >&2
  exit 2
}

serial_device="${VF2_SERIAL_PORT:-/dev/ttyUSB0}"

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

export VF2_SPL_IMAGE="${spl_image}"
export VF2_UBOOT_IMAGE="${uboot_image}"
export VF2_BOOTLOADER_IMAGE="${bootloader_image}"

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
