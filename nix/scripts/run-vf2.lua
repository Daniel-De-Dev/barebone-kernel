local function required_environment_variable(name)
  local value = os.getenv(name)

  if value == nil or value == '' then error(name .. ' must be set by Nix') end

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

local spl_image = required_environment_variable('VF2_SPL_IMAGE')
local uboot_image = required_environment_variable('VF2_UBOOT_IMAGE')
local disk_image = required_environment_variable('VF2_DISK_IMAGE')
local ram_disk_address = required_environment_variable('VF2_RAM_DISK_ADDRESS')
local ram_disk_blocks = required_environment_variable('VF2_RAM_DISK_BLOCKS')
local efi_load_address = required_environment_variable('VF2_EFI_LOAD_ADDRESS')

local modem_send = tio_function('send')
local serial_expect = tio_function('expect')
local serial_write = tio_function('write')

local function run_uboot_command(command)
  serial_write(command .. '\n')
  serial_expect('StarFive #')
end

-- TODO: Add new lines to all prints for better terminal rendering
print('Sending SPL image with XMODEM-1K...')
print('Ensure VF2 is powered on, bootmode set to UART and waiting for data')
modem_send(spl_image, XMODEM_1K)

print('Sending OpenSBI and U-Boot with YMODEM...')
modem_send(uboot_image, YMODEM)

print('Waiting for U-Boot...')
serial_expect('Hit any key to stop autoboot:')
serial_write('\n')
serial_expect('StarFive #')

print('Uploading ESP disk image to RAM via YMODEM...')
serial_write('loady ' .. ram_disk_address .. '\n')
modem_send(disk_image, YMODEM)
serial_expect('StarFive #')

print('Creating a RAM-backed block device for the uploaded ESP image...')
run_uboot_command('blkmap create ram-esp')
run_uboot_command(
  'blkmap map ram-esp 0 ' .. ram_disk_blocks .. ' mem ' .. ram_disk_address
)
run_uboot_command('blkmap get ram-esp dev espdev')

print('Loading the bootloader from the RAM-backed FAT partition...')
run_uboot_command(
  'load blkmap ${espdev}:1 ' .. efi_load_address .. ' EFI/BOOT/BOOTRISCV64.EFI'
)

print('Launching the EFI bootloader...')
serial_write('bootefi ' .. efi_load_address .. '\n')
