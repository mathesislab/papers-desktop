use std::{env, fs, path::Path};

/// What the placeholder of the papers-rs sidecar says; the app knows it by
/// this marker (`cli_tool::is_placeholder`).
const PLACEHOLDER: &str = "#!/bin/sh\n# papers-rs-placeholder\necho 'papers-rs: not built; run scripts/sidecar.sh' >&2\nexit 1\n";

fn main() {
    // tauri-build wants the sidecar (bundle.externalBin) to exist, also for
    // `cargo check` and `cargo test`: scripts/sidecar.sh puts the real one,
    // before a build of the app; a placeholder stands in until then
    let triple = env::var("TARGET").expect("TARGET");
    let sidecar = Path::new("binaries").join(format!("papers-rs-cli-{triple}"));
    if !sidecar.exists() {
        fs::create_dir_all("binaries").expect("binaries/");
        fs::write(&sidecar, PLACEHOLDER).expect("the placeholder of the sidecar");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&sidecar, fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }
    tauri_build::build()
}
