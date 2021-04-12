use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
#[structopt(about = "Toy payment processor")]
#[structopt(global_setting = structopt::clap::AppSettings::ColoredHelp)]
#[structopt(global_setting = structopt::clap::AppSettings::InferSubcommands)]
pub struct Opts {
    // An input file to process
    pub input_cvs: PathBuf,
}
