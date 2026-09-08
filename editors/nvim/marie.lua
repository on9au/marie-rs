-- Drop this in ~/.config/nvim/plugin/ to attach mrs-lsp to MARIE assembly.
--
-- Files in plugin/ are sourced automatically, so this works with LazyVim or any
-- other setup without being a plugin spec. Needs Neovim 0.11 or newer for
-- vim.lsp.config; see the README in this directory for older versions.

-- Nothing knows what a .mas file is, so teach it first.
vim.filetype.add({ extension = { mas = "marie", mar = "marie" } })

-- Point this at your build, or drop the path entirely once `marie-lsp` is on PATH.
local server = vim.fn.expand("~/Projects/marie-rs/target/release/marie-lsp")
if vim.fn.executable(server) == 0 then
  server = "marie-lsp"
end

vim.lsp.config("marie", {
  cmd = { server },
  filetypes = { "marie" },
  -- MARIE programs are single files; fall back to the file's own directory.
  root_markers = { ".git" },
})

vim.lsp.enable("marie")

vim.api.nvim_create_autocmd("LspAttach", {
  callback = function(args)
    local client = vim.lsp.get_client_by_id(args.data.client_id)
    if not client or client.name ~= "marie" then
      return
    end
    -- The address and assembled word for every line, which is the whole point.
    if client:supports_method("textDocument/inlayHint") then
      vim.lsp.inlay_hint.enable(true, { bufnr = args.buf })
    end
    local map = function(keys, fn, desc)
      vim.keymap.set("n", keys, fn, { buffer = args.buf, desc = desc })
    end
    map("grn", vim.lsp.buf.rename, "Rename label")
    map("gra", vim.lsp.buf.code_action, "Quick fix")
    map("grr", vim.lsp.buf.references, "Find references")
    map("K", vim.lsp.buf.hover, "Hover")
  end,
})
