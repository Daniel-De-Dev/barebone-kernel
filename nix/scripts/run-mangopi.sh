opensbi_image="${MANGOPI_OPENSBI_IMAGE:?MANGOPI_OPENSBI_IMAGE must be set by Nix}"
opensbi_address="${MANGOPI_OPENSBI_ADDRESS:?MANGOPI_OPENSBI_ADDRESS must be set by Nix}"
uboot_image="${MANGOPI_UBOOT_IMAGE:?MANGOPI_UBOOT_IMAGE must be set by Nix}"
uboot_address="${MANGOPI_UBOOT_ADDRESS:?MANGOPI_UBOOT_ADDRESS must be set by Nix}"
disk_image="${MANGOPI_DISK_IMAGE:?MANGOPI_DISK_IMAGE must be set by Nix}"
disk_size_file="${MANGOPI_DISK_SIZE_FILE:?MANGOPI_DISK_SIZE_FILE must be set by Nix}"
ram_disk_address="${MANGOPI_RAM_DISK_ADDRESS:?MANGOPI_RAM_DISK_ADDRESS must be set by Nix}"
baudrate="${MANGOPI_BAUDRATE:?MANGOPI_BAUDRATE must be set by Nix}"
tio_script="${MANGOPI_TIO_SCRIPT:?MANGOPI_TIO_SCRIPT must be set by Nix}"

program_name="$(basename "$0")"

usage() {
  cat <<USAGE
Usage: ${program_name} [SERIAL_DEVICE]

Boot the MangoPi MQ Pro through FEL
U-Boot maps the in-memory disk through blkmap.
SERIAL_DEVICE defaults to /dev/ttyUSB0.

Options:
  -h, --help    Show this help
USAGE
}

serial_device="${MANGOPI_SERIAL_PORT:-/dev/ttyUSB0}"
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
export MANGOPI_RAM_DISK_BLOCKS="${disk_blocks_hex}"

if ! xfel version >/dev/null 2>&1; then
  echo "error: unable to access a D1 device in FEL mode" >&2
  echo "Connect the USB OTG port and check USB permissions." >&2
  exit 1
fi

disk_size_bytes=$(stat -c '%s' "${disk_image}")
disk_size_mib=$((disk_size_bytes / 1024 / 1024))

echo "Initializing D1 DDR..."
xfel ddr d1
echo "Uploading U-Boot proper to ${uboot_address}..."
xfel write "${uboot_address}" "${uboot_image}"
echo "Uploading the ${disk_size_bytes}-byte (${disk_size_mib} MiB) ESP disk to ${ram_disk_address}..."
xfel write "${ram_disk_address}" "${disk_image}"
echo "Uploading OpenSBI to ${opensbi_address}..."
xfel write "${opensbi_address}" "${opensbi_image}"
echo "Starting OpenSBI..."
xfel exec "${opensbi_address}"

exec tio \
  --baudrate "${baudrate}" \
  --databits 8 --flow none --stopbits 1 --parity none \
  --no-reconnect \
  --script-file "${tio_script}" \
  --script-run once \
  "${serial_device}"
