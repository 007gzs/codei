use anyhow::{Context, Result};
use codei_config::{init_user_config, load, load_mcp_config, ResolvedConfig};
use codei_i18n::{self, t_fmt};
use codei_sdk::{run_server, ServerOptions};
use codei_session::SessionStore;

use crate::agent::run_agent;
use crate::cli::{Cli, Commands, ConfigCommands};
use crate::mcp::McpCommands;
use crate::session::SessionCommands;

pub async fn run(cli: Cli) -> Result<()> {
    let load_opts = cli.load_options();
    let resolved = load(&load_opts).context("failed to load configuration")?;
    let language = cli
        .language
        .as_deref()
        .unwrap_or(&resolved.config.defaults.language);
    codei_i18n::init(language);

    match cli.command {
        Some(Commands::Config { command }) => match command {
            ConfigCommands::Show => show_config(&resolved),
            ConfigCommands::Init => init_config(),
        },
        Some(Commands::Session { command }) => run_session_command(&resolved, command),
        Some(Commands::Mcp { command }) => run_mcp_command(command),
        Some(Commands::Server { ref host, port }) => {
            run_server_command(&cli, host.clone(), port).await
        }
        Some(Commands::Version) => {
            println!("codei {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        None => run_agent(&cli, resolved).await,
    }
}

async fn run_server_command(cli: &Cli, host: String, port: u16) -> Result<()> {
    let default_cwd = cli
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current directory"));
    run_server(ServerOptions {
        host,
        port,
        default_cwd,
        verbose: cli.verbose,
    })
    .await
}

fn run_session_command(resolved: &ResolvedConfig, command: SessionCommands) -> Result<()> {
    let store =
        SessionStore::open_for_config(&resolved.config.session).context("open session store")?;
    match command {
        SessionCommands::List { limit } => {
            let sessions = store.list(limit)?;
            if sessions.is_empty() {
                println!("No sessions found.");
                return Ok(());
            }
            for s in sessions {
                let title = s.title.unwrap_or_else(|| "(untitled)".into());
                println!(
                    "{}  {}  cwd={}  updated={}",
                    s.id,
                    title,
                    s.cwd.display(),
                    s.updated_at.format("%Y-%m-%d %H:%M")
                );
            }
        }
        SessionCommands::Delete { id } => {
            store.delete(&id)?;
            println!("Deleted session {id}");
        }
        SessionCommands::Export { id, format } => {
            if format != "jsonl" {
                anyhow::bail!("unsupported export format: {format} (only jsonl)");
            }
            println!("{}", store.export_jsonl(&id)?);
        }
    }
    Ok(())
}

fn run_mcp_command(command: McpCommands) -> Result<()> {
    match command {
        McpCommands::List => {
            let config = load_mcp_config()?;
            if config.servers.is_empty() {
                println!("No MCP servers configured.");
                println!(
                    "Run `codei mcp init` or edit {}",
                    codei_config::mcp_config_path().display()
                );
                return Ok(());
            }
            for server in config.servers {
                println!("{}  {} {:?}", server.name, server.command, server.args);
            }
        }
        McpCommands::Init => {
            let (path, created) = codei_config::init_mcp_config()?;
            if created {
                println!("Created MCP config at {}", path.display());
            } else {
                println!("MCP config already exists at {}", path.display());
            }
        }
        McpCommands::Add { name, command } => {
            if command.is_empty() {
                anyhow::bail!("usage: codei mcp add <name> -- <command> [args...]");
            }
            let mut config = load_mcp_config()?;
            if config.servers.iter().any(|s| s.name == name) {
                anyhow::bail!("MCP server `{name}` already exists");
            }
            let (cmd, args) = command.split_first().expect("checked non-empty");
            config.servers.push(codei_config::McpServer {
                name,
                command: cmd.clone(),
                args: args.to_vec(),
                env: Vec::new(),
            });
            codei_config::save_mcp_config(&config)?;
            println!(
                "Added MCP server `{}`",
                config.servers.last().expect("just pushed").name
            );
        }
    }
    Ok(())
}

fn show_config(resolved: &ResolvedConfig) -> Result<()> {
    println!("{}", codei_i18n::t("config_show_title"));
    println!();

    let toml = toml::to_string_pretty(&resolved.config).context("serialize config")?;
    println!("{toml}");

    println!();
    println!("{}", codei_i18n::t("config_show_paths_title"));
    println!(
        "  {}: {}",
        codei_i18n::t("config_path_user"),
        resolved.user_config_path.display()
    );
    match &resolved.project_config_path {
        Some(path) if path.is_file() => {
            println!(
                "  {}: {}",
                codei_i18n::t("config_path_project"),
                path.display()
            );
        }
        _ => {
            println!(
                "  {}: {}",
                codei_i18n::t("config_path_project"),
                codei_i18n::t("config_path_none")
            );
        }
    }
    println!(
        "  {}: {}",
        codei_i18n::t("config_path_cwd"),
        resolved.cwd.display()
    );
    match &resolved.project_root {
        Some(root) => println!(
            "  {}: {}",
            codei_i18n::t("config_path_project_root"),
            root.display()
        ),
        None => println!(
            "  {}: {}",
            codei_i18n::t("config_path_project_root"),
            codei_i18n::t("config_path_none")
        ),
    }

    Ok(())
}

fn init_config() -> Result<()> {
    let (path, created) = init_user_config().context("initialize user configuration")?;
    let key = if created {
        "config_init_created"
    } else {
        "config_init_exists"
    };
    println!("{}", t_fmt(key, &[("path", &path.display().to_string())]));
    Ok(())
}
