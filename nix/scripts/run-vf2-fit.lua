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

local fit_image = required_environment_variable('VF2_FIT_IMAGE')

local modem_send = tio_function('send')

modem_send(fit_image, YMODEM)
