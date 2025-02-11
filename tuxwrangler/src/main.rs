use std::{
    path::{Path, PathBuf},
    process::exit,
};

use clap::Parser;
use log::{error, info};
use serde_json::json;
use tw_config::{build_images, load_lockfile, update_lock, write_dockerfile, Clients};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(long = "log-level")]
    log_level: Option<log::LevelFilter>,

    #[clap(long = "github-token", env = "GITHUB_TOKEN")]
    github_token: Option<String>,

    #[clap(subcommand)]
    command: Command,
}

#[derive(Parser, Debug)]
enum Command {
    Build(BuildArgs),
    Update(UpdateArgs),
    Write(WriteArgs),
    Images(ImagesArgs),
}

#[derive(Parser, Debug)]
struct BuildArgs {
    #[clap(long, short)]
    #[arg( default_value = default_config("lock").into_os_string())]
    lock: PathBuf,
    #[clap(long = "skip-tags")]
    skip_tags: bool,
}

#[derive(Parser, Debug)]
struct WriteArgs {
    #[clap(long, short)]
    #[arg( default_value = default_config("lock").into_os_string())]
    lock: PathBuf,
    #[clap( default_value = default_dir().into_os_string(), long = "out")]
    out_dir: PathBuf,
}

#[derive(Parser, Debug)]
struct ImagesArgs {
    #[clap(long, short)]
    #[arg( default_value = default_config("lock").into_os_string())]
    lock: PathBuf,
}

#[derive(Parser, Debug)]
struct UpdateArgs {
    #[clap(long, short)]
    #[arg( default_value = default_config("toml").into_os_string())]
    config: PathBuf,
    #[clap(long, short)]
    #[arg( default_value = default_config("lock").into_os_string())]
    lock: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    env_logger::builder()
        .filter_level(args.log_level.unwrap_or(log::LevelFilter::Info))
        .filter_module("hyper_util::client::legacy::pool", log::LevelFilter::Error)
        .filter_module("rustls::client::hs", log::LevelFilter::Error)
        .filter_module("rustls::client::tls13", log::LevelFilter::Error)
        .filter_module("hyper_rustls::config", log::LevelFilter::Error)
        .filter_module("tower::buffer::worker", log::LevelFilter::Error)
        .filter_module(
            "hyper_util::client::legacy::connect::dns",
            log::LevelFilter::Error,
        )
        .filter_module(
            "hyper_util::client::legacy::connect::http",
            log::LevelFilter::Error,
        )
        .filter_module(
            "octocrab",
            if args.log_level == Some(log::LevelFilter::Debug) {
                log::LevelFilter::Info
            } else {
                args.log_level.unwrap_or(log::LevelFilter::Info)
            },
        )
        .init();
    let mut clients = match Clients::new(args.github_token) {
        Ok(c) => c,
        Err(e) => {
            error!("Unable to create an TuxWrangler client:\n{:?}", e);
            exit(1)
        }
    };

    match args.command {
        Command::Build(build_args) => {
            let locked = match load_lockfile(build_args.lock) {
                Ok(locked) => locked,
                Err(e) => {
                    error!("Unable to load lock file:\n{:?}", e);
                    exit(1)
                }
            };
            match build_images(&clients, locked, build_args.skip_tags).await {
                Ok(_) => info!("Images build successfully"),
                Err(e) => {
                    error!("Unable to build images:\n{:?}", e);
                    exit(1)
                }
            }
        }
        Command::Update(update_args) => {
            match update_lock(&mut clients, update_args.config, update_args.lock).await {
                Ok(_) => info!("Lockfile updated successfully"),
                Err(e) => {
                    error!("Unable to update lockfile:\n{:?}", e);
                    exit(1)
                }
            }
        }
        Command::Write(write_args) => {
            let locked = match load_lockfile(write_args.lock) {
                Ok(locked) => locked,
                Err(e) => {
                    error!("Unable to load lock file:\n{:?}", e);
                    exit(1)
                }
            };
            match write_dockerfile(locked, &write_args.out_dir) {
                Ok(_) => info!(
                    "Dockerfile written to '{}'",
                    write_args.out_dir.join("Dockerfile").display()
                ),
                Err(e) => {
                    error!("Unable to write Dockerfile:\n{:?}", e);
                    exit(1)
                }
            }
        }
        Command::Images(image_args) => {
            let locked = match load_lockfile(image_args.lock) {
                Ok(locked) => locked,
                Err(e) => {
                    error!("Unable to load lock file:\n{:?}", e);
                    exit(1)
                }
            };
            println!(
                "images={}", serde_json::to_string(&json!(locked
                    .builds
                    .iter()
                    .map(|build| json!({"target": &build.target, "image_name": &build.image_name, "image_tag": &build.image_tag}))
                    .collect::<Vec<_>>())).expect("Images contained invalid json.")
            );
        }
    };
}

fn default_config(extension: &str) -> PathBuf {
    Path::new("WRANGLER").with_extension(extension)
}

fn default_dir() -> PathBuf {
    Path::new("build").to_path_buf()
}
