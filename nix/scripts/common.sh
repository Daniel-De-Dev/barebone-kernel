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
