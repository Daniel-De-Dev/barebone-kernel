spl_image="${VF2_SPL_IMAGE:?VF2_SPL_IMAGE must be set by Nix}"
uboot_image="${VF2_UBOOT_IMAGE:?VF2_UBOOT_IMAGE must be set by Nix}"
disk_image="${VF2_DISK_IMAGE:?VF2_DISK_IMAGE must be set by Nix}"
disk_size_file="${VF2_DISK_SIZE_FILE:?VF2_DISK_SIZE_FILE must be set by Nix}"
tio_script="${VF2_TIO_SCRIPT:?VF2_TIO_SCRIPT must be set by Nix}"

program_name="$(basename "$0")"

usage() {
  cat <<USAGE
Usage: ${program_name} [SERIAL_DEVICE]

Boot the UEFI bootloader on a VisionFive 2 over UART.
SERIAL_DEVICE defaults to /dev/ttyUSB0.

The board must be configured for UART boot.
Start this command with the USB-to-UART adapter connected,
then power on or reset the board. Ensure you have the
necessary permissions.

Options:
  -h, --help    Show this help
USAGE
}

serial_device="${VF2_SERIAL_PORT:-/dev/ttyUSB0}"
positional_args=()

while (($# > 0)); do
  case "$1" in
  -h | --help)
    usage
    exit 0
    ;;
  -*)
    usage_error "unknown option: $1"
    ;;
  *)
    positional_args+=("$1")
    shift
    ;;
  esac
done

if ((${#positional_args[@]} > 1)); then
  usage_error "expected at most one serial device"
elif ((${#positional_args[@]} == 1)); then
  serial_device="${positional_args[0]}"
fi

if [[ ! -e ${serial_device} ]]; then
  usage_error "serial device does not exist: ${serial_device}"
fi

disk_blocks=$(validate_esp_image "${disk_image}" "${disk_size_file}")
printf -v disk_blocks_hex '0x%x' "${disk_blocks}"

export VF2_RAM_DISK_BLOCKS="${disk_blocks_hex}"
export VF2_SPL_IMAGE="${spl_image}"
export VF2_UBOOT_IMAGE="${uboot_image}"
export VF2_DISK_IMAGE="${disk_image}"

# TODO: Look into parametarizing these values (maybe define in boards.nix)
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
