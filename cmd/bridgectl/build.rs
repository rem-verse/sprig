//! Most of this build script comes from [NEARD](https://github.com/near/nearcore)
//!
//! Which is licensed under APACHE2/MIT at the time of copying.

use std::{
	io::{Error as IoError, ErrorKind as IoErrorKind},
	os::unix::ffi::OsStringExt,
};

fn env(key: &str) -> Result<std::ffi::OsString, String> {
	println!("cargo:rerun-if-env-changed={key}");
	std::env::var_os(key).ok_or_else(|| format!("missing `{key}` environment variable"))
}

/// Calls program with given arguments and returns its standard output.  If
/// calling the program fails or it exits with non-zero exit status returns an
/// error.
fn command(prog: &str, args: &[&str], cwd: Option<std::path::PathBuf>) -> Result<Vec<u8>, IoError> {
	println!("cargo:rerun-if-env-changed=PATH");
	let mut command = std::process::Command::new(prog);
	command.args(args);
	command.stderr(std::process::Stdio::inherit());
	if let Some(cwd) = cwd {
		command.current_dir(cwd);
	}
	let out = command.output()?;
	if out.status.success() {
		let mut stdout = out.stdout;
		if let Some(b'\n') = stdout.last() {
			stdout.pop();
			if let Some(b'\r') = stdout.last() {
				stdout.pop();
			}
		}
		Ok(stdout)
	} else if let Some(code) = out.status.code() {
		Err(IoError::new(
			IoErrorKind::Other,
			format!("{prog}: terminated with {code}"),
		))
	} else {
		Err(IoError::new(
			IoErrorKind::Other,
			format!("{prog}: killed by signal"),
		))
	}
}

/// Expose the git version to bridgectl so it can print it out!
fn main() {
	// Figure out git directory.  Don’t just assume it’s ../.git because that
	// doesn’t work with git work trees so use `git rev-parse --git-dir` instead.
	let pkg_dir =
		std::path::PathBuf::from(env("CARGO_MANIFEST_DIR").expect("need manifest directory"));
	let git_dir = command("git", &["rev-parse", "--git-dir"], Some(pkg_dir));
	let git_dir = match git_dir {
		Ok(git_dir) => std::path::PathBuf::from(std::ffi::OsString::from_vec(git_dir)),
		Err(msg) => {
			// We’re probably not inside of a git repository so report git
			// version as unknown.
			println!("cargo:warning=unable to determine git version (not in git repository?)");
			println!("cargo:warning={msg}");
			println!("cargo:rustc-env=BRIDGECTL_BUILD=unknown");
			return;
		}
	};

	// Make Cargo rerun us if currently checked out commit or the state of the
	// working tree changes.  We try to accomplish that by looking at a few
	// crucial git state files.  This probably may result in some false
	// negatives but it’s best we’ve got.
	for subpath in ["HEAD", "logs/HEAD", "index"] {
		let path = git_dir
			.join(subpath)
			.canonicalize()
			.expect("Failed to get canonical path to git directory");
		println!("cargo:rerun-if-changed={}", path.display());
	}

	// * --always → if there is no matching tag, use commit hash
	// * --dirty=-sussy → append ‘-sussy’ if there are local changes
	// * --tags → consider tags even if they are unannotated
	// * --match=[0-9]* → only consider tags starting with a digit; this
	//   prevents tags such as `crates-0.14.0` from being considered
	let args = &[
		"describe",
		"--always",
		"--dirty=-sussy",
		"--tags",
		"--match=[0-9]*",
	];
	let out = command("git", args, None).expect("Failed to run git describe!");
	let git_version = match String::from_utf8_lossy(&out) {
		std::borrow::Cow::Borrowed(version) => version.trim().to_string(),
		std::borrow::Cow::Owned(version) => panic!("git: invalid output: {version}"),
	};

	println!("cargo:rustc-env=BRIDGECTL_BUILD={git_version}");
}
