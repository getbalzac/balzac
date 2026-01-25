use std::path::PathBuf;

use balzac::cli;
use balzac::config::InitFeature;

fn main() {
    colog::init();

    let cmd = clap::Command::new("balzac")
        .bin_name("balzac")
        .subcommand_required(true)
        .subcommand(
            clap::command!("build")
                .about("Build project using balzac")
                .arg(
                    clap::arg!(--root <PATH>)
                        .value_parser(clap::value_parser!(PathBuf))
                        .required(false),
                ),
        )
        .subcommand(
            clap::command!("init")
                .about("Initialize a new balzac project")
                .arg(
                    clap::arg!(--path <PATH>)
                        .value_parser(clap::value_parser!(PathBuf))
                        .required(false),
                )
                .arg(
                    clap::arg!(--sitemap)
                        .help("Include sitemap configuration")
                        .action(clap::ArgAction::SetTrue),
                ),
        );

    let matches = cmd.get_matches();

    match matches.subcommand() {
        Some(("init", sub_matches)) => {
            let path = get_path_arg(sub_matches, "path");
            let mut features = Vec::new();
            if sub_matches.get_flag("sitemap") {
                features.push(InitFeature::Sitemap);
            }
            cli::init(&path, &features);
        }
        Some(("build", sub_matches)) => {
            let path = get_path_arg(sub_matches, "root");
            cli::build(&path);
        }
        _ => unreachable!(),
    }
}

fn get_path_arg(matches: &clap::ArgMatches, name: &str) -> PathBuf {
    match matches.try_get_one::<PathBuf>(name) {
        Ok(Some(path)) => path.clone(),
        Ok(None) => std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("Error: Could not determine current directory: {}", e);
            std::process::exit(1);
        }),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
