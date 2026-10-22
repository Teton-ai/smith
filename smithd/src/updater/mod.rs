mod actor;
mod handler;

pub use handler::Handler as UpdaterHandle;

/// `-f` lets apt finish packages that a failed install left unpacked, which
/// otherwise blocks every later install. apt removes an installed package when
/// a deb conflicts with it, or when `-f` cannot fix the package's dependencies.
pub fn apt_install<'a>(
    debs: impl IntoIterator<Item = &'a std::path::Path>,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("sudo");
    command
        .args(["apt", "install", "-y", "-f", "--allow-downgrades"])
        .args(debs);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn apt_install_fixes_broken_state() {
        let command = apt_install([Path::new("/blobs/a.deb"), Path::new("/blobs/b.deb")]);

        let program = command.as_std().get_program();
        let args = command.as_std().get_args().collect::<Vec<_>>();

        assert_eq!(program, "sudo");
        assert_eq!(
            args,
            [
                "apt",
                "install",
                "-y",
                "-f",
                "--allow-downgrades",
                "/blobs/a.deb",
                "/blobs/b.deb"
            ]
        );
    }
}
