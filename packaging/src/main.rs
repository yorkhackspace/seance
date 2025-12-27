use std::{collections::HashMap, fmt::Display, path::PathBuf};

fn main() {
    let project_root = PathBuf::from("./");
    let packaging_working_directory = project_root.join("packing-staging-temp");
    create_build_directory(&packaging_working_directory);
    let packaging_target_directory = project_root.join("packaged");
    create_build_directory(&packaging_target_directory);

    let mut packaging_targets = vec![
        (
            TargetDistributionFamily::DebianArm64,
            BuiltBinary::Planchette,
        ),
        (
            TargetDistributionFamily::DebianArmv6l,
            BuiltBinary::Planchette,
        ),
        (
            TargetDistributionFamily::DebianX86_64,
            BuiltBinary::Planchette,
        ),
        (
            TargetDistributionFamily::DebianX86_64,
            BuiltBinary::SeanceApp,
        ),
        (TargetDistributionFamily::ArchX86_64, BuiltBinary::SeanceApp),
        (
            TargetDistributionFamily::WindowsX86_64,
            BuiltBinary::SeanceApp,
        ),
    ]
    .into_iter()
    .fold(
        HashMap::<BuildTarget, Vec<(TargetDistributionFamily, BuiltBinary)>>::new(),
        |mut acc, (target, binary)| {
            let entry = acc.entry(target.build_target()).or_default();

            entry.push((target, binary));

            acc
        },
    )
    .drain()
    .collect::<Vec<_>>();
    packaging_targets.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (build_target, to_distribute) in packaging_targets {
        println!("Building {build_target}");
        build_all_binaries(build_target);
        println!("Built {build_target}");
        for (target_distribution, binary) in to_distribute {
            println!("Packaging {binary} for {target_distribution}");
            target_distribution.package(
                &project_root,
                &packaging_working_directory,
                &packaging_target_directory,
                binary,
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TargetDistributionFamily {
    ArchX86_64,
    DebianArm64,
    DebianArmv6l,
    DebianX86_64,
    WindowsX86_64,
}

impl TargetDistributionFamily {
    fn build_target(self) -> BuildTarget {
        match self {
            TargetDistributionFamily::ArchX86_64 => BuildTarget::LinuxX86_64,
            TargetDistributionFamily::DebianArm64 => BuildTarget::LinuxAarch64,
            TargetDistributionFamily::DebianArmv6l => BuildTarget::LinuxArmv6l,
            TargetDistributionFamily::DebianX86_64 => BuildTarget::LinuxX86_64,
            TargetDistributionFamily::WindowsX86_64 => BuildTarget::WindowsX86_64,
        }
    }

    fn package(
        self,
        project_root: &std::path::Path,
        packaging_working_directory: &std::path::Path,
        packaging_target_directory: &std::path::Path,
        binary: BuiltBinary,
    ) {
        let binary_file_name = binary.name_as_built(self.build_target());
        let binary_path = PathBuf::from("./result/bin").join(binary_file_name);
        match self {
            TargetDistributionFamily::ArchX86_64 => match binary {
                BuiltBinary::Planchette => {
                    panic!("Packaging Planchette for Arch (x86_64) is not supported!")
                }
                BuiltBinary::SeanceApp => package_seance_arch_x86_64(),
            },
            TargetDistributionFamily::DebianArm64 => match binary {
                BuiltBinary::Planchette => package_planchette_debian_aarch64(
                    &project_root,
                    packaging_working_directory,
                    packaging_target_directory,
                    &binary_path,
                ),
                BuiltBinary::SeanceApp => {
                    panic!("Packaging Seance for Debian (aarch64) is not supported!")
                }
            },
            TargetDistributionFamily::DebianArmv6l => match binary {
                BuiltBinary::Planchette => package_planchette_debian_armv6l(
                    &project_root,
                    packaging_working_directory,
                    packaging_target_directory,
                    &binary_path,
                ),
                BuiltBinary::SeanceApp => {
                    panic!("Packaging Seance for Debian (armv6l) is not supported!")
                }
            },
            TargetDistributionFamily::DebianX86_64 => match binary {
                BuiltBinary::Planchette => package_planchette_debian_x86_64(
                    &project_root,
                    packaging_working_directory,
                    packaging_target_directory,
                    &binary_path,
                ),
                BuiltBinary::SeanceApp => package_seance_debian_x86_64(),
            },
            TargetDistributionFamily::WindowsX86_64 => match binary {
                BuiltBinary::Planchette => {
                    panic!("Packaging Planchette for Windows (x86_64) is not supported!")
                }
                BuiltBinary::SeanceApp => package_seance_windows_x86_64(),
            },
        }
    }
}

impl Display for TargetDistributionFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetDistributionFamily::ArchX86_64 => write!(f, "Arch (x86_64)"),
            TargetDistributionFamily::DebianArm64 => write!(f, "Debian (arm64)"),
            TargetDistributionFamily::DebianArmv6l => write!(f, "Debian (armhf)"),
            TargetDistributionFamily::DebianX86_64 => write!(f, "Debian (x86_64)"),
            TargetDistributionFamily::WindowsX86_64 => write!(f, "Windows (x86_64)"),
        }
    }
}

fn package_seance_arch_x86_64() {
    // TODO
}

fn package_planchette_debian_aarch64(
    project_root: &std::path::Path,
    packaging_working_directory: &std::path::Path,
    packaging_target_directory: &std::path::Path,
    built_binary_path: &std::path::Path,
) {
    let working_directory = packaging_working_directory.join("planchette-debian-arm64");
    let deb_working_directory = working_directory.join("planchette-deb");

    copy_dir_all(&PathBuf::from("./planchette-deb"), &deb_working_directory)
        .expect("Failed to copy Debian packaging directory");

    let usr_bin_path = deb_working_directory.join("usr/bin");
    std::fs::create_dir_all(&usr_bin_path)
        .expect("Failed to create usr/bin in debian packaging directory");

    let binary_target_path = usr_bin_path.join("plancette");
    std::fs::copy(built_binary_path, &binary_target_path)
        .expect("Failed to copy planchette binary to packaging directory");

    let chmod_output = std::process::Command::new("chmod")
        .arg("755")
        .arg(&binary_target_path)
        .output()
        .expect("Failed to chmod Planchette binary");
    handle_shelled_output(chmod_output, "chmod");

    std::fs::copy(
        project_root.join("planchette/deb-control-arm64"),
        deb_working_directory.join("DEBIAN/control"),
    )
    .expect("Failed to copy deb-control-arm64 to DEBIAN/control");

    let dpkg_deb_output = std::process::Command::new("dpkg-deb")
        .arg("--root-owner-group")
        .arg("--build")
        .arg(deb_working_directory)
        // TODO: would be nice to add the version to the path here.
        .arg(packaging_target_directory.join("planchette-arm64.deb"))
        .output()
        .expect("Failed to run dpkg-deb for planchette");
    handle_shelled_output(dpkg_deb_output, "dpkg-deb");
}

fn package_planchette_debian_armv6l(
    project_root: &std::path::Path,
    packaging_working_directory: &std::path::Path,
    packaging_target_directory: &std::path::Path,
    built_binary_path: &std::path::Path,
) {
    let working_directory = packaging_working_directory.join("planchette-debian-armv6l");
    let deb_working_directory = working_directory.join("planchette-deb");

    copy_dir_all(&PathBuf::from("./planchette-deb"), &deb_working_directory)
        .expect("Failed to copy Debian packaging directory");

    let usr_bin_path = deb_working_directory.join("usr/bin");
    std::fs::create_dir_all(&usr_bin_path)
        .expect("Failed to create usr/bin in debian packaging directory");

    let binary_target_path = usr_bin_path.join("plancette");
    std::fs::copy(built_binary_path, &binary_target_path)
        .expect("Failed to copy planchette binary to packaging directory");

    let chmod_output = std::process::Command::new("chmod")
        .arg("755")
        .arg(&binary_target_path)
        .output()
        .expect("Failed to chmod Planchette binary");
    handle_shelled_output(chmod_output, "chmod");

    std::fs::copy(
        project_root.join("planchette/deb-control-armhf"),
        deb_working_directory.join("DEBIAN/control"),
    )
    .expect("Failed to copy deb-control-armhf to DEBIAN/control");

    let dpkg_deb_output = std::process::Command::new("dpkg-deb")
        .arg("--root-owner-group")
        .arg("--build")
        .arg(deb_working_directory)
        // TODO: would be nice to add the version to the path here.
        .arg(packaging_target_directory.join("planchette-armhf.deb"))
        .output()
        .expect("Failed to run dpkg-deb for planchette");
    handle_shelled_output(dpkg_deb_output, "dpkg-deb");
}

fn package_planchette_debian_x86_64(
    project_root: &std::path::Path,
    packaging_working_directory: &std::path::Path,
    packaging_target_directory: &std::path::Path,
    built_binary_path: &std::path::Path,
) {
    let working_directory = packaging_working_directory.join("planchette-debiaa-x86_64");
    let deb_working_directory = working_directory.join("planchette-deb");

    copy_dir_all(&PathBuf::from("./planchette-deb"), &deb_working_directory)
        .expect("Failed to copy Debian packaging directory");

    let usr_bin_path = deb_working_directory.join("usr/bin");
    std::fs::create_dir_all(&usr_bin_path)
        .expect("Failed to create usr/bin in debian packaging directory");

    let binary_target_path = usr_bin_path.join("plancette");
    std::fs::copy(built_binary_path, &binary_target_path)
        .expect("Failed to copy planchette binary to packaging directory");

    let chmod_output = std::process::Command::new("chmod")
        .arg("755")
        .arg(&binary_target_path)
        .output()
        .expect("Failed to chmod Planchette binary");
    handle_shelled_output(chmod_output, "chmod");

    std::fs::copy(
        project_root.join("planchette/deb-control-x86_64"),
        deb_working_directory.join("DEBIAN/control"),
    )
    .expect("Failed to copy deb-control-x86_64 to DEBIAN/control");

    let dpkg_deb_output = std::process::Command::new("dpkg-deb")
        .arg("--root-owner-group")
        .arg("--build")
        .arg(deb_working_directory)
        // TODO: would be nice to add the version to the path here.
        .arg(packaging_target_directory.join("planchette-amd64.deb"))
        .output()
        .expect("Failed to run dpkg-deb for planchette");
    handle_shelled_output(dpkg_deb_output, "dpkg-deb");
}

fn package_seance_debian_x86_64() {
    // TODO
}

fn package_seance_windows_x86_64() {
    // TODO
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum BuildTarget {
    LinuxAarch64,
    LinuxArmv6l,
    LinuxX86_64,
    WindowsX86_64,
}

impl BuildTarget {
    fn arch_str(self) -> &'static str {
        match self {
            BuildTarget::LinuxAarch64 => "aarch64-linux",
            BuildTarget::LinuxArmv6l => "armv6l-linux",
            BuildTarget::LinuxX86_64 => "x86_64-linux",
            BuildTarget::WindowsX86_64 => "x86_64-windows",
        }
    }
}

impl Display for BuildTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildTarget::LinuxAarch64 => write!(f, "Linux (aarch64)"),
            BuildTarget::LinuxArmv6l => write!(f, "Linux (armhf)"),
            BuildTarget::LinuxX86_64 => write!(f, "Linux (x86_64)"),
            BuildTarget::WindowsX86_64 => write!(f, "Windows (x86_64)"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum BuiltBinary {
    Planchette,
    SeanceApp,
}

impl BuiltBinary {
    fn name_as_built(self, build_target: BuildTarget) -> &'static str {
        match self {
            BuiltBinary::Planchette => match build_target {
                BuildTarget::LinuxAarch64 | BuildTarget::LinuxArmv6l | BuildTarget::LinuxX86_64 => {
                    "planchette"
                }
                BuildTarget::WindowsX86_64 => "planchette.exe",
            },
            BuiltBinary::SeanceApp => match build_target {
                BuildTarget::LinuxAarch64 | BuildTarget::LinuxArmv6l | BuildTarget::LinuxX86_64 => {
                    "seance-app"
                }
                BuildTarget::WindowsX86_64 => "seance-app.exe",
            },
        }
    }
}

impl Display for BuiltBinary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuiltBinary::Planchette => write!(f, "Planchette"),
            BuiltBinary::SeanceApp => write!(f, "Seance"),
        }
    }
}

fn build_all_binaries(target_arch: BuildTarget) {
    let cross_target_str = format!(".#cross-{}", target_arch.arch_str());

    let command_output = match target_arch {
        BuildTarget::LinuxAarch64 | BuildTarget::LinuxArmv6l | BuildTarget::LinuxX86_64 => {
            std::process::Command::new("nix")
                .arg("build")
                .arg(cross_target_str)
                .output()
                .expect("Failed to run nix build")
        }
        BuildTarget::WindowsX86_64 => std::process::Command::new("nix")
            .arg("build")
            .arg("--impure")
            .arg(cross_target_str)
            .env("NIXPKGS_ALLOW_UNSUPPORTED_SYSTEM", "1")
            .output()
            .expect("Failed to run nix build"),
    };
    handle_shelled_output(command_output, "nix build");
}

fn create_build_directory(directory: &std::path::Path) {
    // Yeah yeah TOCTOU it's fiiiiiiine.
    if matches!(std::fs::exists(directory), Ok(true)) {
        std::fs::remove_dir_all(directory).expect("Failed to remove build directory");
    }
    std::fs::create_dir_all(directory).expect("Failed to create build directory");
}

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn handle_shelled_output(output: std::process::Output, cmd_name: &str) {
    let stdout = String::from_utf8(output.stdout);
    if let Ok(stdout) = &stdout {
        println!("{stdout}");
    }
    let stderr = String::from_utf8(output.stderr);
    if let Ok(stderr) = &stderr {
        eprintln!("{stderr}");
    }
    if !output.status.success() {
        let stderr = stderr.unwrap_or("unknown error".to_string());
        panic!("{cmd_name} failed: {stderr}")
    }
}
