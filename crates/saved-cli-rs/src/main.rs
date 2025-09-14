//! SAVED CLI - Command-line interface for SAVED Personal Messages Sync
//!
//! This CLI provides a simple interface to interact with the SAVED core library,
//! allowing users to create accounts, manage messages, and sync between devices.

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

mod commands;
mod utils;

use commands::*;

#[derive(Parser)]
#[command(
    name = "saved",
    about = "SAVED - Personal Messages Sync CLI",
    version = "0.1.0",
    long_about = "A private, end-to-end encrypted personal vault that syncs notes, messages, and files across your own devices using P2P networking."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the account storage directory
    #[arg(short, long, default_value = "./saved-account")]
    account_path: PathBuf,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new SAVED account
    Init {
        /// Account name
        #[arg(short, long, default_value = "My Account")]
        name: String,
    },
    /// Show account information
    Info,
    /// Create a new message
    Create {
        /// Message content
        #[arg(short, long)]
        content: String,
        /// Attach files to the message
        #[arg(short, long)]
        attach: Vec<PathBuf>,
    },
    /// List all messages
    List {
        /// Show only message IDs
        #[arg(short, long)]
        ids_only: bool,
        /// Limit number of messages to show
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Edit an existing message
    Edit {
        /// Message ID to edit
        message_id: String,
        /// New content
        #[arg(short, long)]
        content: String,
    },
    /// Delete a message
    Delete {
        /// Message ID to delete
        message_id: String,
        /// Permanently delete (purge)
        #[arg(short, long)]
        permanent: bool,
    },
    /// Show message content
    Show {
        /// Message ID to show
        message_id: String,
    },
    /// Generate QR code for device linking
    Link {
        /// Save QR code to file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Accept device link from QR code
    Accept {
        /// QR code payload (JSON)
        payload: String,
    },
    /// Start network sync
    Sync {
        /// Run in background
        #[arg(short, long)]
        daemon: bool,
    },
    /// Show connected devices
    Devices,
    /// Show sync status
    Status,
    /// Export messages to JSON
    Export {
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Import messages from JSON
    Import {
        /// Input file path
        input: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize colored output
    colored::control::set_override(true);

    // Print banner
    if !cli.verbose {
        println!("{}", "SAVED - Personal Messages Sync".bright_blue().bold());
        println!("{}", "=".repeat(30).bright_blue());
    }

    // Execute command
    match cli.command {
        Commands::Init { name } => {
            init_command(&cli.account_path, &name, cli.verbose).await?;
        }
        Commands::Info => {
            info_command(&cli.account_path, cli.verbose).await?;
        }
        Commands::Create { content, attach } => {
            create_command(&cli.account_path, &content, attach, cli.verbose).await?;
        }
        Commands::List { ids_only, limit } => {
            list_command(&cli.account_path, ids_only, limit, cli.verbose).await?;
        }
        Commands::Edit {
            message_id,
            content,
        } => {
            edit_command(&cli.account_path, &message_id, &content, cli.verbose).await?;
        }
        Commands::Delete {
            message_id,
            permanent,
        } => {
            delete_command(&cli.account_path, &message_id, permanent, cli.verbose).await?;
        }
        Commands::Show { message_id } => {
            show_command(&cli.account_path, &message_id, cli.verbose).await?;
        }
        Commands::Link { output } => {
            link_command(&cli.account_path, output, cli.verbose).await?;
        }
        Commands::Accept { payload } => {
            accept_command(&cli.account_path, &payload, cli.verbose).await?;
        }
        Commands::Sync { daemon } => {
            sync_command(&cli.account_path, daemon, cli.verbose).await?;
        }
        Commands::Devices => {
            devices_command(&cli.account_path, cli.verbose).await?;
        }
        Commands::Status => {
            status_command(&cli.account_path, cli.verbose).await?;
        }
        Commands::Export { output } => {
            export_command(&cli.account_path, &output, cli.verbose).await?;
        }
        Commands::Import { input } => {
            import_command(&cli.account_path, &input, cli.verbose).await?;
        }
    }

    Ok(())
}
