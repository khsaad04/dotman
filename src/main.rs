mod cli;
mod colors;
mod error;

use indexmap::IndexMap;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs, io,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::exit,
};

#[derive(Debug, Deserialize)]
struct Manifest {
    wallpaper: Option<PathBuf>,
    theme: Option<String>,
    files: IndexMap<String, File>,
}

#[derive(Debug, Deserialize)]
struct File {
    target: PathBuf,
    dest: PathBuf,
    template: Option<PathBuf>,
}

type VarMap = HashMap<String, String>;

impl TryFrom<&Path> for Manifest {
    type Error = error::Error;
    fn try_from(value: &Path) -> std::result::Result<Self, Self::Error> {
        let path = value
            .canonicalize()
            .map_err(|err| format!("invalid path {}: {err}", value.display()))?;
        let parent_dir = path
            .parent()
            .ok_or(format!("could not access parent dir of {}", path.display()))?;
        std::env::set_current_dir(parent_dir).map_err(|err| {
            format!(
                "could not change directory to {}: {err}",
                parent_dir.display()
            )
        })?;
        let manifest: Manifest = toml::from_str(
            &fs::read_to_string(&path)
                .map_err(|err| format!("could not read file {}: {err}", path.display()))?,
        )
        .map_err(|err| format!("could not parse toml {}: {err}", path.display()))?;
        Ok(manifest)
    }
}

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
    if let Err(err) = exec_subcommand() {
        eprintln!("\x1b[0;31mERROR\x1b[0m: {err}");
        exit(1);
    }
}

fn exec_subcommand() -> error::Result<()> {
    let args = cli::Cli::try_parse()?;

    let mut config: VarMap = HashMap::new();
    let manifest = Manifest::try_from(args.manifest_path.as_path())?;

    let mut template_engine = upon::Engine::new();
    template_engine.add_filter("is_equal", |s: &str, other: &str| -> bool { s == other });

    match args.subcommand {
        cli::SubCommand::Sync { force, name } => {
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    symlink_files(file, force).map_err(|err| {
                        format!("something went wrong while symlinking {name}:\n    {err}")
                    })?;
                    if file.template.is_some() {
                        create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
                        generate_template(file, &config, &mut template_engine).map_err(|err| {
                            format!("something went wrong while generating {name}:\n    {err}")
                        })?;
                    }
                } else {
                    return Err(format!("could not find {name}").into());
                }
            } else {
                create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
                for (name, file) in manifest.files.iter() {
                    symlink_files(file, force).map_err(|err| {
                        format!("something went wrong while symlinking {name}:\n    {err}")
                    })?;
                    if file.template.is_some() {
                        generate_template(file, &config, &mut template_engine).map_err(|err| {
                            format!("something went wrong while generating {name}:\n    {err}")
                        })?;
                    }
                }
            }
        }
        cli::SubCommand::Link { force, name } => {
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    symlink_files(file, force).map_err(|err| {
                        format!("something went wrong while symlinking {name}:\n    {err}")
                    })?;
                } else {
                    return Err(format!("could not find {}", &name).into());
                }
            } else {
                for (name, file) in manifest.files.iter() {
                    symlink_files(file, force).map_err(|err| {
                        format!("something went wrong while symlinking {name}:\n    {err}")
                    })?;
                }
            }
        }
        cli::SubCommand::Generate { name } => {
            if let Some(name) = name {
                if let Some(file) = manifest.files.get(&name) {
                    if file.template.is_some() {
                        create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
                        generate_template(file, &config, &mut template_engine).map_err(|err| {
                            format!("something went wrong while generating {name}:\n    {err}")
                        })?;
                    }
                } else {
                    return Err(format!("could not find {}", &name).into());
                }
            } else {
                create_color_palette(&manifest.wallpaper, &mut config, &manifest)?;
                for (name, file) in manifest.files.iter() {
                    if file.template.is_some() {
                        generate_template(file, &config, &mut template_engine).map_err(|err| {
                            format!("something went wrong while generating {name}:\n    {err}")
                        })?;
                    }
                }
            }
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
        let wp_path = wallpaper
            .canonicalize()
            .map_err(|err| format!("could not find {}: {err}", wallpaper.display()))?;
        config.insert("wallpaper".to_string(), wp_path.display().to_string());
        let mut theme = "dark";
        if let Some(theme_pref) = &manifest.theme {
            theme = theme_pref;
        }
        colors::generate_material_colors(&wp_path, theme, config)?;
    } else if has_templates(manifest) {
        return Err("could not generate color palette: wallpaper is not set.".into());
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
            &dest_path.join(target_path.file_name().ok_or(format!(
                "could not extract file_name of {}",
                target_path.display()
            ))?),
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
            .ok_or("invalid Unicode in Path")?
            .replace('~', &home_dir)
            .replace("$HOME", &home_dir),
    );
    Ok(PathBuf::from(result))
}

fn symlink_dir_all(target: &Path, dest: &Path, force: bool) -> error::Result<()> {
    if target.is_dir() {
        for entry in fs::read_dir(target)? {
            let entry = entry?;
            let dest = &dest.join(entry.path().file_name().ok_or(format!(
                "could not extract file_name of {}",
                entry.path().display()
            ))?);
            let dest_parent_dir = dest
                .parent()
                .ok_or(format!("could not access parent dir of {}", dest.display()))?;
            if !dest_parent_dir.exists() {
                fs::create_dir_all(dest_parent_dir).map_err(|err| {
                    format!("could not create dir {}: {err}", dest_parent_dir.display())
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
                        format!("could not remove file {}: {err}", dest.display())
                    })?;
                    symlink(target, dest)?;
                    log!(Info, "Symlinked {} to {}", target.display(), dest.display());
                } else if dest.is_symlink() {
                    if !dest.exists() {
                        log!(
                            Warning,
                            "Destination {} is a broken symlink. Ignoring",
                            dest.display()
                        );
                        std::fs::remove_file(dest).map_err(|err| {
                            format!("could not remove file {}: {err}", dest.display())
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
                    "could not symlink {} to {}: {err}",
                    target.display(),
                    dest.display()
                )
                .into());
            }
        },
    }
    Ok(())
}

fn generate_template(
    file: &File,
    config: &VarMap,
    template_engine: &mut upon::Engine,
) -> error::Result<()> {
    if let Some(template_path) = &file.template {
        let template_path = template_path
            .canonicalize()
            .map_err(|err| format!("could not find {}: {err}", template_path.display()))?;
        let data = fs::read_to_string(&template_path)
            .map_err(|err| format!("could not read file {}: {err}", template_path.display()))?;

        let rendered = template_engine
            .compile(&data)
            .map_err(|err| {
                format!(
                    "could not compile template {}: {err}",
                    template_path.display()
                )
            })?
            .render(template_engine, config)
            .to_string()
            .map_err(|err| {
                format!(
                    "could not render template {}: {err}",
                    template_path.display()
                )
            })?;

        fs::write(&file.target, rendered)
            .map_err(|err| format!("could not write to {}: {err}", file.target.display()))?;
        log!(Info, "Generated template {}", template_path.display());
    }
    Ok(())
}
