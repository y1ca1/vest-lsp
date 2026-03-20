use zed_extension_api as zed;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const WORKSPACE_MANIFEST_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");

struct VestExtension;

impl zed::Extension for VestExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let settings =
            zed::settings::LspSettings::for_worktree("Vest LSP", worktree).unwrap_or_default();

        if let Some(binary) = settings.binary {
            if let Some(path) = binary.path {
                let mut command = zed::Command::new(path);
                if let Some(args) = binary.arguments {
                    command = command.args(args);
                }
                if let Some(env) = binary.env {
                    command = command.envs(env);
                }
                return Ok(command);
            }
        }

        if let Some(binary) = worktree.which("vest_lsp") {
            return Ok(zed::Command::new(binary).envs(worktree.shell_env()));
        }

        if worktree.which("cargo").is_some() {
            return Ok(zed::Command::new(worktree.which("cargo").unwrap())
                .args([
                    "run",
                    "--quiet",
                    "--package",
                    "vest_lsp",
                    "--bin",
                    "vest_lsp",
                    "--manifest-path",
                    WORKSPACE_MANIFEST_PATH,
                ])
                .env("CARGO_MANIFEST_DIR", REPO_ROOT)
                .envs(worktree.shell_env()));
        }

        Err("could not find `vest_lsp` or `cargo` in the Zed shell environment".into())
    }
}

zed::register_extension!(VestExtension);
