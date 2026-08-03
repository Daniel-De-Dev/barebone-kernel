-- TODO: Merge shared function across .lua files to reduse duplication
-- TODO: Clean up lua scripts in project
local function required_environment_variable(name)
  local value = os.getenv(name)

  if value == nil or value == '' then
    error(name .. ' must be set by Nix or the runner')
  end

  return value
end

---@param name string
---@return function
local function tio_function(name)
  local global_function = rawget(_G, name)

  if type(global_function) == 'function' then return global_function end

  local namespace = rawget(_G, 'tio')

  if type(namespace) == 'table' and type(namespace[name]) == 'function' then
    return namespace[name]
  end

  error('tio does not provide ' .. name .. '()')
end

local ram_disk_address =
  required_environment_variable('MANGOPI_RAM_DISK_ADDRESS')
local ram_disk_blocks = required_environment_variable('MANGOPI_RAM_DISK_BLOCKS')
local efi_load_address =
  required_environment_variable('MANGOPI_EFI_LOAD_ADDRESS')

local serial_expect = tio_function('expect')
local serial_write = tio_function('write')

local function run_uboot_command(command)
  serial_write(command .. '\n')
  serial_expect('=> ')
end

print('Waiting for U-Boot...')
serial_expect('Hit any key to stop autoboot:')
serial_write('\n')
serial_expect('=> ')

print('Creating a RAM-backed block device for the uploaded ESP image...')
run_uboot_command('blkmap create fel-esp')
run_uboot_command(
  'blkmap map fel-esp 0 ' .. ram_disk_blocks .. ' mem ' .. ram_disk_address
)
run_uboot_command('blkmap get fel-esp dev feldev')

print('Loading the RISC-V EFI bootloader from the RAM-backed FAT partition...')
run_uboot_command(
  'load blkmap ${feldev}:1 ' .. efi_load_address .. ' EFI/BOOT/BOOTRISCV64.EFI'
)

print('Launching the EFI bootloader...')
serial_write('bootefi ' .. efi_load_address .. '\n')
