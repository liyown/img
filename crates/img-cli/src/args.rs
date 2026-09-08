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
    /// Query the shared remote image library
    Library {
        #[command(subcommand)]
        command: LibraryCommand,
    },
    /// Browse remote files or explicitly delete one version
    Remote {
        #[arg(long, default_value = "")]
        provider: String,
        #[command(subcommand)]
        command: RemoteCommand,
    },
    /// Back up local settings and records to a new directory (never uploads)
    Backup {
        destination: PathBuf,
        #[arg(long)]
        without_config: bool,
        #[arg(long)]
        without_records: bool,
        #[arg(long)]
        include_cache: bool,
        #[arg(
            long,
            help = "Include unencrypted credentials; keep this backup private"
        )]
        include_credentials: bool,
    },
    /// Verify a backup, or restore it while img desktop is closed
    Restore {
        source: PathBuf,
        #[arg(
            long,
            help = "Apply the verified backup; keep a recovery copy of current data"
        )]
        apply: bool,
        #[arg(long)]
        include_credentials: bool,
    },
    /// Preview processing locally without uploading or modifying the source
    Process {
        file: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[command(flatten)]
        processing: Processing,
    },
    /// Restore a Markdown document from a backup created by img rewrite
    RestoreDocument { backup: PathBuf, target: PathBuf },
    /// Upload new or changed images after they settle in a directory
    Watch {
        directory: PathBuf,
        #[command(flatten)]
        processing: Processing,
        #[arg(long, default_value_t = 2, value_parser=clap::value_parser!(u64).range(1..=3600))]
        interval: u64,
        #[arg(long, help = "Ignore files already present when watching starts")]
        new_only: bool,
    },
    /// Preview or import PicGo/PicList storage configurations
    ImportConfig {
        file: PathBuf,
        #[arg(
            long,
            help = "Save supported configurations using the system credential store"
        )]
        apply: bool,
    },
    /// Verify public image URLs without sending storage credentials
    Check {
        #[arg(required = true)]
        urls: Vec<String>,
        #[arg(long)]
        allow_insecure: bool,
    },
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
#[derive(Subcommand)]
pub enum RemoteCommand {
    List {
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        cursor: Option<String>,
    },
    Delete {
        path: String,
        #[arg(long, help = "ETag or GitHub SHA returned by remote list")]
        version: String,
        #[arg(
            long,
            help = "Confirm remote deletion; existing links may stop working"
        )]
        yes: bool,
    },
}
#[derive(Args, Clone, Default)]
pub struct Processing {
    #[arg(long, value_parser=["original","web","photo"])]
    pub preset: Option<String>,
    #[arg(long, value_parser=["original","png","jpeg","webp"])]
    pub image_format: Option<String>,
    #[arg(long, value_parser=clap::value_parser!(u8).range(1..=100), help="JPEG encoding quality")]
    pub quality: Option<u8>,
    #[arg(long, value_parser=clap::value_parser!(u32).range(1..=32768))]
    pub max_edge: Option<u32>,
    #[arg(long, help = "Local image watermark placed at bottom right")]
    pub watermark: Option<PathBuf>,
    #[arg(long, value_parser=clap::value_parser!(u8).range(0..=100))]
    pub watermark_opacity: Option<u8>,
    #[arg(
        long,
        help = "Reuse a previous link for identical processed bytes in this destination"
    )]
    pub reuse: bool,
    #[arg(long, help = "Upload again instead of reusing a cached link")]
    pub force: bool,
    #[arg(long, default_value = "cli", value_parser = ["cli", "agent", "editor", "rewrite", "screenshot"], help = "Source shown in the desktop library")]
    pub origin: String,
    #[arg(long, help = "Do not retain a local library record or image copy")]
    pub no_history: bool,
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
    pub fn options_for(&self, origin: &str) -> img_core::upload::Options {
        let mut options = self.options();
        if options.record_origin.as_deref() == Some("cli") {
            options.record_origin = Some(origin.into());
        }
        options
    }
    pub fn options(&self) -> img_core::upload::Options {
        let recipe = if self.preset.is_some()
            || self.image_format.is_some()
            || self.quality.is_some()
            || self.max_edge.is_some()
            || self.watermark.is_some()
            || self.watermark_opacity.is_some()
        {
            let mut recipe =
                img_core::media::Recipe::preset(self.preset.as_deref().unwrap_or("original"));
            if let Some(format) = &self.image_format {
                recipe.format = format.clone();
            }
            if let Some(quality) = self.quality {
                recipe.quality = quality;
            }
            if let Some(edge) = self.max_edge {
                recipe.max_edge = edge;
            }
            if let Some(path) = &self.watermark {
                recipe.watermark = path.to_string_lossy().into_owned();
            }
            if let Some(opacity) = self.watermark_opacity {
                recipe.opacity = opacity;
            }
            Some(recipe)
        } else {
            None
        };
        img_core::upload::Options {
            recipe,
            reuse: self.reuse,
            force: self.force,
            record_origin: if self.no_history {
                None
            } else {
                Some(if self.origin.is_empty() {
                    if std::env::var("IMG_DESKTOP_UPLOAD").as_deref() == Ok("1") {
                        "desktop".into()
                    } else {
                        "cli".into()
                    }
                } else {
                    self.origin.clone()
                })
            },
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
    #[arg(long, help = "Include images in directories and their subdirectories")]
    pub recursive: bool,
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
    #[arg(
        long,
        help = "List image references without uploading or changing documents"
    )]
    pub dry_run: bool,
    #[arg(long, help = "Write per-image successes and failures to a JSON report")]
    pub report: Option<PathBuf>,
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
    #[arg(
        long,
        default_value = "",
        help = "Authorization header reference, for HTTP or WebDAV"
    )]
    pub authorization: String,
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

#[derive(Subcommand)]
pub enum LibraryCommand {
    Index {
        #[arg(long)]
        provider: String,
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long)]
        resume: bool,
    },
    List {
        #[arg(long, default_value = "")]
        search: String,
        #[arg(long, default_value = "")]
        provider: String,
        #[arg(long, default_value = "")]
        prefix: String,
        #[arg(long, default_value = "")]
        content_type: String,
        #[arg(long, default_value = "")]
        origin: String,
        #[arg(long, default_value = "")]
        availability: String,
        #[arg(long)]
        hidden: bool,
        #[arg(long, default_value_t = 200)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    Show {
        id: String,
    },
    Check {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long, default_value = "")]
        provider: String,
        #[arg(long)]
        allow_insecure: bool,
    },
    Download {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        output_dir: PathBuf,
        #[arg(long, default_value = "")]
        provider: String,
    },
    Hide {
        #[arg(required = true)]
        ids: Vec<String>,
        #[arg(long)]
        restore: bool,
    },
    Cache {
        #[arg(long)]
        clear: bool,
    },
}
