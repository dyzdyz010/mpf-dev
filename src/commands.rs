use anyhow::{bail, Context, Result};
use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{
    self, ComponentConfig, ComponentMode, DevConfig, KNOWN_COMPONENTS,
};

const GITHUB_REPO: &str = "dyzdyz010/mpf-release";

/// Setup command: download and install SDK
pub async fn setup(version: Option<String>) -> Result<()> {
    println!("{}", "MPF SDK Setup".bold().cyan());
    
    let version = match version {
        Some(v) => v,
        None => {
            println!("Fetching latest release...");
            fetch_latest_version().await?
        }
    };
    
    let version_normalized = if version.starts_with('v') {
        version.clone()
    } else {
        format!("v{}", version)
    };
    
    println!("Installing SDK version: {}", version_normalized.green());
    
    let sdk_root = config::sdk_root();
    let version_dir = config::version_dir(&version_normalized);
    
    // Check if already installed
    if version_dir.exists() {
        println!(
            "{} Version {} is already installed",
            "Note:".yellow(),
            version_normalized
        );
    } else {
        // Download and extract
        download_and_extract(&version_normalized, &version_dir).await?;
    }
    
    // Set as current
    let current_link = config::current_link();
    if current_link.exists() || current_link.is_symlink() {
        fs::remove_file(&current_link)?;
    }
    
    #[cfg(unix)]
    std::os::unix::fs::symlink(&version_dir, &current_link)?;
    
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&version_dir, &current_link)?;
    
    // Update dev.json
    let mut config = DevConfig::load().unwrap_or_default();
    config.sdk_version = Some(version_normalized.clone());
    config.save()?;
    
    println!(
        "{} SDK {} installed and set as current",
        "✓".green(),
        version_normalized
    );
    println!("  Location: {}", sdk_root.display());
    
    Ok(())
}

async fn fetch_latest_version() -> Result<String> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPO
    );
    
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "mpf-dev")
        .send()
        .await?
        .json::<serde_json::Value>()
        .await?;
    
    resp["tag_name"]
        .as_str()
        .map(|s| s.to_string())
        .context("Could not find latest release")
}

async fn download_and_extract(version: &str, dest: &PathBuf) -> Result<()> {
    // Determine platform and asset name
    let (asset_name, is_tarball) = if cfg!(target_os = "windows") {
        ("mpf-windows-x64.zip".to_string(), false)
    } else {
        ("mpf-linux-x64.tar.gz".to_string(), true)
    };
    
    let download_url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        GITHUB_REPO, version, asset_name
    );
    
    println!("Downloading {} ({})...", asset_name, version);
    
    let client = reqwest::Client::new();
    let resp = client
        .get(&download_url)
        .header("User-Agent", "mpf-dev")
        .send()
        .await?;
    
    if !resp.status().is_success() {
        bail!(
            "Failed to download SDK: {} ({})",
            resp.status(),
            download_url
        );
    }
    
    let total_size = resp.content_length().unwrap_or(0);
    
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")?
            .progress_chars("#>-"),
    );
    
    // Download to temp file
    let temp_ext = if is_tarball { "tar.gz.tmp" } else { "zip.tmp" };
    let temp_path = dest.with_extension(temp_ext);
    if let Some(parent) = temp_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let mut file = File::create(&temp_path)?;
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        pb.set_position(downloaded);
    }
    
    pb.finish_with_message("Downloaded");
    
    // Extract
    println!("Extracting...");
    fs::create_dir_all(dest)?;
    
    if is_tarball {
        // Extract tar.gz using tar command (more reliable on Unix)
        let status = Command::new("tar")
            .args(["-xzf", &temp_path.to_string_lossy(), "-C", &dest.to_string_lossy()])
            .status()
            .context("Failed to run tar command")?;
        if !status.success() {
            bail!("tar extraction failed");
        }
    } else {
        // Extract zip
        let file = File::open(&temp_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(dest)?;
    }
    
    // Clean up temp file
    fs::remove_file(&temp_path)?;
    
    println!("{} Extraction complete", "✓".green());
    Ok(())
}

/// Versions command: list installed versions
pub fn versions() -> Result<()> {
    let versions = config::installed_versions();
    let current = config::current_version();
    
    if versions.is_empty() {
        println!("No SDK versions installed.");
        println!("Run {} to install.", "mpf-dev setup".cyan());
        return Ok(());
    }
    
    println!("{}", "Installed SDK versions:".bold());
    for v in &versions {
        if Some(v) == current.as_ref() {
            println!("  {} {} {}", "*".green(), v.green(), "(current)".dimmed());
        } else {
            println!("    {}", v);
        }
    }
    
    Ok(())
}

/// Use command: switch SDK version
pub fn use_version(version: &str) -> Result<()> {
    let version_normalized = if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{}", version)
    };
    
    let version_dir = config::version_dir(&version_normalized);
    
    if !version_dir.exists() {
        bail!(
            "Version {} is not installed. Run `mpf-dev setup --version {}`",
            version_normalized,
            version
        );
    }
    
    let current_link = config::current_link();
    if current_link.exists() || current_link.is_symlink() {
        fs::remove_file(&current_link)?;
    }
    
    #[cfg(unix)]
    std::os::unix::fs::symlink(&version_dir, &current_link)?;
    
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&version_dir, &current_link)?;
    
    // Update dev.json
    let mut config = DevConfig::load().unwrap_or_default();
    config.sdk_version = Some(version_normalized.clone());
    config.save()?;
    
    println!(
        "{} Now using SDK {}",
        "✓".green(),
        version_normalized
    );
    
    Ok(())
}

/// Link command: register component for source development
pub fn link(
    component: &str,
    lib: Option<String>,
    qml: Option<String>,
    plugin: Option<String>,
    headers: Option<String>,
) -> Result<()> {
    // Warn if unknown component
    if !config::is_known_component(component) {
        println!(
            "{} Unknown component '{}'. Known components: {}",
            "Warning:".yellow(),
            component,
            KNOWN_COMPONENTS.join(", ")
        );
    }
    
    let mut dev_config = DevConfig::load().unwrap_or_default();
    
    // Resolve paths to absolute
    let cwd = env::current_dir()?;
    let resolve = |p: Option<String>| -> Option<String> {
        p.map(|s| {
            let path = PathBuf::from(&s);
            if path.is_absolute() {
                s
            } else {
                cwd.join(path).to_string_lossy().to_string()
            }
        })
    };
    
    let comp_config = ComponentConfig {
        mode: ComponentMode::Source,
        lib: resolve(lib),
        qml: resolve(qml),
        plugin: resolve(plugin),
        headers: resolve(headers),
    };
    
    dev_config.components.insert(component.to_string(), comp_config.clone());
    dev_config.save()?;
    
    println!(
        "{} Component '{}' linked for source development",
        "✓".green(),
        component
    );
    
    if let Some(lib) = &comp_config.lib {
        println!("  lib: {}", lib);
    }
    if let Some(qml) = &comp_config.qml {
        println!("  qml: {}", qml);
    }
    if let Some(plugin) = &comp_config.plugin {
        println!("  plugin: {}", plugin);
    }
    if let Some(headers) = &comp_config.headers {
        println!("  headers: {}", headers);
    }
    
    Ok(())
}

/// Unlink command: remove component from source development
pub fn unlink(component: &str) -> Result<()> {
    let mut dev_config = DevConfig::load()?;
    
    if dev_config.components.remove(component).is_some() {
        dev_config.save()?;
        println!(
            "{} Component '{}' unlinked",
            "✓".green(),
            component
        );
    } else {
        println!(
            "{} Component '{}' was not linked",
            "Note:".yellow(),
            component
        );
    }
    
    Ok(())
}

/// Status command: show current configuration
pub fn status() -> Result<()> {
    let dev_config = DevConfig::load().unwrap_or_default();
    let current = config::current_version();
    let sdk_root = config::sdk_root();
    
    println!("{}", "MPF Development Environment Status".bold().cyan());
    println!();
    
    // SDK info
    println!("{}", "SDK:".bold());
    println!("  Root: {}", sdk_root.display());
    if let Some(v) = &current {
        println!("  Current version: {}", v.green());
    } else {
        println!("  Current version: {}", "not set".red());
    }
    
    // Config file location
    println!("  Config: {}", config::dev_config_path().display());
    println!();
    
    // Components
    println!("{}", "Components:".bold());
    if dev_config.components.is_empty() {
        println!("  No components linked for source development.");
        println!("  Use {} to register a component.", "mpf-dev link <component>".cyan());
    } else {
        for (name, comp) in &dev_config.components {
            let mode_str = match comp.mode {
                ComponentMode::Source => "source".green(),
                ComponentMode::Binary => "binary".dimmed(),
            };
            println!("  {} [{}]", name.bold(), mode_str);
            if let Some(lib) = &comp.lib {
                println!("    lib: {}", lib);
            }
            if let Some(qml) = &comp.qml {
                println!("    qml: {}", qml);
            }
            if let Some(plugin) = &comp.plugin {
                println!("    plugin: {}", plugin);
            }
            if let Some(headers) = &comp.headers {
                println!("    headers: {}", headers);
            }
        }
    }
    
    Ok(())
}

/// Env command: print environment variables
pub fn env_vars() -> Result<()> {
    let (lib_path, qml_path, plugin_path, _host_path) = build_env_paths()?;
    
    println!("{}", "# Add these to your shell:".dimmed());
    
    #[cfg(unix)]
    {
        println!("export LD_LIBRARY_PATH=\"{}\"", lib_path);
        println!("export QML_IMPORT_PATH=\"{}\"", qml_path);
        println!("export QT_PLUGIN_PATH=\"{}\"", plugin_path);
    }
    
    #[cfg(windows)]
    {
        println!("set PATH={};%PATH%", lib_path);
        println!("set QML_IMPORT_PATH={}", qml_path);
        println!("set QT_PLUGIN_PATH={}", plugin_path);
    }
    
    Ok(())
}

/// Run command: execute mpf-host with development overrides
pub fn run(debug: bool, args: Vec<String>) -> Result<()> {
    let current = config::current_link();
    if !current.exists() {
        bail!("No SDK version set. Run `mpf-dev setup` first.");
    }
    
    let (lib_path, qml_path, plugin_path, host_path) = build_env_paths()?;
    
    if !host_path.exists() {
        bail!("mpf-host not found at: {}", host_path.display());
    }
    
    if debug {
        println!("{}", "Running with development overrides:".dimmed());
        #[cfg(unix)]
        println!("  LD_LIBRARY_PATH={}", lib_path);
        #[cfg(windows)]
        println!("  PATH={}", lib_path);
        println!("  QML_IMPORT_PATH={}", qml_path);
        println!("  QT_PLUGIN_PATH={}", plugin_path);
        println!();
    }
    
    let mut cmd = Command::new(&host_path);
    cmd.args(&args);
    
    #[cfg(unix)]
    {
        cmd.env("LD_LIBRARY_PATH", &lib_path);
    }
    
    #[cfg(windows)]
    {
        let current_path = env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{};{}", lib_path, current_path));
    }
    
    cmd.env("QML_IMPORT_PATH", &qml_path);
    cmd.env("QT_PLUGIN_PATH", &plugin_path);
    
    let status = cmd.status()?;
    
    std::process::exit(status.code().unwrap_or(1));
}

/// Build environment path strings
fn build_env_paths() -> Result<(String, String, String, PathBuf)> {
    let dev_config = DevConfig::load().unwrap_or_default();
    let sdk = config::current_link();
    
    if !sdk.exists() {
        bail!("No SDK version set. Run `mpf-dev setup` first.");
    }
    
    let mut lib_paths: Vec<String> = Vec::new();
    let mut qml_paths: Vec<String> = Vec::new();
    let mut plugin_paths: Vec<String> = Vec::new();
    
    // Source components first (higher priority)
    for (name, comp) in &dev_config.components {
        if comp.mode == ComponentMode::Source {
            if let Some(lib) = &comp.lib {
                lib_paths.push(lib.clone());
            }
            if let Some(qml) = &comp.qml {
                qml_paths.push(qml.clone());
            }
            if let Some(plugin) = &comp.plugin {
                plugin_paths.push(plugin.clone());
            }
            
            // Debug: show which components are in source mode
            eprintln!("{} Using source: {}", "→".cyan(), name);
        }
    }
    
    // SDK paths as fallback
    lib_paths.push(sdk.join("lib").to_string_lossy().to_string());
    qml_paths.push(sdk.join("qml").to_string_lossy().to_string());
    plugin_paths.push(sdk.join("plugins").to_string_lossy().to_string());
    
    let sep = if cfg!(windows) { ";" } else { ":" };
    
    let host_path = sdk.join("bin").join(if cfg!(windows) {
        "mpf-host.exe"
    } else {
        "mpf-host"
    });
    
    Ok((
        lib_paths.join(sep),
        qml_paths.join(sep),
        plugin_paths.join(sep),
        host_path,
    ))
}
