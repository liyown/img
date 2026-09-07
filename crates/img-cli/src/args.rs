use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "img",
    version,
    about = "Upload images from files, screenshots or links. Rust CLI included with img GUI.",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[arg(long, global = true, help = "Use this global configuration file")]
    pub config: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Subcommand)]
pub enum Command {
    /// Upload local files or remote image URLs
    Upload(Upload),
    /// Download an image URL without uploading it
    Fetch(Fetch),
    /// Capture and upload a screenshot (copies result by default)
    Screenshot(Screenshot),
    /// Run a PicGo-compatible editor upload server
    Serve(Serve),
    /// Upload image references and rewrite Markdown documents
    Rewrite(Rewrite),
    /// Inspect image dimensions, type and EXIF presence
    Info(Info),
    /// Configure a storage provider interactively or with flags
    Init(Box<Init>),
    /// List, show, select, remove or test storage providers
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Inspect or change configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Print a shell completion script
    Completion {
        #[arg(default_value = "bash")]
        shell: clap_complete::Shell,
    },
    /// Print application version
    Version,
    /// Add the bundled CLI to a directory on PATH (no GUI required to run it)
    InstallCli {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}
#[derive(Args, Clone, Default)]
pub struct Processing {
    #[arg(long, default_value = "", help = "Storage provider name")]
    pub provider: String,
    #[arg(long, default_value = "", help = "Remote path prefix")]
    pub path: String,
    #[arg(long)]
    pub overwrite: bool,
    #[arg(long, help = "Compress images before uploading")]
    pub optimize: bool,
    #[arg(long, help = "Remove JPEG EXIF metadata, preserving orientation")]
    pub strip_exif: bool,
    #[arg(long,default_value_t=0,value_parser=clap::value_parser!(u32).range(0..=32768))]
    pub resize: u32,
    #[arg(long, help = "Allow trusted plain HTTP image sources")]
    pub allow_insecure: bool,
}
impl Processing {
    pub fn options(&self) -> img_core::upload::Options {
        img_core::upload::Options {
            path: self.path.clone(),
            overwrite: self.overwrite,
            optimize: self.optimize,
            strip_exif: self.strip_exif,
            max_width: self.resize,
            allow_insecure: self.allow_insecure,
            ..Default::default()
        }
    }
}
#[derive(Args)]
pub struct Upload {
    #[command(flatten)]
    pub processing: Processing,
    #[arg(required=true,num_args=1..)]
    pub files: Vec<String>,
    #[arg(long,value_parser=["url","markdown","html","json"])]
    pub format: Option<String>,
    #[arg(long)]
    pub copy: bool,
    #[arg(long)]
    pub no_copy: bool,
    #[arg(long)]
    pub quiet: bool,
    #[arg(long)]
    pub verbose: bool,
    #[arg(long, default_value = "")]
    pub name: String,
    #[arg(long, help = "Write JSON progress events to stderr (one file only)")]
    pub progress: bool,
}
#[derive(Args)]
pub struct Fetch {
    pub url: String,
    #[arg(long)]
    pub output: PathBuf,
    #[arg(long,default_value_t=8<<20,value_parser=clap::value_parser!(u64).range(1..=128<<20))]
    pub max_size: u64,
    #[arg(long)]
    pub allow_insecure: bool,
}
#[derive(Args)]
pub struct Screenshot {
    /// Save the capture locally without uploading or copying a link
    #[arg(long)]
    pub output: Option<std::path::PathBuf>,
    #[command(flatten)]
    pub processing: Processing,
    #[arg(long, conflicts_with = "window")]
    pub region: bool,
    #[arg(long)]
    pub window: bool,
    #[arg(long,value_parser=["url","markdown","html","json"])]
    pub format: Option<String>,
    #[arg(long)]
    pub no_copy: bool,
    #[arg(long)]
    pub verbose: bool,
}
#[derive(Args)]
pub struct Serve {
    #[command(flatten)]
    pub processing: Processing,
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: String,
    #[arg(long, default_value_t = 36677)]
    pub port: u16,
}
#[derive(Args)]
pub struct Rewrite {
    #[command(flatten)]
    pub processing: Processing,
    pub files: Vec<PathBuf>,
    #[arg(long)]
    pub stdout: bool,
}
#[derive(Args)]
pub struct Info {
    #[arg(required=true,num_args=1..)]
    pub files: Vec<PathBuf>,
    #[arg(long,default_value="text",value_parser=["text","json"])]
    pub format: String,
}
#[derive(Args, Default)]
pub struct Init {
    #[arg(long = "type", default_value = "")]
    pub kind: String,
    #[arg(long, default_value = "")]
    pub name: String,
    #[arg(long, default_value = "")]
    pub url: String,
    #[arg(long, default_value = "data.url")]
    pub url_json_path: String,
    #[arg(long,default_value="POST",value_parser=["POST","PUT","PATCH"])]
    pub method: String,
    #[arg(long, default_value = "file")]
    pub file_field: String,
    #[arg(long, default_value = "")]
    pub endpoint: String,
    #[arg(long, default_value = "auto")]
    pub region: String,
    #[arg(long, default_value = "")]
    pub bucket: String,
    #[arg(long, default_value = "")]
    pub access_key: String,
    #[arg(long, default_value = "")]
    pub secret_key: String,
    #[arg(long, default_value = "")]
    pub session_token: String,
    #[arg(long, default_value = "")]
    pub public_url: String,
    #[arg(long)]
    pub path_style: bool,
    #[arg(long)]
    pub allow_insecure: bool,
    #[arg(long, default_value = "")]
    pub owner: String,
    #[arg(long, default_value = "")]
    pub repo: String,
    #[arg(long, default_value = "main")]
    pub branch: String,
    #[arg(long, default_value = "")]
    pub token: String,
    #[arg(long, default_value = "upload: {path}")]
    pub commit_message: String,
}
#[derive(Subcommand)]
pub enum ProviderCommand {
    List,
    Show { name: String },
    Use { name: String },
    Remove { name: String },
    Test { name: String },
}
#[derive(Subcommand)]
pub enum ConfigCommand {
    Path,
    List,
    Validate,
    Get { key: String },
    Set { key: String, value: String },
    Unset { key: String },
}
