use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use dialoguer::{Input, Password, Select, theme::ColorfulTheme};
use image::ImageFormat;
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::RANGE,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tempfile::NamedTempFile;
use url::Url;

const USER_AGENT: &str = "craftingtable/0.1 (Rust media toolbox)";
const PUBLIC_API_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/public-apis/public-apis/master/README.md";
const MAX_PUBLIC_API_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_RESULTS: usize = 50;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct ApiKeyStore {
    sources: HashMap<String, HashMap<String, String>>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    api_key_store_path: Option<PathBuf>,
    default_browser: Option<String>,
}

struct PublicApiEntry {
    name: String,
    description: String,
    auth: String,
    category: String,
    documentation_url: Url,
}

const COOKIE_BROWSERS: [&str; 6] = ["none", "firefox", "chrome", "brave", "librewolf", "chromium"];

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    loop {
        print_header();
        let actions = [
            "Format conversion (media / images)",
            "Resolution scaling (video)",
            "Mass video/audio download (yt-dlp)",
            "Mass image conversion (native Rust)",
            "Mass image/gallery download",
            "Fast single-file download (native Rust)",
            "Settings",
            "Quit",
        ];
        let Some(choice) = select("What would you like to craft today?", &actions)? else {
            break;
        };
        if choice == 7 {
            break;
        }

        println!();
        let result = match choice {
            0 => convert_media(),
            1 => scale_video(),
            2 => download_media(),
            3 => convert_images(),
            4 => download_gallery(),
            5 => fast_download(),
            6 => manage_settings(),
            _ => Ok(()),
        };
        if let Err(error) = result {
            eprintln!("✘ {error:#}");
        }
        pause()?;
    }
    println!("Bye!");
    Ok(())
}

fn print_header() {
    println!("\n╔══════════════════════════════════════╗");
    println!("║            CRAFTING TABLE            ║");
    println!("╚══════════════════════════════════════╝");
}

fn select(title: &str, options: &[&str]) -> Result<Option<usize>> {
    select_with_default(title, options, 0)
}

fn select_with_default(
    title: &str,
    options: &[&str],
    default_index: usize,
) -> Result<Option<usize>> {
    Ok(Select::with_theme(&ColorfulTheme::default())
        .with_prompt(title)
        .items(options)
        .default(default_index.min(options.len().saturating_sub(1)))
        .interact_opt()?)
}

fn ask(prompt: &str, default: Option<&str>) -> Result<String> {
    let theme = ColorfulTheme::default();
    let mut input = Input::<String>::with_theme(&theme).with_prompt(prompt);
    if let Some(value) = default {
        input = input.default(value.to_owned());
    }
    Ok(input.allow_empty(true).interact_text()?)
}

fn pause() -> Result<()> {
    Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt("Press Enter to return to the menu")
        .allow_empty(true)
        .interact_text()?;
    Ok(())
}

fn clean_path(input: &str) -> PathBuf {
    let mut value = input.trim().to_owned();
    if value.len() >= 2 {
        let first = value.chars().next().unwrap_or_default();
        let last = value.chars().last().unwrap_or_default();
        if (first == '\'' && last == '\'') || (first == '"' && last == '"') {
            value = value[1..value.len() - 1].to_owned();
        }
    }
    let mut unescaped = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                unescaped.push(next);
            }
        } else {
            unescaped.push(ch);
        }
    }
    if unescaped == "~" || unescaped.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(unescaped.strip_prefix("~/").unwrap_or(""));
        }
    }
    PathBuf::from(unescaped)
}

fn destination() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine the home directory")?;
    let options = [
        "Downloads",
        "Documents",
        "Pictures",
        "Videos",
        "Music",
        "Custom path…",
    ];
    let Some(choice) = select("Destination", &options)? else {
        bail!("Destination selection cancelled");
    };
    let mut base = match choice {
        0 => home.join("Downloads"),
        1 => home.join("Documents"),
        2 => home.join("Pictures"),
        3 => home.join("Videos"),
        4 => home.join("Music"),
        _ => {
            let path = ask("Path (or shortcut like dz, dc/work, ms/mix)", Some(""))?;
            shortcut_path(&home, &clean_path(&path))
        }
    };
    let subfolder = ask("New subfolder inside it (blank for none)", Some(""))?;
    if !subfolder.trim().is_empty() {
        base.push(clean_path(&subfolder));
    }
    fs::create_dir_all(&base).with_context(|| format!("Cannot create {}", base.display()))?;
    Ok(base)
}

fn shortcut_path(home: &Path, path: &Path) -> PathBuf {
    let Some(input) = path.to_str() else {
        return path.to_owned();
    };
    let choices = [
        ("dz", "Downloads"),
        ("dc", "Documents"),
        ("img", "Pictures"),
        ("mv", "Videos"),
        ("ms", "Music"),
    ];
    for (shortcut, directory) in choices {
        if input == shortcut {
            return home.join(directory);
        }
        if let Some(rest) = input.strip_prefix(&format!("{shortcut}/")) {
            return home.join(directory).join(rest);
        }
    }
    if input.is_empty() {
        home.join("Downloads")
    } else {
        path.to_owned()
    }
}

fn run_tool(program: &str, args: &[String]) -> Result<ExitStatus> {
    Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("Could not start `{program}`. Is it installed?"))
}

fn require_success(program: &str, args: &[String]) -> Result<()> {
    let status = run_tool(program, args)?;
    ensure!(status.success(), "{program} exited with status {status}");
    Ok(())
}

fn media_file(prompt: &str, want_directory: bool) -> Result<PathBuf> {
    let raw = ask(prompt, None)?;
    let path = clean_path(&raw);
    if want_directory {
        ensure!(path.is_dir(), "Folder not found: {}", path.display());
    } else {
        ensure!(path.is_file(), "File not found: {}", path.display());
    }
    Ok(path)
}

fn convert_media() -> Result<()> {
    let source = media_file("File to convert (paste path)", false)?;
    let formats = [
        "mp4", "mkv", "webm", "mp3", "m4a", "flac", "wav", "png", "jpg", "webp", "gif",
        "other…",
    ];
    let Some(index) = select("Target format", &formats)? else {
        return Ok(());
    };
    let extension = if index == formats.len() - 1 {
        ask("Extension", None)?.trim_start_matches('.').to_owned()
    } else {
        formats[index].to_owned()
    };
    ensure!(!extension.trim().is_empty(), "No format given");

    let dest = destination()?;
    let output = dest.join(format!("{}.{}", file_stem(&source)?, extension));
    println!("➜ Converting…");
    require_success(
        "ffmpeg",
        &strings([
            "-hide_banner".to_owned(),
            "-i".to_owned(),
            source.to_string_lossy().into_owned(),
            output.to_string_lossy().into_owned(),
        ]),
    )?;
    println!("✔ Saved to {}", output.display());
    Ok(())
}

fn scale_video() -> Result<()> {
    let source = media_file("Video file to scale (paste path)", false)?;
    let resolutions = ["720p (HD)", "1080p (Full HD)", "1440p (2K)", "2160p (4K)", "Custom…"];
    let Some(choice) = select("Target resolution", &resolutions)? else {
        return Ok(());
    };
    let resolution = if choice == 4 {
        ask("Target size in pixels", None)?
    } else {
        ["720", "1080", "1440", "2160"][choice].to_owned()
    };
    ensure!(
        !resolution.is_empty() && resolution.chars().all(|ch| ch.is_ascii_digit()),
        "Invalid resolution"
    );
    let Some(orientation) = select("Orientation", &["Horizontal (landscape)", "Vertical (portrait)"])? else {
        return Ok(());
    };
    let filter = if orientation == 0 {
        format!("scale=-2:{resolution}")
    } else {
        format!("scale={resolution}:-2")
    };
    let dest = destination()?;
    let output = dest.join(format!("{}_{}p.mp4", file_stem(&source)?, resolution));
    println!("➜ Scaling…");
    require_success(
        "ffmpeg",
        &strings([
            "-hide_banner".to_owned(),
            "-i".to_owned(),
            source.to_string_lossy().into_owned(),
            "-vf".to_owned(),
            filter,
            "-vcodec".to_owned(),
            "libx264".to_owned(),
            "-crf".to_owned(),
            "23".to_owned(),
            "-acodec".to_owned(),
            "aac".to_owned(),
            output.to_string_lossy().into_owned(),
        ]),
    )?;
    println!("✔ Saved to {}", output.display());
    Ok(())
}

fn download_media() -> Result<()> {
    let links = ask_links("Paste video link(s), separated by spaces")?;
    let formats = ["Video (mp4)", "Audio (mp3 + cover art)", "Audio (m4a + cover art)"];
    let Some(kind) = select("Download as", &formats)? else {
        return Ok(());
    };
    let mut args = Vec::new();
    if links.iter().any(|link| {
        ["instagram.com", "facebook.com", "x.com", "twitter.com"]
            .iter()
            .any(|host| link.contains(host))
    }) {
        println!("Login-protected site detected.");
        let settings = load_app_settings()?;
        let default_browser = settings.default_browser.as_deref().unwrap_or("none");
        let default_index = COOKIE_BROWSERS
            .iter()
            .position(|browser| *browser == default_browser)
            .context("Configured default browser is not supported")?;
        let Some(browser) = select_with_default(
            "Grab cookies from browser",
            &COOKIE_BROWSERS,
            default_index,
        )? else {
            return Ok(());
        };
        if browser > 0 {
            args.extend([
                "--cookies-from-browser".to_owned(),
                COOKIE_BROWSERS[browser].to_owned(),
            ]);
        }
    }
    let dest = destination()?;
    if kind == 0 {
        args.extend([
            "-S".to_owned(),
            "res,ext:mp4:m4a".to_owned(),
            "--recode".to_owned(),
            "mp4".to_owned(),
        ]);
    } else {
        args.extend([
            "-x".to_owned(),
            "--audio-format".to_owned(),
            if kind == 1 { "mp3" } else { "m4a" }.to_owned(),
            "--audio-quality".to_owned(),
            "0".to_owned(),
            "--embed-thumbnail".to_owned(),
            "--convert-thumbnails".to_owned(),
            "jpg".to_owned(),
            "--embed-metadata".to_owned(),
            "--ppa".to_owned(),
            "EmbedThumbnail+ffmpeg_o:-c:v mjpeg -vf crop=\"'if(gt(ih,iw),iw,ih)':'if(gt(iw,ih),ih,iw)'\""
                .to_owned(),
        ]);
    }
    args.extend(["-P".to_owned(), dest.to_string_lossy().into_owned()]);
    args.extend(links);
    println!("➜ Downloading…");
    require_success("yt-dlp", &args)?;
    println!("✔ Download complete → {}", dest.display());
    Ok(())
}

fn convert_images() -> Result<()> {
    let source_dir = media_file("Source folder (paste path)", true)?;
    let source_ext = ask("Source file extension (e.g. webp, jpg)", None)?
        .trim_start_matches('.')
        .to_ascii_lowercase();
    ensure!(!source_ext.is_empty(), "No extension given");
    let formats = ["png", "jpg", "webp", "avif", "other…"];
    let Some(choice) = select("Target format", &formats)? else {
        return Ok(());
    };
    let target_ext = if choice == formats.len() - 1 {
        ask("Extension", None)?
            .trim_start_matches('.')
            .to_ascii_lowercase()
    } else {
        formats[choice].to_owned()
    };
    ensure!(!target_ext.is_empty(), "No target extension given");
    let naming = ["Keep original names", "Number sequentially (1, 2, 3…)"];
    let Some(naming_choice) = select("Output names", &naming)? else {
        return Ok(());
    };
    let dest = destination()?;
    let mut files = fs::read_dir(&source_dir)
        .with_context(|| format!("Cannot read {}", source_dir.display()))?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(&source_ext))
        })
        .collect::<Vec<_>>();
    files.sort();
    ensure!(!files.is_empty(), "No .{source_ext} files in {}", source_dir.display());

    let format = ImageFormat::from_extension(&target_ext)
        .with_context(|| format!("Unsupported image format: {target_ext}"))?;
    let mut failures = Vec::new();
    for (index, source) in files.iter().enumerate() {
        let name = if naming_choice == 0 {
            format!("{}.{}", file_stem(source)?, target_ext)
        } else {
            format!("{}.{}", index + 1, target_ext)
        };
        let output = dest.join(name);
        print!("\r[{}/{}] {}", index + 1, files.len(), source.file_name().unwrap_or_default().to_string_lossy());
        let result = image::open(source)
            .with_context(|| format!("Could not decode {}", source.display()))
            .and_then(|image| {
                image
                    .save_with_format(&output, format)
                    .with_context(|| format!("Could not save {}", output.display()))
            });
        if let Err(error) = result {
            failures.push(format!("{}: {error:#}", source.display()));
        }
    }
    println!();
    println!("✔ Converted {}/{} images → {}", files.len() - failures.len(), files.len(), dest.display());
    if !failures.is_empty() {
        for failure in failures {
            eprintln!("✘ {failure}");
        }
    }
    Ok(())
}

fn download_gallery() -> Result<()> {
    let links = ask_links("Paste image/gallery link(s), separated by spaces")?;
    let dest = destination()?;
    let api_keys = load_api_key_store()?;
    let app_settings = load_app_settings()?;
    let mut errors = Vec::new();
    for link in links {
        let host = Url::parse(&link)
            .with_context(|| format!("Invalid gallery URL: {link}"))?
            .host_str()
            .map(str::to_ascii_lowercase)
            .context("Gallery URL has no host")?;
        let config_file = matching_api_key_config(&api_keys, &host)?;
        let mut args = Vec::new();
        if let Some(config_file) = &config_file {
            args.extend([
                "--config-json".to_owned(),
                config_file.path().to_string_lossy().into_owned(),
            ]);
        }
        append_cookie_browser_args(&mut args, app_settings.default_browser.as_deref())?;
        args.extend([
            "-d".to_owned(),
            dest.to_string_lossy().into_owned(),
            link.clone(),
        ]);
        if let Err(error) = require_success("gallery-dl", &args) {
            errors.push(format!("{link}: {error:#}"));
        }
    }
    if errors.is_empty() {
        println!("✔ Done → {}", dest.display());
    } else {
        for error in errors {
            eprintln!("✘ {error}");
        }
        bail!("One or more gallery downloads failed");
    }
    Ok(())
}

fn append_cookie_browser_args(args: &mut Vec<String>, browser: Option<&str>) -> Result<()> {
    let Some(browser) = browser.filter(|browser| *browser != "none") else {
        return Ok(());
    };
    ensure!(
        COOKIE_BROWSERS.contains(&browser),
        "Unsupported default browser for downloader cookies: {browser}"
    );
    args.extend([
        "--cookies-from-browser".to_owned(),
        browser.to_owned(),
    ]);
    Ok(())
}

fn manage_api_keys() -> Result<()> {
    loop {
        let actions = ["Add or update a source key", "Remove a source key", "Back"];
        match select("Manage source API keys", &actions)? {
            Some(0) => add_or_update_api_key()?,
            Some(1) => remove_api_key()?,
            Some(_) | None => return Ok(()),
        }
    }
}

fn manage_settings() -> Result<()> {
    loop {
        let settings = load_app_settings()?;
        let key_store_path = effective_api_key_store_path(&settings)?;
        println!("\nAPI key store: {}", key_store_path.display());
        println!(
            "Default cookie browser: {}",
            settings.default_browser.as_deref().unwrap_or("none")
        );
        let actions = [
            "Browse public API catalog",
            "Manage source API keys",
            "Change API key storage location",
            "Choose default cookie browser",
            "Back",
        ];
        match select("Settings", &actions)? {
            Some(0) => browse_public_api_catalog()?,
            Some(1) => manage_api_keys()?,
            Some(2) => configure_api_key_store_location()?,
            Some(3) => configure_default_browser()?,
            Some(_) | None => return Ok(()),
        }
    }
}

fn browse_public_api_catalog() -> Result<()> {
    println!("➜ Fetching the public API catalog…");
    let entries = load_public_api_catalog()?;
    println!("✔ Loaded {} API entries", entries.len());
    println!(
        "The catalog lists API providers and auth requirements; obtain keys from each provider."
    );

    loop {
        let actions = ["Browse by category", "Search APIs", "Back"];
        match select("Public API catalog", &actions)? {
            Some(0) => browse_catalog_category(&entries)?,
            Some(1) => search_public_api_catalog(&entries)?,
            Some(_) | None => return Ok(()),
        }
    }
}

fn load_public_api_catalog() -> Result<Vec<PublicApiEntry>> {
    let mut response = http_client()?
        .get(PUBLIC_API_CATALOG_URL)
        .send()
        .context("Could not fetch the public API catalog")?
        .error_for_status()
        .context("The public API catalog returned an error")?;
    if let Some(length) = response.content_length() {
        ensure!(
            length <= MAX_PUBLIC_API_CATALOG_BYTES,
            "The public API catalog is unexpectedly large ({length} bytes)"
        );
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_PUBLIC_API_CATALOG_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("Could not read the public API catalog")?;
    ensure!(
        bytes.len() as u64 <= MAX_PUBLIC_API_CATALOG_BYTES,
        "The public API catalog exceeded the {} byte limit",
        MAX_PUBLIC_API_CATALOG_BYTES
    );
    let markdown = String::from_utf8(bytes).context("The public API catalog was not valid UTF-8")?;
    parse_public_api_catalog(&markdown)
}

fn parse_public_api_catalog(markdown: &str) -> Result<Vec<PublicApiEntry>> {
    let mut in_index = false;
    let mut category = String::new();
    let mut entries = Vec::new();
    for line in markdown.lines() {
        let line = line.trim();
        if line == "## Index" {
            in_index = true;
            continue;
        }
        if line == "## License" {
            break;
        }
        if !in_index {
            continue;
        }
        if let Some(next_category) = line.strip_prefix("### ") {
            category = next_category.trim().to_owned();
            continue;
        }
        if let Some(entry) = parse_public_api_row(line, &category) {
            entries.push(entry);
        }
    }
    ensure!(
        !entries.is_empty(),
        "Could not find API entries in the public API catalog"
    );
    Ok(entries)
}

fn parse_public_api_row(line: &str, category: &str) -> Option<PublicApiEntry> {
    if category.is_empty() || !line.starts_with('|') {
        return None;
    }
    let cells = line
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect::<Vec<_>>();
    if cells.len() < 3
        || cells[0].eq_ignore_ascii_case("api")
        || cells[0].contains("---") && cells[0].chars().all(|ch| matches!(ch, '-' | ':' | ' '))
    {
        return None;
    }
    let (name, documentation_url) = parse_markdown_link(cells[0])?;
    let documentation_url = Url::parse(documentation_url).ok()?;
    if documentation_url.scheme() != "https" && documentation_url.scheme() != "http" {
        return None;
    }
    Some(PublicApiEntry {
        name: name.to_owned(),
        description: cells.get(1).copied().unwrap_or_default().to_owned(),
        auth: cells.get(2).copied().unwrap_or("Unknown").to_owned(),
        category: category.to_owned(),
        documentation_url,
    })
}

fn parse_markdown_link(cell: &str) -> Option<(&str, &str)> {
    let label_start = cell.find('[')? + 1;
    let label_end = label_start + cell[label_start..].find(']')?;
    let url_start = label_end + 2;
    if !cell[label_end..].starts_with("](") {
        return None;
    }
    let url_end = url_start + cell[url_start..].find(')')?;
    Some((&cell[label_start..label_end], &cell[url_start..url_end]))
}

fn browse_catalog_category(entries: &[PublicApiEntry]) -> Result<()> {
    let categories = entries
        .iter()
        .map(|entry| entry.category.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let Some(choice) = select("Choose an API category", &categories)? else {
        return Ok(());
    };
    let matches = entries
        .iter()
        .filter(|entry| entry.category == categories[choice])
        .collect::<Vec<_>>();
    show_catalog_entries(&matches)
}

fn search_public_api_catalog(entries: &[PublicApiEntry]) -> Result<()> {
    let query = ask("Search API names, descriptions, categories, or auth", None)?;
    let query = query.trim().to_lowercase();
    ensure!(!query.is_empty(), "Search text cannot be empty");
    let matches = entries
        .iter()
        .filter(|entry| {
            entry.name.to_lowercase().contains(&query)
                || entry.description.to_lowercase().contains(&query)
                || entry.category.to_lowercase().contains(&query)
                || entry.auth.to_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    ensure!(!matches.is_empty(), "No APIs matched “{query}”");
    show_catalog_entries(&matches)
}

fn show_catalog_entries(entries: &[&PublicApiEntry]) -> Result<()> {
    let shown = entries.len().min(MAX_CATALOG_RESULTS);
    if entries.len() > shown {
        println!(
            "Found {} APIs; showing the first {shown}. Refine your search to narrow the results.",
            entries.len()
        );
    }
    let choices = entries[..shown]
        .iter()
        .map(|entry| {
            format!(
                "{} — Auth: {} — {}",
                entry.name, entry.auth, entry.category
            )
        })
        .collect::<Vec<_>>();
    let Some(choice) = select(
        "Choose an API to open its documentation",
        &choices.iter().map(String::as_str).collect::<Vec<_>>(),
    )? else {
        return Ok(());
    };
    let entry = entries[choice];
    println!("\n{} — {}", entry.name, entry.description);
    println!("Documentation: {}", entry.documentation_url);
    open_documentation(&entry.documentation_url)
}

fn open_documentation(url: &Url) -> Result<()> {
    ensure!(
        url.scheme() == "https" || url.scheme() == "http",
        "Documentation URL must use HTTP or HTTPS"
    );
    #[cfg(target_os = "linux")]
    let (program, args) = ("xdg-open", vec![url.to_string()]);
    #[cfg(target_os = "macos")]
    let (program, args) = ("open", vec![url.to_string()]);
    #[cfg(target_os = "windows")]
    let (program, args) = (
        "rundll32",
        vec!["url.dll,FileProtocolHandler".to_owned(), url.to_string()],
    );
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    bail!("Opening documentation is not supported on this operating system");
    require_success(program, &args).context("Could not open API documentation in the system browser")
}

fn configure_api_key_store_location() -> Result<()> {
    let mut settings = load_app_settings()?;
    let old_path = effective_api_key_store_path(&settings)?;
    let current_custom_path = settings
        .api_key_store_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let input = ask(
        "API key file path (blank to use the default location)",
        Some(current_custom_path.as_str()),
    )?;
    let new_custom_path = if input.trim().is_empty() {
        None
    } else {
        Some(absolute_path(&clean_path(&input))?)
    };
    let updated_settings = AppSettings {
        api_key_store_path: new_custom_path,
        default_browser: settings.default_browser.take(),
    };
    let new_path = effective_api_key_store_path(&updated_settings)?;
    let settings_path = app_settings_path()?;
    ensure!(
        !paths_refer_to_same_file(&new_path, &settings_path)?,
        "The API key file cannot be the application settings file"
    );
    if paths_refer_to_same_file(&old_path, &new_path)? {
        save_app_settings(&updated_settings)?;
        println!("✔ API key storage location unchanged");
        return Ok(());
    }
    ensure!(
        !new_path.is_dir(),
        "API key storage path is a directory: {}",
        new_path.display()
    );

    let old_store = load_api_key_store_at(&old_path)?;
    if old_path.is_file() {
        ensure!(
            !new_path.exists(),
            "The selected path already exists. Choose a new file path before moving the current API key store."
        );
        save_api_key_store_at(&old_store, &new_path)?;
        if let Err(error) = save_app_settings(&updated_settings) {
            fs::remove_file(&new_path).with_context(|| {
                format!(
                    "Could not clean up the copied API key store at {}",
                    new_path.display()
                )
            })?;
            return Err(error).context("Could not save the new API key storage location");
        }
        fs::remove_file(&old_path).with_context(|| {
            format!("Could not remove the old API key store at {}", old_path.display())
        })?;
    } else {
        if new_path.exists() {
            load_api_key_store_at(&new_path)?;
        }
        save_app_settings(&updated_settings)?;
    }
    println!("✔ API key storage location set to {}", new_path.display());
    Ok(())
}

fn configure_default_browser() -> Result<()> {
    let mut settings = load_app_settings()?;
    let current_browser = settings.default_browser.as_deref().unwrap_or("none");
    let default_index = COOKIE_BROWSERS
        .iter()
        .position(|browser| *browser == current_browser)
        .context("Configured default browser is not supported")?;
    let Some(choice) = select_with_default(
        "Default browser for downloader cookies",
        &COOKIE_BROWSERS,
        default_index,
    )? else {
        return Ok(());
    };
    settings.default_browser = Some(COOKIE_BROWSERS[choice].to_owned());
    save_app_settings(&settings)?;
    println!(
        "✔ Default cookie browser set to {}",
        COOKIE_BROWSERS[choice]
    );
    Ok(())
}

fn add_or_update_api_key() -> Result<()> {
    let host = prompt_source_host()?;
    let setting = ask(
        "Downloader setting path (must start with extractor.)",
        None,
    )?;
    validate_setting_path(&setting)?;
    let value = Password::with_theme(&ColorfulTheme::default())
        .with_prompt("API key or setting value")
        .allow_empty_password(false)
        .interact()?;

    let mut store = load_api_key_store()?;
    store
        .sources
        .entry(host.clone())
        .or_default()
        .insert(setting, value);
    save_api_key_store(&store)?;
    println!("✔ Saved credentials for {host}");
    Ok(())
}

fn remove_api_key() -> Result<()> {
    let mut store = load_api_key_store()?;
    let mut entries = store
        .sources
        .iter()
        .flat_map(|(host, settings)| {
            settings
                .keys()
                .map(move |setting| (host.clone(), setting.clone()))
        })
        .collect::<Vec<_>>();
    entries.sort();
    ensure!(!entries.is_empty(), "No saved source API keys");

    let labels = entries
        .iter()
        .map(|(host, setting)| format!("{host} — {setting}"))
        .collect::<Vec<_>>();
    let Some(choice) = select("Remove which saved key?", &labels.iter().map(String::as_str).collect::<Vec<_>>())? else {
        return Ok(());
    };
    let (host, setting) = &entries[choice];
    if let Some(settings) = store.sources.get_mut(host) {
        settings.remove(setting);
        if settings.is_empty() {
            store.sources.remove(host);
        }
    }
    save_api_key_store(&store)?;
    println!("✔ Removed saved key for {host}");
    Ok(())
}

fn prompt_source_host() -> Result<String> {
    let input = ask("Source host (for example, media.example)", None)?;
    normalize_host(&input)
}

fn normalize_host(input: &str) -> Result<String> {
    let input = input.trim();
    ensure!(!input.is_empty(), "Source host cannot be empty");
    let url = Url::parse(&format!("https://{input}")).context("Enter a host name, not a URL")?;
    ensure!(
        url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
        "Enter only a host name without a path or query"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "Enter a host name without user information"
    );
    ensure!(url.port().is_none(), "Enter a host name without a port");
    Ok(url
        .host_str()
        .context("Invalid source host")?
        .trim_end_matches('.')
        .to_ascii_lowercase())
}

fn validate_setting_path(path: &str) -> Result<Vec<&str>> {
    let parts = path.split('.').collect::<Vec<_>>();
    ensure!(
        parts.len() >= 3 && parts[0] == "extractor",
        "Setting path must be extractor.<source>.<setting>"
    );
    ensure!(
        parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        }),
        "Setting path may contain only letters, numbers, underscores, and hyphens separated by dots"
    );
    Ok(parts)
}

fn config_directory() -> Result<PathBuf> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")))
        .context("Could not determine config directory")?;
    Ok(root.join("craftingtable"))
}

fn app_settings_path() -> Result<PathBuf> {
    Ok(config_directory()?.join("settings.json"))
}

fn default_api_key_store_path() -> Result<PathBuf> {
    Ok(config_directory()?.join("api-keys.json"))
}

fn load_app_settings() -> Result<AppSettings> {
    let path = app_settings_path()?;
    let settings = match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<AppSettings>(&bytes)
            .with_context(|| format!("Invalid application settings: {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
        Err(error) => {
            return Err(error).with_context(|| format!("Could not read {}", path.display()));
        }
    };
    if let Some(browser) = settings.default_browser.as_deref() {
        ensure!(
            COOKIE_BROWSERS.contains(&browser),
            "Unsupported default browser in {}: {browser}",
            path.display()
        );
    }
    Ok(settings)
}

fn save_app_settings(settings: &AppSettings) -> Result<()> {
    save_private_json(&app_settings_path()?, settings, "application settings")
}

fn effective_api_key_store_path(settings: &AppSettings) -> Result<PathBuf> {
    match &settings.api_key_store_path {
        Some(path) => Ok(path.clone()),
        None => default_api_key_store_path(),
    }
}

fn api_key_store_path() -> Result<PathBuf> {
    effective_api_key_store_path(&load_app_settings()?)
}

fn load_api_key_store() -> Result<ApiKeyStore> {
    let path = api_key_store_path()?;
    load_api_key_store_at(&path)
}

fn load_api_key_store_at(path: &Path) -> Result<ApiKeyStore> {
    match fs::read(&path) {
        Ok(bytes) => {
            let store: ApiKeyStore = serde_json::from_slice(&bytes)
                .with_context(|| format!("Invalid API key store: {}", path.display()))?;
            for settings in store.sources.values() {
                for setting in settings.keys() {
                    validate_setting_path(setting)
                        .with_context(|| format!("Invalid saved setting path: {setting}"))?;
                }
            }
            Ok(store)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ApiKeyStore::default()),
        Err(error) => Err(error).with_context(|| format!("Could not read {}", path.display())),
    }
}

fn save_api_key_store(store: &ApiKeyStore) -> Result<()> {
    save_api_key_store_at(store, &api_key_store_path()?)
}

fn save_api_key_store_at(store: &ApiKeyStore, path: &Path) -> Result<()> {
    save_private_json(path, store, "API key store")
}

fn save_private_json<T: Serialize>(path: &Path, value: &T, description: &str) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Could not create {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(&path)
        .with_context(|| format!("Could not save {description} at {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Could not secure {}", path.display()))?;
    }
    Ok(())
}

fn paths_refer_to_same_file(left: &Path, right: &Path) -> Result<bool> {
    Ok(comparable_path(left)? == comparable_path(right)?)
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn comparable_path(path: &Path) -> Result<PathBuf> {
    let absolute = normalize_absolute_path(path)?;
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .context("Invalid path")?
                .to_owned(),
        );
        ancestor = ancestor.parent().context("Invalid path")?;
    }
    let mut resolved = ancestor
        .canonicalize()
        .with_context(|| format!("Could not resolve {}", ancestor.display()))?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_absolute_path(path: &Path) -> Result<PathBuf> {
    let absolute = absolute_path(path)?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                normalized.push(component.as_os_str())
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else {
                    normalized.push("..");
                }
            }
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    Ok(normalized)
}

fn matching_api_key_config(store: &ApiKeyStore, host: &str) -> Result<Option<NamedTempFile>> {
    let mut config = Value::Object(Map::new());
    let Some((_, settings)) = store
        .sources
        .iter()
        .filter(|(saved_host, _)| host == saved_host.as_str() || host.ends_with(&format!(".{saved_host}")))
        .max_by_key(|(saved_host, _)| saved_host.len())
    else {
        return Ok(None);
    };
    for (setting, value) in settings {
        let parts = validate_setting_path(setting)?;
        insert_nested_config(&mut config, &parts, Value::String(value.clone()))?;
    }
    let mut file = NamedTempFile::new()?;
    serde_json::to_writer(file.as_file_mut(), &config)?;
    file.as_file().sync_all()?;
    Ok(Some(file))
}

fn insert_nested_config(root: &mut Value, parts: &[&str], value: Value) -> Result<()> {
    let mut current = root;
    for part in &parts[..parts.len() - 1] {
        if !current.is_object() {
            *current = Value::Object(Map::new());
        }
        current = current
            .as_object_mut()
            .context("Invalid nested config structure")?
            .entry((*part).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    current
        .as_object_mut()
        .context("Invalid nested config structure")?
        .insert(parts[parts.len() - 1].to_owned(), value);
    Ok(())
}

fn fast_download() -> Result<()> {
    let raw_url = ask("Paste file download link", None)?;
    let url = Url::parse(raw_url.trim()).context("Invalid URL")?;
    ensure!(url.scheme() == "http" || url.scheme() == "https", "Only HTTP(S) links are supported");
    let custom_name = ask("Custom filename (blank = original name)", Some(""))?;
    let speed = ask("Max speed (e.g. 5M, 500K — blank = unlimited)", Some(""))?;
    let speed_limit = if speed.trim().is_empty() {
        None
    } else {
        Some(parse_speed(&speed)?)
    };
    let dest = destination()?;
    let filename = if custom_name.trim().is_empty() {
        url.path_segments()
            .and_then(Iterator::last)
            .filter(|name| !name.is_empty())
            .map(safe_filename)
            .unwrap_or_else(|| "download".to_owned())
    } else {
        safe_filename(custom_name.trim())
    };
    let output = unique_path(&dest, &filename);
    println!("➜ Downloading…");
    download_parallel(&url, &output, speed_limit)?;
    println!("✔ Saved to {}", output.display());
    Ok(())
}

fn ask_links(prompt: &str) -> Result<Vec<String>> {
    let input = ask(prompt, None)?;
    let links = input.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    ensure!(!links.is_empty(), "No links given");
    Ok(links)
}

fn http_client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .context("Could not create HTTP client")?)
}

fn download_single(client: &Client, url: &Url, output: &Path, speed_limit: Option<u64>) -> Result<()> {
    let mut response = client
        .get(url.clone())
        .send()
        .with_context(|| format!("Could not download {url}"))?
        .error_for_status()
        .with_context(|| format!("Download failed for {url}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(
        output.parent().context("Output path has no parent")?,
    )?;
    copy_with_limit(&mut response, temporary.as_file_mut(), speed_limit)?;
    temporary
        .persist(output)
        .with_context(|| format!("Could not save {}", output.display()))?;
    Ok(())
}

fn download_parallel(url: &Url, output: &Path, speed_limit: Option<u64>) -> Result<()> {
    let client = http_client()?;
    let mut probe = client.get(url.clone()).header(RANGE, "bytes=0-0").send()?;
    if let Some(limit) = speed_limit {
        drop(probe);
        let mut response = client
            .get(url.clone())
            .send()?
            .error_for_status()
            .context("Download request failed")?;
        let mut temporary = tempfile::NamedTempFile::new_in(
            output.parent().context("Output path has no parent")?,
        )?;
        copy_with_limit(&mut response, temporary.as_file_mut(), Some(limit))?;
        temporary
            .persist(output)
            .with_context(|| format!("Could not save {}", output.display()))?;
        return Ok(());
    }
    if probe.status() != StatusCode::PARTIAL_CONTENT {
        probe.error_for_status_ref().context("Download request failed")?;
        let mut temporary = tempfile::NamedTempFile::new_in(
            output.parent().context("Output path has no parent")?,
        )?;
        copy_with_limit(&mut probe, temporary.as_file_mut(), None)?;
        temporary
            .persist(output)
            .with_context(|| format!("Could not save {}", output.display()))?;
        return Ok(());
    }
    let total = content_range_total(&probe)?;
    drop(probe);
    if total < 2 {
        download_single(&client, url, output, None)?;
        return Ok(());
    }
    let count = 16_u64.min(total);
    let scratch = tempfile::tempdir_in(output.parent().context("Output path has no parent")?)?;
    let mut workers = Vec::new();
    for index in 0..count {
        let start = total * index / count;
        let end = total * (index + 1) / count - 1;
        let part_path = scratch.path().join(format!("part-{index}"));
        let client = client.clone();
        let url = url.clone();
        workers.push(thread::spawn(move || -> Result<PathBuf> {
            let response = client
                .get(url)
                .header(RANGE, format!("bytes={start}-{end}"))
                .send()?
                .error_for_status()?;
            ensure!(
                response.status() == StatusCode::PARTIAL_CONTENT,
                "Server stopped honoring byte ranges"
            );
            validate_content_range(&response, start, end, total)?;
            let mut file = File::create(&part_path)?;
            let expected = end - start + 1;
            let copied = std::io::copy(&mut response.take(expected + 1), &mut file)?;
            ensure!(
                copied == expected,
                "Unexpected range length: expected {expected} bytes, received {copied}"
            );
            file.sync_all()?;
            Ok(part_path)
        }));
    }
    let mut parts = Vec::new();
    for worker in workers {
        parts.push(worker.join().map_err(|_| anyhow::anyhow!("Download worker panicked"))??);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        output.parent().context("Output path has no parent")?,
    )?;
    let mut copied_total = 0;
    for part in parts {
        let mut file = File::open(part)?;
        copied_total += std::io::copy(&mut file, temporary.as_file_mut())?;
    }
    ensure!(copied_total == total, "Download size mismatch: expected {total}, received {copied_total}");
    temporary
        .persist(output)
        .with_context(|| format!("Could not save {}", output.display()))?;
    Ok(())
}

fn content_range_total(response: &Response) -> Result<u64> {
    let value = response
        .headers()
        .get("content-range")
        .context("Server omitted Content-Range for a partial response")?
        .to_str()?;
    let total = value
        .rsplit_once('/')
        .context("Invalid Content-Range header")?
        .1
        .parse::<u64>()
        .context("Invalid total size in Content-Range")?;
    ensure!(total > 0, "Server reported an empty file");
    Ok(total)
}

fn validate_content_range(response: &Response, start: u64, end: u64, total: u64) -> Result<()> {
    let value = response
        .headers()
        .get("content-range")
        .context("Server omitted Content-Range for a range request")?
        .to_str()?;
    let (range, reported_total) = value
        .strip_prefix("bytes ")
        .context("Invalid Content-Range header")?
        .split_once('/')
        .context("Invalid Content-Range header")?;
    ensure!(
        range == format!("{start}-{end}") && reported_total == total.to_string(),
        "Server returned an unexpected byte range: {value}"
    );
    Ok(())
}

fn copy_with_limit(reader: &mut impl Read, writer: &mut impl Write, limit: Option<u64>) -> Result<u64> {
    let start = Instant::now();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let size = reader.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        writer.write_all(&buffer[..size])?;
        copied += size as u64;
        if let Some(bytes_per_second) = limit {
            let expected = Duration::from_secs_f64(copied as f64 / bytes_per_second as f64);
            let elapsed = start.elapsed();
            if expected > elapsed {
                thread::sleep(expected - elapsed);
            }
        }
    }
    writer.flush()?;
    Ok(copied)
}

fn parse_speed(value: &str) -> Result<u64> {
    let mut number = value.trim().to_owned();
    let multiplier = match number.chars().last() {
        Some('K' | 'k') => {
            number.pop();
            1024_u64
        }
        Some('M' | 'm') => {
            number.pop();
            1024_u64.pow(2)
        }
        Some('G' | 'g') => {
            number.pop();
            1024_u64.pow(3)
        }
        _ => 1,
    };
    let number = number
        .parse::<u64>()
        .context("Speed must be a positive number with optional K/M/G suffix")?;
    let speed = number
        .checked_mul(multiplier)
        .context("Speed limit is too large")?;
    ensure!(speed > 0, "Speed limit must be greater than zero");
    Ok(speed)
}

fn unique_path(directory: &Path, filename: &str) -> PathBuf {
    let initial = directory.join(filename);
    if !initial.exists() {
        return initial;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().map(|ext| ext.to_string_lossy());
    for index in 1.. {
        let candidate = match &extension {
            Some(ext) => directory.join(format!("{stem} ({index}).{ext}")),
            None => directory.join(format!("{stem} ({index})")),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn safe_filename(filename: &str) -> String {
    let sanitized = filename
        .chars()
        .map(|ch| if ch.is_control() || ch == '/' || ch == '\\' { '_' } else { ch })
        .collect::<String>();
    let trimmed = sanitized.trim_matches(['.', ' ']);
    if trimmed.is_empty() {
        "download".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn file_stem(path: &Path) -> Result<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .context("Source file has no usable filename")
}

fn strings<const N: usize>(values: [String; N]) -> Vec<String> {
    values.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        io::{BufRead, BufReader},
        net::TcpListener,
    };

    #[test]
    fn parses_speed_limits() {
        assert_eq!(parse_speed("500K").unwrap(), 500 * 1024);
        assert_eq!(parse_speed("5M").unwrap(), 5 * 1024 * 1024);
        assert!(parse_speed("0").is_err());
        assert!(parse_speed("fast").is_err());
    }

    #[test]
    fn expands_destination_shortcuts() {
        let home = Path::new("/home/test");
        assert_eq!(shortcut_path(home, Path::new("dz")), home.join("Downloads"));
        assert_eq!(
            shortcut_path(home, Path::new("dc/work")),
            home.join("Documents/work")
        );
        assert_eq!(
            shortcut_path(home, Path::new("/tmp/output")),
            PathBuf::from("/tmp/output")
        );
    }

    #[test]
    fn sanitizes_download_names() {
        assert_eq!(safe_filename("../../hello.mp4"), "_.._hello.mp4");
        assert_eq!(safe_filename(""), "download");
    }

    #[test]
    fn validates_api_setting_paths() {
        assert!(validate_setting_path("extractor.site.api-key").is_ok());
        assert!(validate_setting_path("extractor.site").is_err());
        assert!(validate_setting_path("downloader.output.api-key").is_err());
        assert!(validate_setting_path("extractor.site.api key").is_err());
    }

    #[test]
    fn app_settings_round_trip_storage_path_and_browser() {
        let settings = AppSettings {
            api_key_store_path: Some(PathBuf::from("/tmp/craftingtable/keys.json")),
            default_browser: Some("firefox".to_owned()),
        };
        let encoded = serde_json::to_vec(&settings).unwrap();
        let decoded: AppSettings = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded.api_key_store_path,
            Some(PathBuf::from("/tmp/craftingtable/keys.json"))
        );
        assert_eq!(decoded.default_browser.as_deref(), Some("firefox"));
    }

    #[test]
    fn gallery_cookie_browser_option_uses_configured_browser() {
        let mut args = Vec::new();
        append_cookie_browser_args(&mut args, Some("firefox")).unwrap();
        assert_eq!(
            args,
            ["--cookies-from-browser".to_owned(), "firefox".to_owned()]
        );

        args.clear();
        append_cookie_browser_args(&mut args, Some("none")).unwrap();
        assert!(args.is_empty());
        assert!(append_cookie_browser_args(&mut args, Some("unsupported")).is_err());
    }

    #[test]
    fn parses_public_api_catalog_entries_from_index() {
        let markdown = r#"
## APILayer APIs
| [Ignore](https://ignore.example) | not in index | apiKey |
## Index
### Animals
API | Description | Auth | HTTPS | CORS
|:---|:---|:---|:---|:---|
| [Cats](https://cats.example/docs) | Pictures and facts | `apiKey` | Yes | No |
### Games
| [Board Games](http://games.example/api) | Board game data | No | Yes | Yes |
## License
### Ignored
| [Ignore too](https://ignore.example) | outside index | No |
"#;
        let entries = parse_public_api_catalog(markdown).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "Cats");
        assert_eq!(entries[0].category, "Animals");
        assert_eq!(entries[0].auth, "`apiKey`");
        assert_eq!(
            entries[0].documentation_url.as_str(),
            "https://cats.example/docs"
        );
        assert_eq!(entries[1].category, "Games");
    }

    #[test]
    fn ignores_non_http_links_in_public_api_catalog() {
        assert!(
            parse_public_api_row(
                "| [Unsafe](javascript:alert(1)) | test | No | Yes | Yes |",
                "Testing"
            )
            .is_none()
        );
    }

    #[test]
    fn identifies_equivalent_settings_and_key_store_paths() {
        let directory = tempfile::tempdir().unwrap();
        let settings = directory.path().join("settings.json");
        let alias = directory.path().join("sub/../settings.json");
        assert!(paths_refer_to_same_file(&settings, &alias).unwrap());
    }

    #[test]
    fn normalizes_and_validates_source_hosts() {
        assert_eq!(normalize_host("Media.Example.").unwrap(), "media.example");
        assert!(normalize_host("media.example/path").is_err());
        assert!(normalize_host("user@media.example").is_err());
    }

    #[test]
    fn api_keys_apply_only_to_their_host_or_subdomains() {
        let store = ApiKeyStore {
            sources: HashMap::from([(
                "example.com".to_owned(),
                HashMap::from([(
                    "extractor.sample.api-key".to_owned(),
                    "secret".to_owned(),
                )]),
            )]),
        };
        let file = matching_api_key_config(&store, "media.example.com")
            .unwrap()
            .unwrap();
        let config: Value = serde_json::from_reader(File::open(file.path()).unwrap()).unwrap();
        assert_eq!(
            config["extractor"]["sample"]["api-key"],
            Value::String("secret".to_owned())
        );
        assert!(matching_api_key_config(&store, "example.com.attacker.test")
            .unwrap()
            .is_none());
    }

    #[test]
    fn downloads_ranged_response_in_parallel() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let data = b"ok";
            for _ in 0..3 {
                let (mut stream, _) = listener.accept().unwrap();
                let range = {
                    let mut reader = BufReader::new(&mut stream);
                    let mut range = None;
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("range: bytes=") {
                            let value = line.split_once(':').unwrap().1.trim();
                            let value = value.strip_prefix("bytes=").unwrap();
                            let (start, end) = value.split_once('-').unwrap();
                            range = Some((start.parse::<usize>().unwrap(), end.parse::<usize>().unwrap()));
                        }
                    }
                    range.unwrap()
                };
                let (start, end) = range;
                let body = &data[start..=end];
                write!(
                    stream,
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    data.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("download");
        let url = Url::parse(&format!("http://{address}/asset")).unwrap();
        download_parallel(&url, &output, None).unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(output).unwrap(), b"ok");
    }
}
