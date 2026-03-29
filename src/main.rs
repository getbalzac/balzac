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
        )
        .subcommand(
            clap::command!("dev")
                .about("Serve project locally with targeted page reloads")
                .arg(
                    clap::arg!(--root <PATH>)
                        .value_parser(clap::value_parser!(PathBuf))
                        .required(false),
                )
                .arg(
                    clap::arg!(--host <HOST>)
                        .value_parser(clap::builder::NonEmptyStringValueParser::new())
                        .default_value("127.0.0.1"),
                )
                .arg(
                    clap::arg!(--port <PORT>)
                        .value_parser(clap::value_parser!(u16))
                        .default_value("4000"),
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
        Some(("dev", sub_matches)) => {
            let path = get_path_arg(sub_matches, "root");
            let host = sub_matches
                .get_one::<String>("host")
                .expect("host should always have a default")
                .clone();
            let port = *sub_matches
                .get_one::<u16>("port")
                .expect("port should always have a default");
            cli::dev(&path, &host, port);
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
