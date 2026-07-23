--- Lua script that is supposed to be provided to tio so it can transfer
--- the payload via usart booting for vf2 deterministically

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
local bootloader_image = required_environment_variable('VF2_BOOTLOADER_IMAGE')

local modem_send = tio_function('send')
local serial_expect = tio_function('expect')
local serial_write = tio_function('write')

print('Sending SPL image with XMODEM-1K...')
print('Ensure VF2 is powered on, bootmode set to UART and waiting for data')
modem_send(spl_image, XMODEM_1K)

print('Sending OpenSBI and U-Boot with YMODEM...')
modem_send(uboot_image, YMODEM)

print('Waiting for U-Boot...')
serial_expect('Hit any key to stop autoboot:')
serial_write('\n')
serial_expect('StarFive #')

print('Sending the UEFI bootloader with YMODEM...')
serial_write('loady ${loadaddr}\n')
modem_send(bootloader_image, YMODEM)

serial_expect('StarFive #')

print('Launching the UEFI bootloader...')
serial_write('bootefi ${loadaddr}\n')
