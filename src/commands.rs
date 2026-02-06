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
use crate::LinkAction;

const GITHUB_REPO: &str = "dyzdyz010/mpf-release";

/// Normalize a path by removing .\ and .. components
fn normalize_path(p: PathBuf) -> String {
    // Try to canonicalize, fall back to string cleanup
    if let Ok(canonical) = p.canonicalize() {
        canonical.to_string_lossy().to_string()
    } else {
        // Path doesn't exist yet, just clean up the string
        let s = p.to_string_lossy().to_string();
        s.replace("\\.\\", "\\").replace("/./", "/")
    }
}

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
    config::set_current_version(&version_normalized)?;
    
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
    
    config::set_current_version(&version_normalized)?;
    
    // Update dev.json
    let mut dev_config = DevConfig::load().unwrap_or_default();
    dev_config.sdk_version = Some(version_normalized.clone());
    dev_config.save()?;
    
    println!(
        "{} Now using SDK {}",
        "✓".green(),
        version_normalized
    );
    
    Ok(())
}

/// New link action handler - dispatches to appropriate link function
pub fn link_action(action: LinkAction) -> Result<()> {
    match action {
        LinkAction::Plugin { name, path } => link_plugin(&name, &path),
        LinkAction::Host { path } => link_host(&path),
        LinkAction::Component { name, path } => link_component(&name, &path),
        LinkAction::Manual { name, lib, qml, plugin, headers, bin } => {
            link(&name, lib, qml, plugin, headers, bin, None)
        }
    }
}

/// Link a plugin - auto-derives lib, qml, plugin paths from build directory
pub fn link_plugin(name: &str, path: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let build_path = PathBuf::from(path);
    let abs_path = PathBuf::from(normalize_path(if build_path.is_absolute() {
        build_path
    } else {
        cwd.join(build_path)
    }));
    
    // Auto-derive paths from plugin build output
    let plugins_mpf_path = abs_path.join("plugins").join("mpf");
    let lib_path = if plugins_mpf_path.exists() {
        normalize_path(plugins_mpf_path)
    } else {
        normalize_path(abs_path.join("plugins"))
    };
    let qml_path = normalize_path(abs_path.join("qml"));
    let plugin_path = normalize_path(abs_path.clone());
    
    println!("{} Linking plugin '{}'", "→".cyan(), name);
    println!("  Build root: {}", abs_path.display());
    println!("  lib (plugins): {}", lib_path);
    println!("  qml: {}", qml_path);
    
    let mut dev_config = DevConfig::load().unwrap_or_default();
    
    // Store as "plugin-<name>" for clarity
    let component_name = if name.starts_with("plugin-") {
        name.to_string()
    } else {
        format!("plugin-{}", name)
    };
    
    dev_config.components.insert(component_name.clone(), ComponentConfig {
        mode: ComponentMode::Source,
        lib: Some(lib_path),
        qml: Some(qml_path),
        plugin: Some(plugin_path),
        headers: None,
        bin: None,
    });
    dev_config.save()?;
    
    println!("{} Plugin '{}' linked", "✓".green(), component_name);
    Ok(())
}

/// Link host - auto-derives bin, qml paths from build directory
pub fn link_host(path: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let build_path = PathBuf::from(path);
    let abs_path = PathBuf::from(normalize_path(if build_path.is_absolute() {
        build_path
    } else {
        cwd.join(build_path)
    }));
    
    let host_exe = if cfg!(windows) { "mpf-host.exe" } else { "mpf-host" };
    
    // Auto-derive bin path
    let bin_path = if abs_path.join("bin").join(host_exe).exists() {
        normalize_path(abs_path.join("bin"))
    } else if abs_path.join(host_exe).exists() {
        normalize_path(abs_path.clone())
    } else {
        normalize_path(abs_path.join("bin"))
    };
    
    // Auto-derive qml path
    let qml_path = if abs_path.join("qml").exists() {
        normalize_path(abs_path.join("qml"))
    } else {
        normalize_path(abs_path.clone())
    };
    
    println!("{} Linking host", "→".cyan());
    println!("  Build root: {}", abs_path.display());
    println!("  bin: {}", bin_path);
    println!("  qml: {}", qml_path);
    
    let mut dev_config = DevConfig::load().unwrap_or_default();
    dev_config.components.insert("host".to_string(), ComponentConfig {
        mode: ComponentMode::Source,
        lib: None,
        qml: Some(qml_path),
        plugin: None,
        headers: None,
        bin: Some(bin_path),
    });
    dev_config.save()?;
    
    println!("{} Host linked", "✓".green());
    Ok(())
}

/// Link a library component (ui-components, http-client, etc.)
pub fn link_component(name: &str, path: &str) -> Result<()> {
    let cwd = env::current_dir()?;
    let build_path = PathBuf::from(path);
    let abs_path = PathBuf::from(normalize_path(if build_path.is_absolute() {
        build_path
    } else {
        cwd.join(build_path)
    }));
    
    // Auto-derive paths based on component type
    let lib_path = if abs_path.join("lib").exists() {
        Some(normalize_path(abs_path.join("lib")))
    } else if abs_path.join("bin").exists() {
        // Windows DLLs often go in bin/
        Some(normalize_path(abs_path.join("bin")))
    } else {
        Some(normalize_path(abs_path.clone()))
    };
    
    let qml_path = if abs_path.join("qml").exists() {
        Some(normalize_path(abs_path.join("qml")))
    } else {
        None
    };
    
    let headers_path = if abs_path.join("include").exists() {
        Some(normalize_path(abs_path.join("include")))
    } else {
        None
    };
    
    println!("{} Linking component '{}'", "→".cyan(), name);
    println!("  Build root: {}", abs_path.display());
    if let Some(ref p) = lib_path { println!("  lib: {}", p); }
    if let Some(ref p) = qml_path { println!("  qml: {}", p); }
    if let Some(ref p) = headers_path { println!("  headers: {}", p); }
    
    let mut dev_config = DevConfig::load().unwrap_or_default();
    dev_config.components.insert(name.to_string(), ComponentConfig {
        mode: ComponentMode::Source,
        lib: lib_path,
        qml: qml_path,
        plugin: None,
        headers: headers_path,
        bin: None,
    });
    dev_config.save()?;
    
    println!("{} Component '{}' linked", "✓".green(), name);
    Ok(())
}

/// Link command: register component for source development (legacy interface)
/// 
/// For plugins, the --plugin option specifies the build output root directory.
/// The function will automatically derive:
/// - lib path: <plugin>/plugins (for DLL loading path)
/// - qml path: <plugin>/qml (for QML import path)
/// 
/// For host, the --host option specifies the build output root directory.
/// The function will automatically derive:
/// - bin path: <host>/bin (for mpf-host executable)
/// - qml path: <host>/qml (for QML modules)
/// 
/// You can also use --lib, --qml, --bin separately for fine-grained control.
pub fn link(
    component: &str,
    lib: Option<String>,
    qml: Option<String>,
    plugin: Option<String>,
    headers: Option<String>,
    bin: Option<String>,
    host: Option<String>,
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
    
    // Warn if bin is used for non-host component
    if bin.is_some() && component != "host" {
        println!(
            "{} --bin option is typically used for 'host' component only",
            "Note:".yellow()
        );
    }
    
    let mut dev_config = DevConfig::load().unwrap_or_default();
    
    // Resolve paths to absolute and normalize (remove .\ and ..)
    let cwd = env::current_dir()?;
    let resolve = |p: Option<String>| -> Option<String> {
        p.map(|s| {
            let path = PathBuf::from(&s);
            if path.is_absolute() {
                normalize_path(path)
            } else {
                normalize_path(cwd.join(path))
            }
        })
    };
    
    // If --plugin is specified, automatically derive lib and qml paths
    // --plugin points to build output root, which contains:
    //   - plugins/ subdirectory for plugin DLLs (may have mpf/ subfolder)
    //   - qml/ subdirectory for QML modules
    let (derived_lib, derived_qml) = if let Some(ref plugin_root) = plugin {
        let plugin_path = PathBuf::from(plugin_root);
        let abs_plugin_root = PathBuf::from(normalize_path(if plugin_path.is_absolute() {
            plugin_path
        } else {
            cwd.join(plugin_path)
        }));
        
        // Check for plugins/mpf subdirectory (common CMake output structure)
        // If it exists, use it; otherwise use plugins/ directly
        let plugins_mpf_path = abs_plugin_root.join("plugins").join("mpf");
        let lib_path = if plugins_mpf_path.exists() {
            normalize_path(plugins_mpf_path)
        } else {
            normalize_path(abs_plugin_root.join("plugins"))
        };
        let qml_path = normalize_path(abs_plugin_root.join("qml"));
        
        println!(
            "{} --plugin specified, auto-deriving paths from build root:",
            "Info:".cyan()
        );
        println!("  → lib (plugins): {}", lib_path);
        println!("  → qml: {}", qml_path);
        
        (Some(lib_path), Some(qml_path))
    } else {
        (None, None)
    };
    
    // If --host is specified, automatically derive bin and qml paths
    // --host points to build output root
    // Qt Creator outputs can be in different structures:
    //   - bin/mpf-host.exe (CMake default)
    //   - mpf-host.exe (Qt Creator sometimes puts it at root)
    let (derived_bin, derived_host_qml) = if let Some(ref host_root) = host {
        let host_path = PathBuf::from(host_root);
        let abs_host_root = PathBuf::from(normalize_path(if host_path.is_absolute() {
            host_path
        } else {
            cwd.join(host_path)
        }));
        
        let host_exe = if cfg!(windows) { "mpf-host.exe" } else { "mpf-host" };
        
        // Try to find mpf-host executable in different locations
        let bin_path = if abs_host_root.join("bin").join(host_exe).exists() {
            // Standard CMake layout: bin/mpf-host.exe
            normalize_path(abs_host_root.join("bin"))
        } else if abs_host_root.join(host_exe).exists() {
            // Qt Creator sometimes puts exe at build root
            normalize_path(abs_host_root.clone())
        } else {
            // Default to bin/ even if not found yet (might be built later)
            normalize_path(abs_host_root.join("bin"))
        };
        
        // Try to find qml directory
        let qml_path = if abs_host_root.join("qml").exists() {
            normalize_path(abs_host_root.join("qml"))
        } else {
            // Qt Creator might put it at build root
            normalize_path(abs_host_root.clone())
        };
        
        println!(
            "{} --host specified, auto-deriving paths from build root:",
            "Info:".cyan()
        );
        println!("  -> bin: {}", bin_path);
        println!("  -> qml: {}", qml_path);
        
        (Some(bin_path), Some(qml_path))
    } else {
        (None, None)
    };
    
    // Use explicit options if provided, otherwise use derived paths
    // Priority: explicit > --host derived > --plugin derived
    let final_lib = resolve(lib).or(derived_lib);
    let final_qml = resolve(qml).or(derived_host_qml).or(derived_qml);
    let final_bin = resolve(bin).or(derived_bin);
    
    let comp_config = ComponentConfig {
        mode: ComponentMode::Source,
        lib: final_lib,
        qml: final_qml,
        plugin: resolve(plugin),
        headers: resolve(headers),
        bin: final_bin,
    };
    
    dev_config.components.insert(component.to_string(), comp_config.clone());
    dev_config.save()?;
    
    println!(
        "{} Component '{}' linked for source development",
        "✓".green(),
        component
    );
    
    if let Some(bin) = &comp_config.bin {
        println!("  bin: {}", bin);
    }
    if let Some(lib) = &comp_config.lib {
        println!("  lib: {}", lib);
    }
    if let Some(qml) = &comp_config.qml {
        println!("  qml: {}", qml);
    }
    if let Some(plugin) = &comp_config.plugin {
        println!("  plugin (build root): {}", plugin);
    }
    if let Some(headers) = &comp_config.headers {
        println!("  headers: {}", headers);
    }
    
    Ok(())
}

/// Unlink command: remove component from source development
pub fn unlink(component: &str) -> Result<()> {
    let mut dev_config = DevConfig::load()?;
    
    if component == "all" {
        let count = dev_config.components.len();
        dev_config.components.clear();
        dev_config.save()?;
        println!("{} Unlinked {} component(s)", "✓".green(), count);
        return Ok(());
    }
    
    // Try exact match first
    if dev_config.components.remove(component).is_some() {
        dev_config.save()?;
        println!("{} Component '{}' unlinked", "✓".green(), component);
        return Ok(());
    }
    
    // Try with plugin- prefix
    let with_prefix = format!("plugin-{}", component);
    if dev_config.components.remove(&with_prefix).is_some() {
        dev_config.save()?;
        println!("{} Plugin '{}' unlinked", "✓".green(), component);
        return Ok(());
    }
    
    println!("{} Component '{}' was not linked", "Note:".yellow(), component);
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
    println!("{}", "📦 SDK".bold());
    println!("  Root: {}", sdk_root.display());
    if let Some(v) = &current {
        println!("  Version: {}", v.green());
    } else {
        println!("  Version: {}", "not set".red());
    }
    println!();
    
    // Group components by type
    let mut host: Option<(&String, &ComponentConfig)> = None;
    let mut plugins: Vec<(&String, &ComponentConfig)> = Vec::new();
    let mut libs: Vec<(&String, &ComponentConfig)> = Vec::new();
    
    for (name, comp) in &dev_config.components {
        if name == "host" {
            host = Some((name, comp));
        } else if name.starts_with("plugin-") || name.contains("plugin") {
            plugins.push((name, comp));
        } else {
            libs.push((name, comp));
        }
    }
    
    // Host section
    println!("{}", "🖥️  Host".bold());
    if let Some((_, comp)) = host {
        if let Some(bin) = &comp.bin {
            println!("  {} bin: {}", "✓".green(), bin);
        }
        if let Some(qml) = &comp.qml {
            println!("    qml: {}", qml);
        }
    } else {
        println!("  {} Not linked", "○".dimmed());
        println!("  {}", "mpf-dev link host <build-path>".dimmed());
    }
    println!();
    
    // Plugins section
    println!("{}", "🔌 Plugins".bold());
    if plugins.is_empty() {
        println!("  {} None linked", "○".dimmed());
        println!("  {}", "mpf-dev link plugin <name> <build-path>".dimmed());
    } else {
        for (name, comp) in &plugins {
            let display_name = name.strip_prefix("plugin-").unwrap_or(name);
            println!("  {} {}", "✓".green(), display_name.bold());
            if let Some(lib) = &comp.lib {
                println!("    lib: {}", lib);
            }
            if let Some(qml) = &comp.qml {
                println!("    qml: {}", qml);
            }
        }
    }
    println!();
    
    // Libraries section
    println!("{}", "📚 Libraries".bold());
    if libs.is_empty() {
        println!("  {} None linked", "○".dimmed());
        println!("  {}", "mpf-dev link component <name> <build-path>".dimmed());
    } else {
        for (name, comp) in &libs {
            println!("  {} {}", "✓".green(), name.bold());
            if let Some(lib) = &comp.lib {
                println!("    lib: {}", lib);
            }
            if let Some(qml) = &comp.qml {
                println!("    qml: {}", qml);
            }
            if let Some(headers) = &comp.headers {
                println!("    headers: {}", headers);
            }
        }
    }
    println!();
    
    // Config file location
    println!("{}", "📝 Config".bold());
    println!("  {}", config::dev_config_path().display());
    
    Ok(())
}

/// Env command: print environment variables
pub fn env_vars() -> Result<()> {
    let (sdk_root, lib_path, qml_path, plugin_path, mpf_plugin_path, _host_path) = build_env_paths()?;
    
    println!("{}", "# MPF Development Environment".bold().cyan());
    println!("{}", "# Add these to your shell or IDE:".dimmed());
    println!();
    
    // Detect Qt path from common locations
    let qt_hint = detect_qt_path();
    
    #[cfg(unix)]
    {
        println!("{}", "# === Unix/Linux/macOS ===".green());
        println!("export MPF_SDK_ROOT=\"{}\"", sdk_root);
        if let Some(ref qt) = qt_hint {
            println!("export CMAKE_PREFIX_PATH=\"{};{}\"", qt, sdk_root);
        } else {
            println!("export CMAKE_PREFIX_PATH=\"$QT_DIR;{}\"  # Set QT_DIR to your Qt path", sdk_root);
        }
        println!("export QML_IMPORT_PATH=\"{}\"", qml_path);
        println!("export LD_LIBRARY_PATH=\"{}\"", lib_path);
        println!("export QT_PLUGIN_PATH=\"{}\"", plugin_path);
        if !mpf_plugin_path.is_empty() {
            println!("export MPF_PLUGIN_PATH=\"{}\"", mpf_plugin_path);
        }
    }
    
    #[cfg(windows)]
    {
        println!("{}", "# === Windows (CMD) ===".green());
        println!("set MPF_SDK_ROOT={}", sdk_root);
        if let Some(ref qt) = qt_hint {
            println!("set CMAKE_PREFIX_PATH={};{}", qt, sdk_root);
        } else {
            println!("set CMAKE_PREFIX_PATH=C:\\Qt\\6.8.3\\mingw_64;{}", sdk_root);
        }
        println!("set QML_IMPORT_PATH={}", qml_path);
        println!("set PATH={};%PATH%", lib_path);
        println!("set QT_PLUGIN_PATH={}", plugin_path);
        if !mpf_plugin_path.is_empty() {
            println!("set MPF_PLUGIN_PATH={}", mpf_plugin_path);
        }
        
        println!();
        println!("{}", "# === Windows (PowerShell) ===".green());
        println!("$env:MPF_SDK_ROOT=\"{}\"", sdk_root);
        if let Some(ref qt) = qt_hint {
            println!("$env:CMAKE_PREFIX_PATH=\"{};{}\"", qt, sdk_root);
        } else {
            println!("$env:CMAKE_PREFIX_PATH=\"C:\\Qt\\6.8.3\\mingw_64;{}\"", sdk_root);
        }
        println!("$env:QML_IMPORT_PATH=\"{}\"", qml_path);
        println!("$env:PATH=\"{};$env:PATH\"", lib_path);
        println!("$env:QT_PLUGIN_PATH=\"{}\"", plugin_path);
        if !mpf_plugin_path.is_empty() {
            println!("$env:MPF_PLUGIN_PATH=\"{}\"", mpf_plugin_path);
        }
    }
    
    println!();
    println!("{}", "# Then configure CMake:".dimmed());
    println!("{}", "#   cmake -B build -G \"MinGW Makefiles\"  # Windows".dimmed());
    println!("{}", "#   cmake -B build -G Ninja                # Linux".dimmed());
    
    Ok(())
}

/// Try to detect Qt installation path
fn detect_qt_path() -> Option<String> {
    // Check environment first
    if let Ok(qt_dir) = std::env::var("QT_DIR") {
        return Some(qt_dir);
    }
    if let Ok(qt_dir) = std::env::var("Qt6_DIR") {
        return Some(qt_dir);
    }
    
    // Check common paths
    #[cfg(windows)]
    {
        let common_paths = [
            "C:\\Qt\\6.8.3\\mingw_64",
            "C:\\Qt\\6.8.2\\mingw_64",
            "C:\\Qt\\6.8.1\\mingw_64",
            "C:\\Qt\\6.8.0\\mingw_64",
        ];
        for path in common_paths {
            if std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
    }
    
    #[cfg(unix)]
    {
        let common_paths = [
            "/opt/qt6",
            "/usr/local/Qt-6.8.3",
            "/usr/lib/qt6",
        ];
        for path in common_paths {
            if std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
    }
    
    None
}

/// Run command: execute mpf-host with development overrides
pub fn run(debug: bool, args: Vec<String>) -> Result<()> {
    let current = config::current_link();
    if !current.exists() {
        bail!("No SDK version set. Run `mpf-dev setup` first.");
    }
    
    let (sdk_root, lib_path, qml_path, plugin_path, mpf_plugin_path, host_path) = build_env_paths()?;
    
    if !host_path.exists() {
        bail!("mpf-host not found at: {}", host_path.display());
    }
    
    if debug {
        println!("{}", "Running with development overrides:".dimmed());
        println!("  MPF_SDK_ROOT={}", sdk_root);
        #[cfg(unix)]
        println!("  LD_LIBRARY_PATH={}", lib_path);
        #[cfg(windows)]
        println!("  PATH={}", lib_path);
        println!("  QML_IMPORT_PATH={}", qml_path);
        println!("  QT_PLUGIN_PATH={}", plugin_path);
        if !mpf_plugin_path.is_empty() {
            println!("  MPF_PLUGIN_PATH={}", mpf_plugin_path);
        }
        println!();
    }
    
    let mut cmd = Command::new(&host_path);
    cmd.args(&args);
    
    // MPF_SDK_ROOT tells mpf-host where the SDK is installed
    // This is the primary way mpf-host discovers its paths
    cmd.env("MPF_SDK_ROOT", &sdk_root);
    
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
    
    // Set MPF_PLUGIN_PATH for mpf-host to discover linked plugins
    // This allows linked source plugins to override SDK binary plugins
    if !mpf_plugin_path.is_empty() {
        cmd.env("MPF_PLUGIN_PATH", &mpf_plugin_path);
    }
    
    let status = cmd.status()?;
    
    std::process::exit(status.code().unwrap_or(1));
}

// =============================================================================
// Workspace Commands
// =============================================================================

const WORKSPACE_REPOS: &[(&str, &str)] = &[
    ("mpf-sdk", "https://github.com/dyzdyz010/mpf-sdk.git"),
    ("mpf-ui-components", "https://github.com/dyzdyz010/mpf-ui-components.git"),
    ("mpf-http-client", "https://github.com/dyzdyz010/mpf-http-client.git"),
    ("mpf-host", "https://github.com/dyzdyz010/mpf-host.git"),
    ("mpf-plugin-orders", "https://github.com/dyzdyz010/mpf-plugin-orders.git"),
    ("mpf-plugin-rules", "https://github.com/dyzdyz010/mpf-plugin-rules.git"),
];

/// Find workspace root by looking for .mpf-workspace marker
fn find_workspace_root() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;
    loop {
        if current.join(".mpf-workspace").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Workspace init: create workspace and clone all components
pub fn workspace_init(path: Option<String>) -> Result<()> {
    let workspace_dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap());
    
    println!("{}", "MPF Workspace Initialization".bold().cyan());
    println!("Directory: {}", workspace_dir.display());
    println!();
    
    fs::create_dir_all(&workspace_dir)?;
    
    // Create workspace marker
    let marker_path = workspace_dir.join(".mpf-workspace");
    fs::write(&marker_path, "# MPF Workspace\n")?;
    
    // Clone all repos
    for (name, url) in WORKSPACE_REPOS {
        let repo_dir = workspace_dir.join(name);
        
        if repo_dir.exists() {
            println!("{} {} (already exists)", "->".yellow(), name);
            continue;
        }
        
        println!("{} Cloning {}...", "->".cyan(), name);
        let status = Command::new("git")
            .args(["clone", url, &repo_dir.to_string_lossy()])
            .status()
            .context("Failed to run git clone")?;
        
        if !status.success() {
            bail!("Failed to clone {}", name);
        }
    }
    
    // Create top-level CMakeLists.txt
    let cmake_content = generate_workspace_cmake();
    fs::write(workspace_dir.join("CMakeLists.txt"), cmake_content)?;
    
    // Create CMakePresets.json for easy Qt Creator integration
    let presets_content = generate_cmake_presets();
    fs::write(workspace_dir.join("CMakePresets.json"), presets_content)?;
    
    println!();
    println!("{} Workspace initialized!", "[OK]".green());
    println!();
    println!("Next steps:");
    println!("  1. Open {} in Qt Creator", workspace_dir.join("CMakeLists.txt").display());
    println!("  2. Configure with MinGW kit");
    println!("  3. Build and run");
    println!();
    println!("Or use CLI:");
    println!("  cd {}", workspace_dir.display());
    println!("  mpf-dev workspace build");
    println!("  mpf-dev workspace run");
    
    Ok(())
}

fn generate_workspace_cmake() -> String {
    String::from(r##"cmake_minimum_required(VERSION 3.21)
project(mpf-workspace VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

if(COMMAND qt_policy)
    qt_policy(SET QTP0001 NEW)
    qt_policy(SET QTP0004 NEW)
endif()

find_package(Qt6 REQUIRED COMPONENTS Core Gui Qml Quick QuickControls2 Network)

# SDK (header-only)
add_library(mpf-sdk INTERFACE)
add_library(MPF::sdk ALIAS mpf-sdk)
target_include_directories(mpf-sdk INTERFACE
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/mpf-sdk/include>
)
target_link_libraries(mpf-sdk INTERFACE Qt6::Core Qt6::Gui Qt6::Qml)

# HTTP Client (static)
add_library(mpf-http-client STATIC
    mpf-http-client/src/http_client.cpp
    mpf-http-client/include/mpf/http/http_client.h
)
add_library(MPF::http-client ALIAS mpf-http-client)
target_include_directories(mpf-http-client PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/mpf-http-client/include>
)
target_compile_definitions(mpf-http-client PUBLIC MPF_HTTP_CLIENT_STATIC)
target_link_libraries(mpf-http-client PUBLIC Qt6::Core Qt6::Network)

# UI Components
add_compile_definitions(MPF_UI_COMPONENTS_EXPORTS)

set(UI_QML_FILES
    mpf-ui-components/qml/MPFCard.qml
    mpf-ui-components/qml/MPFButton.qml
    mpf-ui-components/qml/MPFIconButton.qml
    mpf-ui-components/qml/StatusBadge.qml
    mpf-ui-components/qml/MPFDialog.qml
    mpf-ui-components/qml/MPFTextField.qml
    mpf-ui-components/qml/MPFLoadingIndicator.qml
)

foreach(file ${UI_QML_FILES})
    string(REGEX REPLACE "^mpf-ui-components/qml/" "" alias "${file}")
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${alias})
endforeach()

qt_add_qml_module(mpf-ui-components
    URI MPF.Components
    VERSION 1.0
    RESOURCE_PREFIX /
    SOURCES
        mpf-ui-components/src/ui_components_global.h
        mpf-ui-components/src/color_helper.h
        mpf-ui-components/src/color_helper.cpp
        mpf-ui-components/src/input_validator.h
        mpf-ui-components/src/input_validator.cpp
    QML_FILES ${UI_QML_FILES}
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/MPF/Components
)
add_library(MPF::ui-components ALIAS mpf-ui-components)
target_include_directories(mpf-ui-components PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/mpf-ui-components/src>
)
target_link_libraries(mpf-ui-components PUBLIC Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick)

# Host Application
add_executable(mpf-host
    mpf-host/src/main.cpp
    mpf-host/src/application.cpp
    mpf-host/src/service_registry.cpp
    mpf-host/src/logger.cpp
    mpf-host/src/plugin_metadata.cpp
    mpf-host/src/plugin_manager.cpp
    mpf-host/src/plugin_loader.cpp
    mpf-host/src/navigation_service.cpp
    mpf-host/src/settings_service.cpp
    mpf-host/src/theme_service.cpp
    mpf-host/src/menu_service.cpp
    mpf-host/src/event_bus_service.cpp
    mpf-host/src/qml_context.cpp
)
target_include_directories(mpf-host PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/mpf-host/include
    ${CMAKE_CURRENT_BINARY_DIR}/host
)
target_link_libraries(mpf-host PRIVATE
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::QuickControls2
    MPF::sdk MPF::ui-components
)

# Generate version header
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/host/mpf/version.h [=[
#pragma once
#define MPF_VERSION_MAJOR 1
#define MPF_VERSION_MINOR 0
#define MPF_VERSION_PATCH 0
#define MPF_VERSION_STRING "1.0.0-workspace"
]=])

# Generate sdk_paths header
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/host/mpf/sdk_paths.h [=[
#pragma once
#define MPF_SDK_HAS_QML_PATH 0
#define MPF_PREFIX ""
#define MPF_QML_PATH ""
]=])

# Host QML
set(HOST_QML_FILES
    mpf-host/qml/Main.qml
    mpf-host/qml/SideMenu.qml
    mpf-host/qml/MenuItemCustom.qml
    mpf-host/qml/ErrorDialog.qml
)
set(HOST_RESOURCES mpf-host/qml/images/logo.svg)

foreach(file ${HOST_QML_FILES} ${HOST_RESOURCES})
    string(REGEX REPLACE "^mpf-host/qml/" "" alias "${file}")
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${alias})
endforeach()

qt_add_qml_module(mpf-host
    URI MPF.Host
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES ${HOST_QML_FILES}
    RESOURCES ${HOST_RESOURCES}
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/MPF/Host
)

# Orders Plugin
add_library(orders-plugin SHARED
    mpf-plugin-orders/src/orders_plugin.cpp
    mpf-plugin-orders/src/orders_service.cpp
    mpf-plugin-orders/src/order_model.cpp
)
target_include_directories(orders-plugin PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/mpf-plugin-orders/include
)
target_link_libraries(orders-plugin PRIVATE
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick Qt6::Network
    MPF::sdk MPF::http-client
)

set(ORDERS_QML_FILES
    mpf-plugin-orders/qml/OrdersPage.qml
    mpf-plugin-orders/qml/OrderCard.qml
    mpf-plugin-orders/qml/CreateOrderDialog.qml
)
foreach(file ${ORDERS_QML_FILES})
    string(REGEX REPLACE "^mpf-plugin-orders/qml/" "" alias "${file}")
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${alias})
endforeach()

qt_add_qml_module(orders-plugin
    URI YourCo.Orders
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES ${ORDERS_QML_FILES}
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/YourCo/Orders
    NO_PLUGIN
)

# Rules Plugin
add_library(rules-plugin SHARED
    mpf-plugin-rules/src/rules_plugin.cpp
    mpf-plugin-rules/src/orders_service.cpp
    mpf-plugin-rules/src/order_model.cpp
)
target_include_directories(rules-plugin PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/mpf-plugin-rules/include
)
target_link_libraries(rules-plugin PRIVATE
    Qt6::Core Qt6::Gui Qt6::Qml Qt6::Quick
    MPF::sdk
)

set(RULES_QML_FILES
    mpf-plugin-rules/qml/OrdersPage.qml
    mpf-plugin-rules/qml/OrderCard.qml
    mpf-plugin-rules/qml/CreateOrderDialog.qml
    mpf-plugin-rules/qml/TestCard.qml
)
foreach(file ${RULES_QML_FILES})
    string(REGEX REPLACE "^mpf-plugin-rules/qml/" "" alias "${file}")
    set_source_files_properties(${file} PROPERTIES QT_RESOURCE_ALIAS ${alias})
endforeach()

qt_add_qml_module(rules-plugin
    URI Biiz.Rules
    VERSION 1.0
    RESOURCE_PREFIX /
    QML_FILES ${RULES_QML_FILES}
    OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/qml/Biiz/Rules
    NO_PLUGIN
)

# Output directories
set_target_properties(mpf-host PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin
)
set_target_properties(orders-plugin rules-plugin PROPERTIES
    LIBRARY_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/plugins
    RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/plugins
)

file(MAKE_DIRECTORY ${CMAKE_BINARY_DIR}/plugins)
file(MAKE_DIRECTORY ${CMAKE_BINARY_DIR}/qml)
"##)
}

fn generate_cmake_presets() -> String {
    r##"{
  "version": 6,
  "configurePresets": [
    {
      "name": "debug",
      "displayName": "Debug",
      "generator": "Ninja",
      "binaryDir": "${sourceDir}/build",
      "cacheVariables": {
        "CMAKE_BUILD_TYPE": "Debug"
      }
    },
    {
      "name": "release",
      "displayName": "Release",
      "generator": "Ninja",
      "binaryDir": "${sourceDir}/build",
      "cacheVariables": {
        "CMAKE_BUILD_TYPE": "Release"
      }
    }
  ],
  "buildPresets": [
    {"name": "debug", "configurePreset": "debug"},
    {"name": "release", "configurePreset": "release"}
  ]
}
"##.to_string()
}

/// Workspace build: build all components
pub fn workspace_build(config: &str) -> Result<()> {
    let workspace = find_workspace_root()
        .context("Not in an MPF workspace. Run 'mpf-dev workspace init' first.")?;
    
    println!("{}", "Building MPF Workspace".bold().cyan());
    println!("Directory: {}", workspace.display());
    println!("Configuration: {}", config);
    println!();
    
    let build_dir = workspace.join("build");
    
    // Configure if needed
    if !build_dir.join("CMakeCache.txt").exists() {
        println!("{} Configuring CMake...", "->".cyan());
        
        let status = Command::new("cmake")
            .current_dir(&workspace)
            .args([
                "-B", "build",
                "-G", "Ninja",
                &format!("-DCMAKE_BUILD_TYPE={}", config),
            ])
            .status()
            .context("Failed to run cmake configure")?;
        
        if !status.success() {
            bail!("CMake configuration failed");
        }
    }
    
    // Build
    println!("{} Building...", "->".cyan());
    
    let status = Command::new("cmake")
        .current_dir(&workspace)
        .args(["--build", "build", "-j"])
        .status()
        .context("Failed to run cmake build")?;
    
    if !status.success() {
        bail!("Build failed");
    }
    
    println!();
    println!("{} Build complete!", "[OK]".green());
    println!();
    println!("Output:");
    let host_name = if cfg!(windows) { "mpf-host.exe" } else { "mpf-host" };
    println!("  Host: {}", build_dir.join("bin").join(host_name).display());
    println!("  Plugins: {}", build_dir.join("plugins").display());
    println!("  QML: {}", build_dir.join("qml").display());
    
    Ok(())
}

/// Workspace run: run mpf-host from workspace
pub fn workspace_run(args: Vec<String>) -> Result<()> {
    let workspace = find_workspace_root()
        .context("Not in an MPF workspace. Run 'mpf-dev workspace init' first.")?;
    
    let build_dir = workspace.join("build");
    let host_exe = if cfg!(windows) {
        build_dir.join("bin").join("mpf-host.exe")
    } else {
        build_dir.join("bin").join("mpf-host")
    };
    
    if !host_exe.exists() {
        bail!("mpf-host not found. Run 'mpf-dev workspace build' first.");
    }
    
    println!("{} Running mpf-host from workspace...", "->".cyan());
    
    let mut cmd = Command::new(&host_exe);
    cmd.current_dir(&workspace);
    cmd.args(&args);
    
    // Set library paths
    #[cfg(windows)]
    {
        let current_path = env::var("PATH").unwrap_or_default();
        let lib_path = format!(
            "{};{};{}",
            build_dir.join("bin").display(),
            build_dir.join("plugins").display(),
            current_path
        );
        cmd.env("PATH", lib_path);
    }
    
    #[cfg(unix)]
    {
        let lib_path = format!(
            "{}:{}",
            build_dir.join("bin").display(),
            build_dir.join("plugins").display()
        );
        cmd.env("LD_LIBRARY_PATH", lib_path);
    }
    
    cmd.env("QML_IMPORT_PATH", build_dir.join("qml").to_string_lossy().to_string());
    
    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}

/// Workspace status: show workspace info
pub fn workspace_status() -> Result<()> {
    let workspace = find_workspace_root();
    
    println!("{}", "MPF Workspace Status".bold().cyan());
    println!();
    
    if let Some(ws) = workspace {
        println!("{} Workspace: {}", "[OK]".green(), ws.display());
        
        // Check each component
        for (name, _) in WORKSPACE_REPOS {
            let repo_dir = ws.join(name);
            if repo_dir.exists() {
                // Get git status
                let output = Command::new("git")
                    .current_dir(&repo_dir)
                    .args(["log", "-1", "--oneline"])
                    .output();
                
                let commit = output
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                
                println!("  {} {}: {}", "[OK]".green(), name, commit.dimmed());
            } else {
                println!("  {} {}: {}", "[X]".red(), name, "missing".red());
            }
        }
        
        // Check build
        let build_dir = ws.join("build");
        if build_dir.exists() {
            println!();
            let host_exe = if cfg!(windows) {
                build_dir.join("bin").join("mpf-host.exe")
            } else {
                build_dir.join("bin").join("mpf-host")
            };
            
            if host_exe.exists() {
                println!("{} Built: {}", "[OK]".green(), "yes".green());
            } else {
                println!("{} Built: {}", "->".yellow(), "not yet".yellow());
            }
        } else {
            println!();
            println!("{} Built: {}", "[X]".red(), "no".red());
        }
    } else {
        println!("{} Not in an MPF workspace", "[X]".red());
        println!();
        println!("Run {} to create one.", "mpf-dev workspace init".cyan());
    }
    
    Ok(())
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Build environment path strings
/// Returns: (sdk_root, lib_path, qml_path, qt_plugin_path, mpf_plugin_path, host_path)
fn build_env_paths() -> Result<(String, String, String, String, String, PathBuf)> {
    let dev_config = DevConfig::load().unwrap_or_default();
    let sdk = config::current_link();
    
    if !sdk.exists() {
        bail!("No SDK version set. Run 'mpf-dev setup' first.");
    }
    
    // SDK root path (used by mpf-host to find default paths)
    let sdk_root = sdk.to_string_lossy().to_string();
    
    let mut lib_paths: Vec<String> = Vec::new();
    let mut qml_paths: Vec<String> = Vec::new();
    let mut plugin_paths: Vec<String> = Vec::new();
    let mut mpf_plugin_paths: Vec<String> = Vec::new();  // MPF plugin paths for development
    let mut host_bin_override: Option<String> = None;
    
    // Source components first (higher priority)
    for (name, comp) in &dev_config.components {
        if comp.mode == ComponentMode::Source {
            if let Some(lib) = &comp.lib {
                lib_paths.push(lib.clone());
                
                // For plugin components (not host/sdk), also add to MPF_PLUGIN_PATH
                // This tells mpf-host where to find the linked plugin DLLs
                if name != "host" && name != "sdk" {
                    mpf_plugin_paths.push(lib.clone());
                }
            }
            if let Some(qml) = &comp.qml {
                qml_paths.push(qml.clone());
            }
            if let Some(plugin) = &comp.plugin {
                plugin_paths.push(plugin.clone());
            }
            
            // Check for host component bin override
            if name == "host" {
                if let Some(bin) = &comp.bin {
                    host_bin_override = Some(bin.clone());
                }
            }
            
            // Debug: show which components are in source mode
            eprintln!("{} Using source: {}", "->".cyan(), name);
        }
    }
    
    // SDK paths as fallback
    lib_paths.push(sdk.join("lib").to_string_lossy().to_string());
    qml_paths.push(sdk.join("qml").to_string_lossy().to_string());
    plugin_paths.push(sdk.join("plugins").to_string_lossy().to_string());
    
    let sep = if cfg!(windows) { ";" } else { ":" };
    
    // Use linked host bin if available, otherwise use SDK's mpf-host
    let host_exe_name = if cfg!(windows) { "mpf-host.exe" } else { "mpf-host" };
    let host_path = if let Some(bin_dir) = host_bin_override {
        let linked_host = PathBuf::from(&bin_dir).join(host_exe_name);
        eprintln!("{} Using linked host: {}", "->".cyan(), linked_host.display());
        linked_host
    } else {
        sdk.join("bin").join(host_exe_name)
    };
    
    Ok((
        sdk_root,
        lib_paths.join(sep),
        qml_paths.join(sep),
        plugin_paths.join(sep),
        mpf_plugin_paths.join(sep),
        host_path,
    ))
}
