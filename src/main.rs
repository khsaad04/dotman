mod colors;
mod error;

use indexmap::IndexMap;
use serde::Deserialize;
use std::{
    collections::HashMap,
    env::Args,
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::exit,
};

#[derive(Debug, Deserialize)]
struct File {
    target: PathBuf,
    dest: PathBuf,
    template: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    wallpaper: Option<PathBuf>,
    theme: Option<String>,
    files: IndexMap<String, File>,
}

impl TryFrom<&Path> for Manifest {
    type Error = error::Error;
    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        let manifest_path = value
            .canonicalize()
            .map_err(|err| format!("could not find {}: {}", &value.display(), err))?;
        let manifest_parent_dir = manifest_path.parent().unwrap();
        std::env::set_current_dir(manifest_parent_dir).map_err(|err| {
            format!(
                "could not change directory to {}: {}",
                &manifest_parent_dir.display(),
                err
            )
        })?;
        let manifest: Manifest =
            toml::from_str(&fs::read_to_string(&manifest_path).map_err(|err| {
                format!("could not read file {}: {}", &manifest_path.display(), err)
            })?)
            .map_err(|err| format!("could not parse toml {}: {}", &manifest_path.display(), err))?;
        Ok(manifest)
    }
}

type VarMap = HashMap<String, String>;

const USAGE: &str = "Usage: dotman [OPTION] <SUBCOMMAND>

Options:
  -m, --manifest <PATH>  custom path to manifest file [default: Manifest.toml]
  -h, --help             show this help message

Subcommands:
  sync      [-f | --force] [NAME] symlink files and generate templates 
  link      [-f | --force] [NAME] symlink files
  generate  [NAME] generate templates";

enum LogLevel {
    Info,
    Warning,
}

macro_rules! log {
    ($loglevel:ident, $($arg:tt)*) => {
        match LogLevel::$loglevel {
            LogLevel::Info => {
                print!("\x1b[0;32mINFO\x1b[0m: ");
            }
            LogLevel::Warning => {
                print!("\x1b[0;33mWARNING\x1b[0m: ");
            }
        }
        println!($($arg)*);
    };
}

fn main() {
    let mut args = std::env::args();
    let _program_name = args.next();

    if let Err(err) = parse_arguments(&mut args) {
        eprintln!("{err}");
        exit(1);
    }
}

fn parse_arguments(args: &mut Args) -> error::Result<()> {
    let mut config: VarMap = HashMap::new();
    let mut manifest_path = "Manifest.toml".to_string();
    let mut arg = args
        .next()
        .ok_or(format!("Subcommand not found.\n{USAGE}"))?;
    if arg.starts_with('-') {
        match arg.as_str() {
            "-m" | "--manifest" => {
                let path = args.next();
                if let Some(path) = path {
                    manifest_path = path;
                } else {
                    return Err(format!("Please provide path to manifest file.\n{USAGE}").into());
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => {
                return Err(format!("flag {arg} not found.\n{USAGE}").into());
            }
        }
        arg = args
            .next()
            .ok_or(format!("Subcommand not found.\n{USAGE}"))?;
    }
    let manifest = Manifest::try_from(Path::new(&manifest_path))?;
    let mut force = false;
    let mut name: Option<String> = None;
    match arg.as_str() {
        "sync" => {
            if let Some(arg) = args.next() {
                if arg.starts_with('-') {
                    match arg.as_str() {
                        "-f" | "--force" => {
                            force = true;
                        }
                        "-h" | "--help" => {
                            println!("{USAGE}");
                            return Ok(());
                        }
                        _ => {
                            return Err(format!("flag {arg} not found.\n{USAGE}").into());
                        }
                    }
                    name = args.next();
                } else {
                    name = Some(arg);
                }
            }
            create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    symlink_files(file, force)?;
                    if file.template.is_some() {
                        generate_template(file, &config)?;
                    }
                } else {
                    return Err(format!("could not find {}", &name).into());
                }
            } else {
                for (_, file) in manifest.files.iter() {
                    symlink_files(file, force)?;
                    if file.template.is_some() {
                        generate_template(file, &config)?;
                    }
                }
            }
        }
        "link" => {
            if let Some(arg) = args.next() {
                if arg.starts_with('-') {
                    match arg.as_str() {
                        "-f" | "--force" => {
                            force = true;
                        }
                        "-h" | "--help" => {
                            println!("{USAGE}");
                            return Ok(());
                        }
                        _ => {
                            return Err(format!("flag {arg} not found.\n{USAGE}").into());
                        }
                    }
                    name = args.next();
                } else {
                    name = Some(arg);
                }
            }
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    symlink_files(file, force)?;
                } else {
                    return Err(format!("could not find {}", &name).into());
                }
            } else {
                for (_, file) in manifest.files.iter() {
                    symlink_files(file, force)?;
                }
            }
        }
        "generate" => {
            if let Some(arg) = args.next() {
                if arg.starts_with('-') {
                    match arg.as_str() {
                        "-h" | "--help" => {
                            println!("{USAGE}");
                            return Ok(());
                        }
                        _ => {
                            return Err(format!("flag {arg} not found.\n{USAGE}").into());
                        }
                    }
                } else {
                    name = Some(arg);
                }
            }
            create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    if file.template.is_some() {
                        generate_template(file, &config)?;
                    }
                } else {
                    return Err(format!("could not find {}", &name).into());
                }
            } else {
                for (_, file) in manifest.files.iter() {
                    if file.template.is_some() {
                        generate_template(file, &config)?;
                    }
                }
            }
        }
        _ => {
            return Err(format!("subcommand {arg} not found.\n{USAGE}").into());
        }
    }
    Ok(())
}

fn create_color_palette(
    path: &Option<PathBuf>,
    config: &mut VarMap,
    manifest: &Manifest,
) -> error::Result<()> {
    if let Some(wallpaper) = path {
        let wp_path = PathBuf::from(&wallpaper)
            .canonicalize()
            .map_err(|err| format!("could not find {}: {}", wallpaper.display(), err))?;
        config.insert("wallpaper".to_string(), wp_path.display().to_string());
        let theme = manifest.theme.clone().unwrap_or("dark".to_string());
        colors::generate_material_colors(&wp_path, &theme, config)?;
    } else if has_templates(manifest) {
        return Err("could not generate color palette: `wallpaper` is not set.".into());
    } else {
        log!(Warning, "Skipping color scheme generation.");
    }
    Ok(())
}

fn has_templates(manifest: &Manifest) -> bool {
    for (_, file) in manifest.files.iter() {
        if file.template.is_some() {
            return true;
        }
    }
    false
}

fn symlink_files(file: &File, force: bool) -> error::Result<()> {
    let target_path = resolve_home_dir(&file.target)?.canonicalize()?;
    let dest_path = resolve_home_dir(&file.dest)?;
    if dest_path.is_dir() {
        symlink_dir_all(
            &target_path,
            &dest_path.join(target_path.file_name().unwrap()),
            force,
        )?;
    } else {
        symlink_dir_all(&target_path, &dest_path, force)?;
    };
    Ok(())
}

fn resolve_home_dir(path: &Path) -> error::Result<PathBuf> {
    let mut result = String::new();
    let home_dir =
        std::env::var("HOME").map_err(|err| format!("could not find home directory: {err}"))?;
    result.push_str(
        &path
            .to_str()
            .unwrap()
            .replace('~', &home_dir)
            .replace("$HOME", &home_dir),
    );
    Ok(PathBuf::from(result))
}

fn symlink_dir_all(target: &Path, dest: &Path, force: bool) -> error::Result<()> {
    if target.is_dir() {
        for entry in fs::read_dir(target)? {
            let entry = entry?;
            let dest = &dest.join(entry.path().file_name().unwrap());
            let dest_parent_dir = dest.parent().unwrap();
            if !dest_parent_dir.exists() {
                fs::create_dir_all(dest_parent_dir).map_err(|err| {
                    format!(
                        "could not create dir {}: {}",
                        &dest_parent_dir.display(),
                        err
                    )
                })?;
            }
            symlink_dir_all(&entry.path(), dest, force)?;
        }
    } else {
        symlink_file(target, dest, force)?;
    }
    Ok(())
}

fn symlink_file(target: &Path, dest: &Path, force: bool) -> error::Result<()> {
    match symlink(target, dest) {
        Ok(()) => {
            log!(Info, "Symlinked {} to {}", target.display(), dest.display());
        }
        Err(err) => match err.kind() {
            io::ErrorKind::AlreadyExists => {
                if force {
                    log!(
                        Warning,
                        "Destination {} already exists. Removing",
                        dest.display()
                    );
                    std::fs::remove_file(dest).map_err(|err| {
                        format!("could not remove file {}: {}", &dest.display(), err)
                    })?;
                    symlink(target, dest)?;
                    log!(Info, "Symlinked {} to {}", target.display(), dest.display());
                } else if dest.is_symlink() {
                    if !dest.exists() {
                        log!(Warning, "Destination is a broken symlink. Ignoring",);
                        std::fs::remove_file(dest).map_err(|err| {
                            format!("could not remove file {}: {}", &dest.display(), err)
                        })?;
                        symlink(target, dest)?;
                        log!(Info, "Symlinked {} to {}", target.display(), dest.display());
                    } else {
                        let symlink_origin = dest.canonicalize()?;
                        if target.canonicalize()? == symlink_origin {
                            log!(Info, "Skipped symlinking {}. Up to date.", dest.display());
                        } else {
                            log!(
                                Warning,
                                "Destination {} is symlinked to {}. Resolve manually.",
                                dest.display(),
                                symlink_origin.display()
                            );
                        }
                    }
                } else {
                    log!(
                        Warning,
                        "Destination {} exists but it's not a symlink. Resolve manually",
                        dest.display()
                    );
                }
            }
            _ => {
                return Err(format!(
                    "could not symlink {} to {}: {}",
                    &target.display(),
                    &dest.display(),
                    err
                )
                .into());
            }
        },
    }
    Ok(())
}

fn generate_template(file: &File, config: &VarMap) -> error::Result<()> {
    let target_path = PathBuf::from(&file.target).canonicalize().map_err(|err| {
        format!(
            "cannot generate template into {}: {}",
            &file.target.display(),
            err
        )
    })?;
    if let Some(template_path) = &file.template {
        let template_path = PathBuf::from(template_path);
        if template_path.exists() {
            let data = fs::read_to_string(&template_path).map_err(|err| {
                format!("could not read file {}: {}", &template_path.display(), err)
            })?;

            let mut engine = upon::Engine::new();
            engine
                .add_template(template_path.to_str().unwrap(), &data)
                .map_err(|err| {
                    format!(
                        "could not add template {}: {}",
                        &template_path.display(),
                        err
                    )
                })?;
            let rendered = engine
                .template(template_path.to_str().unwrap())
                .render(config)
                .to_string()
                .map_err(|err| {
                    format!(
                        "could not render template {}: {}",
                        &template_path.display(),
                        err
                    )
                })?;

            fs::write(&target_path, rendered).map_err(|err| {
                format!(
                    "could not write to file {}: {}",
                    &target_path.display(),
                    err
                )
            })?;
            log!(Info, "Generated template {}", template_path.display());
        } else {
            return Err(format!("could not find template {}", &template_path.display()).into());
        }
    }
    Ok(())
}
