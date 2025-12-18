use crate::FinnContext;
use crate::utils;
use std::fs;
use std::io::copy;
use anyhow::{Result, anyhow, Context};
use colored::*;
use reqwest::blocking::Client;
use serde_json::Value;

pub fn run(version: Option<String>, ctx: &FinnContext) -> Result<()> {
    let client = Client::new();
    
    let (os_keyword, binary_name) = if cfg!(target_os = "windows") {
        ("windows", "fin.exe")
    } else if cfg!(target_os = "linux") {
        ("linux", "fin")
    } else if cfg!(target_os = "macos") {
        ("macos", "fin")
    } else {
        return Err(anyhow!("Unsupported operating system."));
    };

    let pb = utils::create_spinner("Fetching release info...", ctx.quiet);

    let repo_owner = "M1778M";
    let repo_name = "fin-compiler";
    
    let api_url = if let Some(v) = &version {
        format!("https://api.github.com/repos/{}/{}/releases/tags/{}", repo_owner, repo_name, v)
    } else {
        format!("https://api.github.com/repos/{}/{}/releases/latest", repo_owner, repo_name)
    };

    let resp = client.get(&api_url)
        .header("User-Agent", "finn-cli")
        .header("Accept", "application/vnd.github+json")
        .send()
        .context("Failed to connect to GitHub API")?;

    if resp.status() == 404 {
        pb.finish_and_clear();
        return Err(anyhow!("Release not found."));
    }

    if !resp.status().is_success() {
        pb.finish_and_clear();
        return Err(anyhow!("GitHub API Error: {}", resp.status()));
    }

    let json: Value = resp.json().context("Failed to parse GitHub API response")?;
    let tag_name = json["tag_name"].as_str().unwrap_or("unknown version");
    pb.set_message(format!("Found release: {}", tag_name));

    let assets = json["assets"].as_array()
        .ok_or_else(|| anyhow!("Release {} has no assets attached.", tag_name))?;

    let target_asset = assets.iter().find(|asset| {
        let name = asset["name"].as_str().unwrap_or("").to_lowercase();
        name.contains(os_keyword)
    });

    let asset = match target_asset {
        Some(a) => a,
        None => {
            pb.finish_and_clear();
            return Err(anyhow!("No binary found for '{}' in release {}.", os_keyword, tag_name));
        }
    };

    let download_url = asset["browser_download_url"].as_str()
        .ok_or_else(|| anyhow!("Asset has no download URL"))?;
    
    let asset_filename = asset["name"].as_str().unwrap_or("fin");

    pb.set_message(format!("Downloading {}...", asset_filename));
    
    let mut download_resp = client.get(download_url)
        .header("User-Agent", "finn-cli")
        .send()
        .context("Failed to start download")?;

    if !download_resp.status().is_success() {
        return Err(anyhow!("Download failed with status: {}", download_resp.status()));
    }

    let home = dirs::home_dir().ok_or(anyhow!("Could not find home directory"))?;
    let bin_dir = home.join(".finn").join("bin");
    
    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir).context("Failed to create ~/.finn/bin directory")?;
    }

    let target_path = bin_dir.join(binary_name);
    let mut dest = fs::File::create(&target_path).context("Failed to create output file")?;
    
    copy(&mut download_resp, &mut dest).context("Failed to write binary to disk")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_path, perms)?;
    }

    pb.finish_and_clear();
    if !ctx.quiet {
        println!("{} Successfully installed {} ({})", "[OK]".green(), binary_name, tag_name);
        println!("   Location: {:?}", target_path);
    }

    Ok(())
}
