use std::path::PathBuf;
use std::process::Command;

pub(crate) fn package_dir() -> PathBuf {
    PathBuf::from(crate::domain::config::get_downloads_dir()).join(".yt-dlp-python")
}

pub(crate) fn python_with_pip() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TVSERVER_PYTHON") {
        return choose_python_with_pip([PathBuf::from(explicit)]);
    }
    let path = std::env::var_os("PATH")?;
    choose_python_with_pip(std::env::split_paths(&path).map(|dir| dir.join("python3")))
}

fn choose_python_with_pip(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| {
        candidate.is_file()
            && Command::new(candidate)
                .args(["-m", "pip", "--version"])
                .output()
                .is_ok_and(|output| output.status.success())
    })
}

pub(crate) const RUN_YTDLP: &str =
    "import sys; sys.path.insert(0, sys.argv.pop(1)); from yt_dlp import main; main()";

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn chooses_a_python_interpreter_that_can_import_pip() {
        let root = std::env::temp_dir().join(format!("tvserver-python-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let broken = root.join("broken-python3");
        let working = root.join("working-python3");
        std::fs::write(&broken, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::write(&working, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        for path in [&broken, &working] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(
            choose_python_with_pip(vec![broken, working.clone()]),
            Some(working)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
