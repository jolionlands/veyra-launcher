use std::fmt;
use std::path::PathBuf;

use veyra_import::import_keypirinha_profile;
use veyra_platform::profile_dir;

fn main() {
    if let Err(error) = run() {
        match error {
            CliError::Help(message) => {
                println!("{message}\n\n{}", usage());
            }
            error => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}

fn run() -> Result<(), CliError> {
    let args = parse_args(std::env::args().skip(1))?;

    match args.command {
        ImportCommand::Keypirinha {
            source,
            profile,
            output,
            dry_run,
            force,
        } => {
            let imported = import_keypirinha_profile(&source)?;
            let toml = imported.to_toml_string()?;

            if dry_run {
                print!("{toml}");
                return Ok(());
            }

            let output = output.unwrap_or_else(|| {
                profile
                    .unwrap_or_else(|| profile_dir("Veyra"))
                    .join("commands.toml")
            });

            if output.exists() && !force {
                return Err(CliError::RefusingOverwrite(output));
            }

            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output, toml)?;

            println!(
                "Imported {} commands, {} web searches, and {} catalogs to {}",
                imported.commands.len(),
                imported.web_search.len(),
                imported.catalogs.len(),
                output.display()
            );
        }
    }

    Ok(())
}

#[derive(Debug)]
struct CliArgs {
    command: ImportCommand,
}

#[derive(Debug)]
enum ImportCommand {
    Keypirinha {
        source: PathBuf,
        profile: Option<PathBuf>,
        output: Option<PathBuf>,
        dry_run: bool,
        force: bool,
    },
}

#[derive(Debug)]
enum CliError {
    Help(String),
    Usage(String),
    Import(veyra_import::ImportError),
    Serialize(toml::ser::Error),
    Io(std::io::Error),
    RefusingOverwrite(PathBuf),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Help(message) => write!(formatter, "{message}\n\n{}", usage()),
            CliError::Usage(message) => write!(formatter, "{message}\n\n{}", usage()),
            CliError::Import(error) => write!(formatter, "{error}"),
            CliError::Serialize(error) => {
                write!(formatter, "could not serialize imported profile: {error}")
            }
            CliError::Io(error) => write!(formatter, "{error}"),
            CliError::RefusingOverwrite(path) => write!(
                formatter,
                "refusing to overwrite {}; pass --force or choose --output",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CliError {}

impl From<veyra_import::ImportError> for CliError {
    fn from(error: veyra_import::ImportError) -> Self {
        Self::Import(error)
    }
}

impl From<toml::ser::Error> for CliError {
    fn from(error: toml::ser::Error) -> Self {
        Self::Serialize(error)
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

fn parse_args(raw_args: impl IntoIterator<Item = String>) -> Result<CliArgs, CliError> {
    let mut args = raw_args.into_iter();
    let Some(command) = args.next() else {
        return Err(CliError::Usage("missing import command".to_string()));
    };

    match command.as_str() {
        "keypirinha" => parse_keypirinha_args(args),
        "-h" | "--help" | "help" => Err(CliError::Help("Veyra profile importer".to_string())),
        _ => Err(CliError::Usage(format!(
            "unknown import command: {command}"
        ))),
    }
}

fn parse_keypirinha_args(raw_args: impl IntoIterator<Item = String>) -> Result<CliArgs, CliError> {
    let mut source = None;
    let mut profile = None;
    let mut output = None;
    let mut dry_run = false;
    let mut force = false;
    let mut args = raw_args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => source = Some(required_value("--source", &mut args)?),
            "--profile" => profile = Some(required_value("--profile", &mut args)?),
            "--output" => output = Some(required_value("--output", &mut args)?),
            "--dry-run" => dry_run = true,
            "--force" => force = true,
            "-h" | "--help" => {
                return Err(CliError::Help("Keypirinha importer".to_string()));
            }
            _ => return Err(CliError::Usage(format!("unknown argument: {arg}"))),
        }
    }

    let Some(source) = source else {
        return Err(CliError::Usage(
            "missing required --source <path>".to_string(),
        ));
    };

    Ok(CliArgs {
        command: ImportCommand::Keypirinha {
            source,
            profile,
            output,
            dry_run,
            force,
        },
    })
}

fn required_value(
    flag: &str,
    args: &mut impl Iterator<Item = String>,
) -> Result<PathBuf, CliError> {
    let Some(value) = args.next() else {
        return Err(CliError::Usage(format!("missing value for {flag}")));
    };

    if value.starts_with("--") {
        return Err(CliError::Usage(format!("missing value for {flag}")));
    }

    Ok(PathBuf::from(value))
}

fn usage() -> &'static str {
    "Usage:
  veyra-import keypirinha --source <keypirinha-root> [--profile <veyra-profile-dir>]
  veyra-import keypirinha --source <keypirinha-root> --output <commands.toml>
  veyra-import keypirinha --source <keypirinha-root> --dry-run

Options:
  --source <path>   Keypirinha portable root or folder containing Apps.ini/WebSearch.ini
  --profile <path>  Veyra profile directory; defaults to the current platform profile
  --output <path>   Exact commands.toml path to write
  --dry-run         Print generated TOML instead of writing
  --force           Overwrite an existing output file"
}
