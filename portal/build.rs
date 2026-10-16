use std::{env, fs, io, path::Path, process::Command};

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_EMBED_CONSOLE").is_none() {
        return Ok(());
    }

    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("CARGO_MANIFEST_DIR is missing"))?;
    let output = env::var_os("OUT_DIR")
        .ok_or_else(|| io::Error::other("OUT_DIR is missing"))?;
    let source = Path::new(&manifest).join("console");
    let output = Path::new(&output);
    let console = output.join("console");

    // Keep generated files outside the source tree and discard stale assets.
    if console.exists() {
        fs::remove_dir_all(&console)?;
    }
    fs::create_dir_all(&console)?;
    for name in ["package.json", "package-lock.json", "build.mjs", "index.html", "src"] {
        println!("cargo::rerun-if-changed=console/{name}");
        copy(&source.join(name), &console.join(name))?;
    }

    npm(&console, &output.join("npm-cache"), &["ci", "--include=dev", "--no-audit", "--no-fund"])?;
    npm(&console, &output.join("npm-cache"), &["run", "build"])?;

    if !console.join("dist/index.html").is_file() {
        return Err(io::Error::other("console build did not produce index.html"));
    }
    Ok(())
}

fn copy(source: &Path, destination: &Path) -> io::Result<()> {
    let kind = fs::symlink_metadata(source)?.file_type();
    if kind.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if kind.is_file() {
        fs::copy(source, destination)?;
    } else {
        return Err(io::Error::other(format!("unsupported console source: {}", source.display())));
    }
    Ok(())
}

fn npm(directory: &Path, cache: &Path, args: &[&str]) -> io::Result<()> {
    let executable = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(executable)
        .args(args)
        .current_dir(directory)
        .env("npm_config_cache", cache)
        .status()
        .map_err(|error| {
            io::Error::new(error.kind(), format!("cannot run npm; install Node.js and npm: {error}"))
        })?;
    if !status.success() {
        return Err(io::Error::other(format!("npm {} failed: {status}", args.join(" "))));
    }
    Ok(())
}
