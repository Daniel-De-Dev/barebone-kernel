opensbi_image="${MANGOPI_OPENSBI_IMAGE:?MANGOPI_OPENSBI_IMAGE must be set by Nix}"
opensbi_address="${MANGOPI_OPENSBI_ADDRESS:?MANGOPI_OPENSBI_ADDRESS must be set by Nix}"
kernel_image="${MANGOPI_KERNEL_IMAGE:?MANGOPI_KERNEL_IMAGE must be set by Nix}"
kernel_address="${MANGOPI_KERNEL_ADDRESS:?MANGOPI_KERNEL_ADDRESS must be set by Nix}"
baudrate="${MANGOPI_BAUDRATE:?MANGOPI_BAUDRATE must be set by Nix}"

program_name="$(basename "$0")"

usage() {
  cat <<USAGE
Usage: ${program_name} [SERIAL_DEVICE]

Boot the kernel on a MangoPi MQ Pro through FEL.

SERIAL_DEVICE defaults to MANGOPI_SERIAL_PORT when set, otherwise /dev/ttyUSB0.

The board must be connected through its USB OTG port and available in FEL
mode. A USB-to-UART adapter is also required to view the serial console.
Ensure you have the necessary USB and serial-device permissions.

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

if ! xfel version >/dev/null 2>&1; then
  echo "error: unable to access a D1 device in FEL mode" >&2
  echo "Connect the USB OTG port and check USB permissions." >&2
  exit 1
fi

echo "Initializing D1 DDR..."
xfel ddr d1
echo "Uploading kernel to ${kernel_address}..."
xfel write "${kernel_address}" "${kernel_image}"
echo "Uploading OpenSBI to ${opensbi_address}..."
xfel write "${opensbi_address}" "${opensbi_image}"
echo "Starting OpenSBI..."
xfel exec "${opensbi_address}"

exec tio \
  --baudrate "${baudrate}" \
  --databits 8 --flow none --stopbits 1 --parity none \
  --no-reconnect \
  "${serial_device}"
