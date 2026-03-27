use zed_extension_api as zed;

const GITHUB_REPO: &str = "y1ca1/vest-lsp";
const CACHED_BINARY_DIR: &str = "vest-lsp";
const DEV_WORKSPACE_MANIFEST: &str = "Cargo.toml";
const DEV_SERVER_MANIFEST: &str = "vest_lsp/Cargo.toml";
const RELEASE_TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(Default)]
struct VestExtension {
    cached_binary_path: Option<String>,
}

impl zed::Extension for VestExtension {
    fn new() -> Self {
        Self::default()
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
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

        if let Some(path) = self.cached_binary_path.clone() {
            if binary_is_available(&path) {
                return Ok(zed::Command::new(path).envs(worktree.shell_env()));
            }

            self.cached_binary_path = None;
        }

        if let Some(path) = self.ensure_downloaded_binary(language_server_id)? {
            self.cached_binary_path = Some(path.clone());
            return Ok(zed::Command::new(path).envs(worktree.shell_env()));
        }

        if let Some(binary) = worktree.which("vest_lsp") {
            return Ok(zed::Command::new(binary).envs(worktree.shell_env()));
        }

        if let Some(command) = self.dev_workspace_command(worktree) {
            return Ok(command);
        }

        Err(
            "could not find `vest_lsp`, download a published release binary, or locate a local Vest workspace"
                .into(),
        )
    }
}

impl VestExtension {
    fn ensure_downloaded_binary(
        &self,
        language_server_id: &zed::LanguageServerId,
    ) -> zed::Result<Option<String>> {
        let binary_path = cached_binary_path();
        if binary_is_available(&binary_path) {
            return Ok(Some(binary_path));
        }

        let Some(asset_name) = release_asset_name() else {
            return Ok(None);
        };

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = match zed::github_release_by_tag_name(
            GITHUB_REPO,
            RELEASE_TAG,
        ) {
            Ok(release) => release,
            Err(_) => {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::None,
                );
                return Ok(None);
            }
        };

        let Some(asset) = release.assets.into_iter().find(|asset| asset.name == asset_name) else {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::None,
            );
            return Ok(None);
        };

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::Downloading,
        );

        let download_result =
            zed::download_file(&asset.download_url, &binary_path, zed::DownloadedFileType::Gzip);

        if let Err(error) = download_result {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Failed(error.clone()),
            );
            return Ok(None);
        }

        if !matches!(zed::current_platform(), (zed::Os::Windows, _)) {
            if let Err(error) = zed::make_file_executable(&binary_path) {
                zed::set_language_server_installation_status(
                    language_server_id,
                    &zed::LanguageServerInstallationStatus::Failed(error),
                );
                return Ok(None);
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::None,
        );

        Ok(Some(binary_path))
    }

    fn dev_workspace_command(&self, worktree: &zed::Worktree) -> Option<zed::Command> {
        let cargo = worktree.which("cargo")?;

        // Only use the workspace fallback when the opened worktree is actually a vest-lsp checkout.
        worktree.read_text_file(DEV_WORKSPACE_MANIFEST).ok()?;
        worktree.read_text_file(DEV_SERVER_MANIFEST).ok()?;

        let manifest_path = format!("{}/{}", worktree.root_path(), DEV_WORKSPACE_MANIFEST);

        Some(
            zed::Command::new(cargo)
                .args([
                    "run",
                    "--quiet",
                    "--package",
                    "vest_lsp",
                    "--bin",
                    "vest_lsp",
                    "--manifest-path",
                    &manifest_path,
                ])
                .envs(worktree.shell_env()),
        )
    }
}

fn cached_binary_path() -> String {
    let binary_name = if matches!(zed::current_platform(), (zed::Os::Windows, _)) {
        "vest_lsp.exe"
    } else {
        "vest_lsp"
    };

    format!("{CACHED_BINARY_DIR}/{}/{}", env!("CARGO_PKG_VERSION"), binary_name)
}

fn binary_is_available(binary_path: &str) -> bool {
    zed::Command::new(binary_path).arg("--version").output().is_ok()
}

fn release_asset_name() -> Option<&'static str> {
    match zed::current_platform() {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Some("vest_lsp-mac-aarch64.gz"),
        (zed::Os::Mac, zed::Architecture::X8664) => Some("vest_lsp-mac-x8664.gz"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Some("vest_lsp-linux-aarch64.gz"),
        (zed::Os::Linux, zed::Architecture::X8664) => Some("vest_lsp-linux-x8664.gz"),
        (zed::Os::Windows, zed::Architecture::Aarch64) => Some("vest_lsp-windows-aarch64.gz"),
        (zed::Os::Windows, zed::Architecture::X8664) => Some("vest_lsp-windows-x8664.gz"),
        _ => None,
    }
}

zed::register_extension!(VestExtension);
