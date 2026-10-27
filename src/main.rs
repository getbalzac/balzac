use std::path::PathBuf;

use balzac::cli;
use balzac::config::InitFeature;

fn main() {
    colog::init();

    let matches = command().get_matches();

    match matches.subcommand() {
        Some(("init", sub_matches)) => {
            let path = get_path_arg(sub_matches, "path");
            let mut features = Vec::new();
            if sub_matches.get_flag("sitemap") {
                features.push(InitFeature::Sitemap);
            }
            if sub_matches.get_flag("feed") {
                features.push(InitFeature::Feed);
            }
            cli::init(&path, &features);
        }
        Some(("build", sub_matches)) => {
            let path = get_path_arg(sub_matches, "root");
            cli::build(&path);
        }
        Some(("clean", sub_matches)) => {
            let path = get_path_arg(sub_matches, "root");
            cli::clean(&path);
        }
        Some(("new", sub_matches)) => {
            let path = get_path_arg(sub_matches, "root");
            let collection = sub_matches
                .get_one::<String>("COLLECTION")
                .expect("collection is required");
            let title = sub_matches
                .get_one::<String>("TITLE")
                .expect("title is required");
            cli::new_content(&path, collection, title);
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
            eprintln!("Error: Could not determine current directory: {e}");
            std::process::exit(1);
        }),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn command() -> clap::Command {
    clap::Command::new("balzac")
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
                )
                .arg(
                    clap::arg!(--feed)
                        .help("Include RSS/Atom feed configuration")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            clap::command!("clean")
                .about("Remove the output directory")
                .arg(
                    clap::arg!(--root <PATH>)
                        .value_parser(clap::value_parser!(PathBuf))
                        .required(false),
                ),
        )
        .subcommand(
            clap::command!("new")
                .about("Create a draft markdown file in a collection")
                .arg(clap::arg!(<COLLECTION> "Collection to add the file to, e.g. posts"))
                .arg(clap::arg!(<TITLE> "Title of the new item; the file name is derived from it"))
                .arg(
                    clap::arg!(--root <PATH>)
                        .value_parser(clap::value_parser!(PathBuf))
                        .required(false),
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
        )
}
