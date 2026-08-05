usage_error() {
  echo "error: $1" >&2
  echo >&2
  # Call the script-specific usage function if it is defined
  if type usage &>/dev/null; then
    usage >&2
  fi
  exit 2
}

require_file() {
  if [[ ! -f $1 ]]; then
    usage_error "file does not exist: $1"
  fi
}

require_positive_integer() {
  local name="$1"
  local value="$2"
  if [[ ! ${value} =~ ^[1-9][0-9]*$ ]]; then
    usage_error "${name} must be a positive integer, got: ${value}"
  fi
}

validate_esp_image() {
  local disk_image="$1"
  local disk_size_file="$2"

  require_file "${disk_image}"
  require_file "${disk_size_file}"

  local disk_size_bytes
  local reserved_size_bytes

  disk_size_bytes=$(stat -c '%s' "${disk_image}")
  reserved_size_bytes=$(<"${disk_size_file}")

  if ((disk_size_bytes == 0)); then
    usage_error "ESP disk image is empty"
  fi
  if ((disk_size_bytes % 512 != 0)); then
    usage_error "ESP disk image size must be a multiple of 512 bytes"
  fi
  if ((disk_size_bytes != reserved_size_bytes)); then
    usage_error "ESP disk image (${disk_size_bytes} bytes) does not match the DTB reservation (${reserved_size_bytes} bytes)"
  fi

  echo $((disk_size_bytes / 512))
}
