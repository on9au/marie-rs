# Neovim

Copy `marie.lua` into `~/.config/nvim/plugin/`, adjusting the path to the binary at
the top if your checkout is elsewhere:

    cp editors/nvim/marie.lua ~/.config/nvim/plugin/

Then open a program and check it attached:

    nvim examples/demo.mas
    :checkhealth vim.lsp

Requires Neovim 0.11+ for `vim.lsp.config`. On 0.10 or older, replace the
`vim.lsp.config`/`vim.lsp.enable` pair with a `vim.lsp.start` call inside a
`FileType marie` autocommand.
