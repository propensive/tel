use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

struct TelExtension;

impl zed::Extension for TelExtension {
    fn new() -> Self {
        TelExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // The server is the Ethereal native launcher `tel`, installed on the PATH by `make install`
        // in the `lsp/` directory. `tel lsp` runs the language server over stdio.
        let command = worktree.which("tel").ok_or_else(|| {
            "`tel` was not found on your PATH. Build and install it with `make install` in the lsp/ \
             directory (it is copied to ~/.local/bin), and make sure that directory is on the PATH \
             that Zed inherits."
                .to_string()
        })?;

        Ok(Command {
            command,
            args: vec!["lsp".to_string()],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(TelExtension);
