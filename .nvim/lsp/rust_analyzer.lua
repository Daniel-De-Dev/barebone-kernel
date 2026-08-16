local source = debug.getinfo(1, 'S').source:sub(2)
local lsp_dir = vim.fs.dirname(vim.fn.fnamemodify(source, ':p'))

local project_root = vim.fs.dirname(vim.fs.dirname(lsp_dir))

local kernel_root = vim.fs.normalize(project_root .. '/kernel')
local fdt_root = vim.fs.normalize(project_root .. '/fdt')

local function is_under(path, root)
  path = vim.fs.normalize(path)
  root = vim.fs.normalize(root)

  return path == root or vim.startswith(path, root .. '/')
end

return {
  root_dir = function(bufnr, on_dir)
    local path = vim.api.nvim_buf_get_name(bufnr)

    if is_under(path, kernel_root) then
      on_dir(kernel_root)
    elseif is_under(path, fdt_root) then
      on_dir(fdt_root)
    end
  end,

  settings = {
    ['rust-analyzer'] = {
      cargo = {
        allTargets = false,
      },

      check = {
        workspace = false,
      },
    },
  },

  before_init = function(_, config)
    local root = vim.fs.normalize(config.root_dir)

    ---@type lsp.LSPObject
    local settings = assert(config.settings)
    local current = settings['rust-analyzer']

    assert(type(current) == 'table')

    if root == kernel_root then
      settings['rust-analyzer'] = vim.tbl_deep_extend('force', current, {
        cargo = {
          target = 'riscv64gc-unknown-none-elf',
        },
        check = {
          allTargets = false,
        },
      })
    elseif root == fdt_root then
      settings['rust-analyzer'] = vim.tbl_deep_extend('force', current, {
        check = {
          allTargets = true,
        },
      })
    end
  end,
}
