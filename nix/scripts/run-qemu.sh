set -euo pipefail

esp_image="${ESP_IMAGE:?ESP_IMAGE must be set by Nix}"
uboot_image="${UBOOT_IMAGE:?UBOOT_IMAGE must be set by Nix}"
fit_load_address="${FIT_LOAD_ADDRESS:?FIT_LOAD_ADDRESS must be set by Nix}"

program_name="$(basename "$0")"

# TODO: eventually add args for launching with graphics
usage() {
  cat <<USAGE
Usage: ${program_name} [OPTIONS]

Run the RISC-V bootloader & kernel under QEMU.

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

workdir="$(mktemp -d -t run-qemu.XXXXXX)"
disk_image="${workdir}/disk.img"

cleanup() {
  rm -f -- "${disk_image}"
  rmdir -- "${workdir}"
}
trap cleanup EXIT

cp --reflink=auto "${esp_image}/disk.img" "${disk_image}"
chmod u+w "${disk_image}"

qemu_arguments=(
  -machine virt
  -m 1G
  -nographic
  -bios "${uboot_image}/u-boot-spl"
  -device "loader,file=${uboot_image}/u-boot.itb,addr=${fit_load_address}"
  -object "rng-random,filename=/dev/urandom,id=rng0"
  -device "virtio-rng-device,rng=rng0"
  -drive "file=${disk_image},format=raw,if=virtio"
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
