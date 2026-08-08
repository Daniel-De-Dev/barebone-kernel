set -euo pipefail

opensbi_image="${OPENSBI_IMAGE:?OPENSBI_IMAGE must be set by Nix}"
bootloader_image="${BOOTLOADER_IMAGE:?BOOTLOADER_IMAGE must be set by Nix}"
bootloader_address="${BOOTLOADER_ADDRESS:?BOOTLOADER_ADDRESS must be set by Nix}"

program_name="$(basename "$0")"

# TODO: eventually add args for launching with graphics
usage() {
  cat <<USAGE
Usage: ${program_name} [OPTIONS]

Boot the bootloader under QEMU.

Options:
  --log FILE    Write QEMU output to FILE
  --gdb         Wait for GDB on localhost:1234
  -h, --help    Show this help
USAGE
}

log_file=""
gdb_enabled=false

while (($# > 0)); do
  case "$1" in
  -h | --help)
    usage
    exit 0
    ;;
  --log)
    if (($# < 2)); then
      usage_error "--log requires a filename"
    fi
    log_file="$2"
    shift 2
    ;;
  --gdb)
    gdb_enabled=true
    shift
    ;;
  -*)
    usage_error "unknown option: $1"
    ;;
  *)
    usage_error "unknown argument: $1"
    ;;
  esac
done

qemu_arguments=(
  -machine virt
  -m 1G
  -nographic
  -bios "${opensbi_image}"
  -device "loader,file=${bootloader_image},addr=${bootloader_address},force-raw=on"
)

if [[ ${gdb_enabled} == true ]]; then
  qemu_arguments+=(
    -S
    -gdb "tcp:127.0.0.1:1234"
  )
  echo "QEMU is waiting for GDB on localhost:1234" >&2
fi

if [[ -n ${log_file} ]]; then
  qemu-system-riscv64 "${qemu_arguments[@]}" 2>&1 |
    tee "${log_file}"
else
  qemu-system-riscv64 "${qemu_arguments[@]}"
fi
