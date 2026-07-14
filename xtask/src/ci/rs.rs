use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Subcommand};
use hermit_entry::config;

use crate::cargo_build::CargoBuild;

/// Work with hermit-rs images
#[derive(Args)]
pub struct Rs {
	#[command(flatten)]
	pub cargo_build: CargoBuild,

	/// Package to build (see `cargo help pkgid`)
	#[arg(short, long, id = "SPEC")]
	pub package: String,

	/// Create multiple vCPUs.
	#[arg(long, default_value_t = 1)]
	pub smp: usize,

	#[command(subcommand)]
	action: Action,
}

#[derive(Subcommand)]
pub enum Action {
	/// Build image.
	Build,
	Firecracker(super::firecracker::Firecracker),
	Qemu(super::qemu::Qemu),
	Uhyve(super::uhyve::Uhyve),
}

impl Rs {
	pub fn run(mut self) -> Result<()> {
		let image = self.build()?;

		let arch = self.cargo_build.artifact.arch;
		let small = self.cargo_build.artifact.profile() == "release";
		match self.action {
			Action::Build => Ok(()),
			Action::Firecracker(firecracker) => firecracker.run(&image, self.smp),
			Action::Qemu(qemu) => {
				qemu.run(&image, &self.cargo_build.features, self.smp, arch, small)
			}
			Action::Uhyve(uhyve) => uhyve.run(&image, self.smp),
		}
	}

	pub fn build(&mut self) -> Result<PathBuf> {
		if super::in_ci() {
			eprintln!("::group::cargo build");
		}

		if self.smp > 1 {
			self.cargo_build.features.push("hermit/smp".to_owned());
		}

		let mut cargo = crate::cargo();
		let parent_root = super::parent_root();

		if self.package.contains("rftrace") {
			cargo.env(
				"RUSTFLAGS",
				"-Zinstrument-mcount -Cpasses=ee-instrument<post-inline>",
			);
		};

		cargo
			.current_dir(parent_root)
			.arg("build")
			.args(self.cargo_build.artifact.arch.ci_cargo_args())
			.args(self.cargo_build.cargo_build_args())
			.args(["--package", self.package.as_str()]);

		eprintln!("$ {cargo:?}");
		let status = cargo.status()?;
		assert!(status.success());

		// discover Hermit Images case
		let manifest_dir = {
			let cur_package = cargo_metadata::PackageName::new(self.package.clone());
			let mut cargo = cargo_metadata::MetadataCommand::new();
			cargo
				.current_dir(parent_root)
				.no_deps()
				.verbose(true)
				.exec()?
				.packages
				.iter()
				.find(|i| i.name == cur_package)
				.expect("unable to find current package in `cargo metadata` output")
				// this path points to `Cargo.toml`
				.manifest_path
				.parent()
				.unwrap()
				.to_path_buf()
		};
		eprintln!("MANIFEST_DIR = {manifest_dir}");
		let hermit_image_root = {
			let image_root = manifest_dir.join("image-root");
			if image_root.is_dir() {
				Some(image_root)
			} else {
				None
			}
		};

		let mut build_artifact = self.cargo_build.artifact.ci_image(&self.package);

		// handle Hermit Images
		if let Some(image_root) = hermit_image_root {
			eprintln!("discovered image-root, creating Hermit Image.");
			// find kernel name
			let konfig = fs::read_to_string(image_root.join(config::Config::DEFAULT_PATH))?;
			let konfig: config::Config<'_> = toml::from_str(&konfig)?;
			let kernel_name: &str = match &konfig {
				config::Config::V1 { kernel, .. } => kernel,
			};

			let tar_artifact_path = build_artifact.with_extension("tar.gz");
			let mut tar_artifact = tar::Builder::new(flate2::write::GzEncoder::new(
				fs::File::create(&tar_artifact_path)?,
				flate2::Compression::default(),
			));
			tar_artifact.mode(tar::HeaderMode::Deterministic);

			// NOTE: use tar ustar to create the image.

			// add kernel
			eprintln!("- {kernel_name}");
			{
				let mut header = tar::Header::new_ustar();
				let kernel_meta = fs::metadata(&build_artifact)?;
				header.set_path(kernel_name).unwrap();
				header.set_size(kernel_meta.len());
				header.set_cksum();

				tar_artifact.append(&header, fs::File::open(&build_artifact)?)?;
			}

			// add rest
			for entry in walkdir::WalkDir::new(&image_root) {
				let entry = entry?;
				let entry_rel_path = entry.path().strip_prefix(&image_root)?;
				eprintln!("- {}", entry_rel_path.display());
				if entry_rel_path == Path::new(kernel_name) || entry.metadata()?.is_dir() {
					continue;
				}

				tar_artifact.append_path_with_name(entry.path(), entry_rel_path)?;
			}

			tar_artifact.into_inner()?.finish()?.sync_all()?;
			build_artifact = tar_artifact_path;
		}

		if super::in_ci() {
			eprintln!("::endgroup::");
		}

		Ok(build_artifact)
	}
}
