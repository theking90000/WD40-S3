use std::{env, io, path::Path, process::Command};

fn main() -> io::Result<()> {
    println!("cargo::rerun-if-changed=build.rs");
    if env::var_os("CARGO_FEATURE_EMBED_CONSOLE").is_none() {
        return Ok(());
    }

    println!("cargo::rerun-if-env-changed=WD40_PORTAL_BASE_PATH");
    let basename = match env::var("WD40_PORTAL_BASE_PATH") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => "/".to_owned(),
        Err(error) => return Err(io::Error::other(error)),
    };
    if !basename.starts_with('/')
        || !basename.ends_with('/')
        || basename.contains("//")
        || basename.contains(['?', '#', '\\', '\n', '\r', '{', '}', '*', ':'])
        || basename.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(io::Error::other(
            "WD40_PORTAL_BASE_PATH must be an absolute URL path ending with /",
        ));
    }
    println!("cargo::rustc-env=WD40_PORTAL_BASE_PATH={basename}");

    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .ok_or_else(|| io::Error::other("CARGO_MANIFEST_DIR is missing"))?;
    let output = env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR is missing"))?;
    let console = Path::new(&manifest).join("console");
    for name in [
        "package.json",
        "package-lock.json",
        "vite.config.ts",
        "react-router.config.ts",
        "tsconfig.json",
        "src",
    ] {
        println!("cargo::rerun-if-changed=console/{name}");
    }

    let executable = if cfg!(windows) { "npm.cmd" } else { "npm" };
    for args in [
        &["ci", "--include=dev", "--no-audit", "--no-fund"][..],
        &["run", "build"][..],
    ] {
        let status = Command::new(executable)
            .args(args)
            .current_dir(&console)
            .env("npm_config_cache", Path::new(&output).join("npm-cache"))
            .status()
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot run npm; install Node.js and npm: {error}"),
                )
            })?;
        if !status.success() {
            return Err(io::Error::other(format!(
                "npm {} failed: {status}",
                args.join(" ")
            )));
        }
    }
    Ok(())
}
